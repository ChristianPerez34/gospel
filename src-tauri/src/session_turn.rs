use crate::harness_profile::ActiveWorkspaceContext;
use crate::llm::{self, LlmError, StreamCompletionResult, StreamEvent};
use crate::models::ModelRegistry;
use crate::session_mode::{SessionMode, SESSION_MODE_BUILD};
use crate::session_store::SessionNote;
use crate::skills::{self, RunSkillScriptTool, Skill};
use crate::trace;
use crate::verification::{VerificationResult, VerificationStatus};
use rig::completion::message::{AssistantContent, Message, UserContent};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
};

#[derive(Debug, Clone, Deserialize)]
pub struct InvokedSkillRequest {
    pub name: String,
    #[serde(default)]
    pub args: Option<String>,
}

pub struct StreamingTurnRequest {
    pub provider: String,
    pub prompt: String,
    pub model: String,
    pub variant: Option<String>,
    pub session_id: Option<String>,
    pub invoked_skill: Option<InvokedSkillRequest>,
    pub delegate_provider: String,
    pub delegate_model: String,
    pub delegate_api_key: String,
}

pub type SessionTurnFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub struct StreamingTurnDependencies<'a> {
    pub workspace: &'a dyn SessionTurnWorkspace,
    pub credentials: &'a dyn SessionTurnCredentials,
    pub sessions: &'a dyn SessionTurnSessions,
    pub conversation: &'a dyn SessionTurnConversation,
    pub skills: &'a dyn SessionTurnSkills,
    pub llm: &'a dyn SessionTurnLlm,
    pub events: &'a dyn SessionTurnEvents,
    pub verification: &'a dyn SessionTurnVerification,
}

pub trait SessionTurnWorkspace: Send + Sync {
    fn active_workspace_selection(&self) -> Option<ActiveWorkspaceSelection>;

    fn validate_workspace_path(&self, workspace_path: &Path) -> Result<(), String>;

    fn ensure_workspace_corpus<'a>(
        &'a self,
        workspace_path: &'a Path,
    ) -> SessionTurnFuture<'a, Result<Option<usize>, String>>;

    fn emit_corpus_auto_build_failure(&self);
}

pub trait SessionTurnCredentials: Send + Sync {
    fn api_key(&self, provider: &str) -> Result<String, LlmError>;
}

pub trait SessionTurnSessions: Send + Sync {
    fn validate_workspace_binding(
        &self,
        session_id: &str,
        active_workspace_id: Option<&str>,
    ) -> Result<(), String>;

    fn session_mode(&self, session_id: &str) -> Result<String, String>;

    fn unresolved_notes(&self, session_id: &str) -> Vec<SessionNote>;

    fn failure_snapshot(&self, session_id: &str) -> Option<SessionFailureSnapshot>;

    fn persist_turn(
        &self,
        session_id: &str,
        display_transcript: &str,
        model_history: Option<&str>,
    ) -> Result<(), String>;

    fn update_model_selection(
        &self,
        session_id: &str,
        provider: &str,
        model: &str,
        variant: Option<&str>,
    ) -> Result<(), String>;

    fn update_status(&self, session_id: &str, status: &str) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFailureSnapshot {
    pub display_transcript: String,
    pub model_history: Option<String>,
}

pub trait SessionTurnConversation: Send + Sync {
    fn chat_history(&self, session_id: Option<&str>) -> Vec<Message>;
    fn store_history(&self, session_id: &str, history: Vec<Message>);
}

pub trait SessionTurnSkills: Send + Sync {
    fn load_skills(&self, workspace_path: Option<&Path>) -> Vec<Skill>;
}

pub struct SessionTurnStreamRequest<'a> {
    pub provider: &'a str,
    pub prompt: &'a str,
    pub model: &'a str,
    pub variant: Option<&'a str>,
    pub api_key: &'a str,
    pub delegate_provider: &'a str,
    pub delegate_model: &'a str,
    pub delegate_api_key: &'a str,
    pub workspace: Option<ActiveWorkspaceContext>,
    pub chat_history: Vec<Message>,
    pub matched_skills_section: Option<String>,
    pub invoked_skill_section: Option<String>,
    pub skill_script_tool: Option<RunSkillScriptTool>,
}

pub trait SessionTurnLlm: Send + Sync {
    fn stream_completion<'a>(
        &'a self,
        request: SessionTurnStreamRequest<'a>,
        on_event: Box<dyn FnMut(SessionTurnEvent) + Send + 'a>,
    ) -> SessionTurnFuture<'a, Result<StreamCompletionResult, LlmError>>;
}

pub trait SessionTurnEvents: Send + Sync {
    fn emit_stream_event(&self, session_id: &str, role: &str, event: &SessionTurnEvent);
    fn trace_done(
        &self,
        session_id: &str,
        role: &str,
        response_length: usize,
        prompt_tokens: usize,
        response_tokens: usize,
        tool_calls: usize,
    );
    fn trace_error(&self, session_id: &str, role: &str, error: &LlmError);
    fn emit_done(
        &self,
        response: &str,
        prompt_tokens: usize,
        response_tokens: usize,
        tool_calls: usize,
    );
    fn emit_error(&self, error: &LlmError);
}

pub trait SessionTurnVerification: Send + Sync {
    fn schedule_verification(&self, job: VerificationJobRequest);
}

pub async fn run_streaming_turn(
    deps: StreamingTurnDependencies<'_>,
    request: StreamingTurnRequest,
) -> Result<(), llm::LlmErrorDto> {
    let active_workspace = deps.workspace.active_workspace_selection();
    let workspace_resolution =
        resolve_streaming_active_workspace(deps.workspace, active_workspace).await;
    let workspace_path = workspace_resolution.workspace_path.clone();
    let mut workspace_context = workspace_resolution.tool_context.clone();

    eprintln!(
        "[CORPUS-AUTO] complete_streaming workspace path: {}",
        workspace_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".to_string())
    );

    let api_key = deps
        .credentials
        .api_key(&request.provider)
        .map_err(|error| error.to_dto())?;

    let session_mode = if let Some(sid) = &request.session_id {
        if let Err(e) = deps
            .sessions
            .validate_workspace_binding(sid, workspace_resolution.workspace_id.as_deref())
        {
            return Err(LlmError::ProviderError(e.to_string()).to_dto());
        }
        deps.sessions
            .session_mode(sid)
            .map_err(|e| LlmError::ProviderError(e).to_dto())?
    } else {
        SESSION_MODE_BUILD.to_string()
    };
    if let Some(context) = workspace_context.as_mut() {
        context.session_mode = SessionMode::from_stored(&session_mode);
    }
    let resolved_model_variant = ModelRegistry::resolve_model_variant(
        &request.provider,
        &request.model,
        request.variant.as_deref(),
    );
    if let Some(sid) = request.session_id.as_deref() {
        deps.sessions
            .update_model_selection(
                sid,
                &request.provider,
                &request.model,
                resolved_model_variant.variant.as_deref(),
            )
            .map_err(|e| LlmError::ProviderError(e).to_dto())?;
    }

    let chat_history = deps
        .conversation
        .chat_history(request.session_id.as_deref());

    let all_skills = deps.skills.load_skills(workspace_path.as_deref());
    let unresolved_notes = if let Some(sid) = request.session_id.as_deref() {
        deps.sessions.unresolved_notes(sid)
    } else {
        Vec::new()
    };
    let prompt_preparation = prepare_prompt(
        &request.prompt,
        &all_skills,
        request.invoked_skill.as_ref(),
        &unresolved_notes,
    );
    if let Some(name) = prompt_preparation.unknown_invoked_skill.as_deref() {
        tracing::warn!("Unknown skill '{}'; proceeding as normal turn", name);
    }
    let skill_script_tool = skill_script_tool(&all_skills, workspace_path.clone());

    let trace_sid = request.session_id.clone().unwrap_or_default();
    let trace_role = "main";
    let events = deps.events;
    let workspace_for_verify = workspace_context.clone();
    let result = deps
        .llm
        .stream_completion(
            SessionTurnStreamRequest {
                provider: &request.provider,
                prompt: &prompt_preparation.effective_prompt,
                model: &request.model,
                variant: request.variant.as_deref(),
                api_key: &api_key,
                delegate_provider: &request.delegate_provider,
                delegate_model: &request.delegate_model,
                delegate_api_key: &request.delegate_api_key,
                workspace: workspace_context,
                chat_history,
                matched_skills_section: prompt_preparation.matched_skills_section.clone(),
                invoked_skill_section: prompt_preparation.invoked_skill_section.clone(),
                skill_script_tool,
            },
            Box::new(move |event| {
                events.emit_stream_event(&trace_sid, trace_role, &event);
            }),
        )
        .await;

    match result {
        Ok(stream_result) => {
            deps.events.trace_done(
                request.session_id.as_deref().unwrap_or(""),
                "main",
                stream_result.full_response.len(),
                stream_result.prompt_tokens,
                stream_result.response_tokens,
                stream_result.tool_calls,
            );
            if let (Some(sid), Some(persistence)) = (
                &request.session_id,
                successful_turn_persistence(stream_result.history.as_deref()),
            ) {
                if let Err(e) = deps.sessions.persist_turn(
                    sid,
                    &persistence.display_transcript,
                    persistence.model_history.as_deref(),
                ) {
                    tracing::warn!("Failed to persist session {}: {}", sid, e);
                } else {
                    if let Err(e) = deps.sessions.update_status(sid, "active") {
                        tracing::warn!("Failed to update session {} status: {}", sid, e);
                    }
                    deps.conversation
                        .store_history(sid, persistence.history.clone());
                }
            }

            let response_for_verify = stream_result.full_response.clone();
            deps.events.emit_done(
                &response_for_verify,
                stream_result.prompt_tokens,
                stream_result.response_tokens,
                stream_result.tool_calls,
            );

            if let Some(job) = verification_job_request(
                &request.provider,
                &request.model,
                &api_key,
                workspace_for_verify.as_ref(),
                &response_for_verify,
                &prompt_preparation.effective_prompt,
                request.session_id.as_deref(),
                stream_result.source_edit_succeeded,
            ) {
                deps.verification.schedule_verification(job);
            }

            Ok(())
        }
        Err(e) => {
            deps.events
                .trace_error(request.session_id.as_deref().unwrap_or(""), "main", &e);
            if let Some(sid) = &request.session_id {
                if let Some(snapshot) = deps.sessions.failure_snapshot(sid) {
                    let persistence = failure_turn_persistence(
                        &snapshot.display_transcript,
                        snapshot.model_history.as_deref(),
                        &e,
                    );
                    let _ = deps.sessions.persist_turn(
                        sid,
                        &persistence.display_transcript,
                        persistence.model_history.as_deref(),
                    );
                }
            }
            deps.events.emit_error(&e);
            Err(e.to_dto())
        }
    }
}

async fn resolve_streaming_active_workspace(
    workspace: &dyn SessionTurnWorkspace,
    selection: Option<ActiveWorkspaceSelection>,
) -> ResolvedActiveWorkspaceContext {
    let probe = match selection {
        Some(selection) => match workspace.validate_workspace_path(&selection.path) {
            Ok(()) => match workspace.ensure_workspace_corpus(&selection.path).await {
                Ok(_) => ActiveWorkspaceProbe::CorpusAvailable { selection },
                Err(reason) => {
                    tracing::warn!(
                        "[CORPUS-AUTO] continuing without corpus for {}: {}",
                        selection.path.display(),
                        reason
                    );
                    ActiveWorkspaceProbe::CorpusUnavailable { selection, reason }
                }
            },
            Err(reason) => {
                tracing::warn!(
                    "[CORPUS-AUTO] workspace tools unavailable for {}: {}",
                    selection.path.display(),
                    reason
                );
                ActiveWorkspaceProbe::Invalid { selection, reason }
            }
        },
        None => ActiveWorkspaceProbe::NoWorkspace,
    };

    let resolution = resolve_active_workspace_context(probe);
    if resolution.corpus_failure_reason.is_some() {
        workspace.emit_corpus_auto_build_failure();
    }
    resolution
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptPreparation {
    pub effective_prompt: String,
    pub matched_skills_section: Option<String>,
    pub invoked_skill_section: Option<String>,
    pub unknown_invoked_skill: Option<String>,
}

pub fn prepare_prompt(
    prompt: &str,
    all_skills: &[Skill],
    invoked_skill: Option<&InvokedSkillRequest>,
    unresolved_notes: &[SessionNote],
) -> PromptPreparation {
    let (matched_skills_section, invoked_skill_section, effective_prompt, unknown_invoked_skill) =
        if let Some(invoked) = invoked_skill {
            match all_skills.iter().find(|skill| skill.name == invoked.name) {
                Some(skill) => {
                    let preamble = skills::format_invoked_skill_preamble(skill);
                    let user_msg = invoked
                        .args
                        .as_deref()
                        .map(|args| args.trim().to_string())
                        .filter(|args| !args.is_empty())
                        .unwrap_or_else(|| prompt.to_string());
                    (None, Some(preamble), user_msg, None)
                }
                None => (None, None, prompt.to_string(), Some(invoked.name.clone())),
            }
        } else {
            let matched = skills::match_skills(prompt, all_skills);
            (
                skills::format_skills_preamble_section(&matched),
                None,
                prompt.to_string(),
                None,
            )
        };

    PromptPreparation {
        effective_prompt: append_unresolved_notes(effective_prompt, unresolved_notes),
        matched_skills_section,
        invoked_skill_section,
        unknown_invoked_skill,
    }
}

pub fn skill_script_tool(
    all_skills: &[Skill],
    workspace_path: Option<PathBuf>,
) -> Option<RunSkillScriptTool> {
    let scriptable: Vec<_> = all_skills
        .iter()
        .filter(|skill| !skill.scripts.is_empty())
        .cloned()
        .collect();

    if scriptable.is_empty() {
        None
    } else {
        Some(RunSkillScriptTool {
            available_skills: scriptable,
            workspace_path,
            command_approval: None,
        })
    }
}

fn append_unresolved_notes(prompt: String, unresolved_notes: &[SessionNote]) -> String {
    if unresolved_notes.is_empty() {
        return prompt;
    }

    let concerns_text: Vec<String> = unresolved_notes
        .iter()
        .map(|note| format!("- [{}] {}", note.note_type, note.content))
        .collect();

    format!(
        "{}\n\n## Previous Verification Concerns\nThe following issues were flagged by the verification agent in a previous response. Please address these if applicable:\n{}",
        prompt,
        concerns_text.join("\n")
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct SuccessfulTurnPersistence {
    pub display_transcript: String,
    pub model_history: Option<String>,
    pub history: Vec<Message>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureTurnPersistence {
    pub display_transcript: String,
    pub model_history: Option<String>,
}

pub fn successful_turn_persistence(
    history: Option<&[Message]>,
) -> Option<SuccessfulTurnPersistence> {
    let history = history?;
    let display_transcript = serde_json::to_string(&build_display_transcript(history))
        .unwrap_or_else(|_| "[]".to_string());
    let model_history = serde_json::to_string(history).ok();

    Some(SuccessfulTurnPersistence {
        display_transcript,
        model_history,
        history: history.to_vec(),
    })
}

pub fn failure_turn_persistence(
    existing_display_transcript: &str,
    existing_model_history: Option<&str>,
    error: &LlmError,
) -> FailureTurnPersistence {
    let mut transcript: Vec<serde_json::Value> =
        serde_json::from_str(existing_display_transcript).unwrap_or_default();
    let (message, is_controlled_stop) = match error {
        LlmError::ControlledStop(message) => (message.clone(), true),
        _ => (format!("Error: {}", error.to_dto().message), false),
    };

    transcript.push(json!({
        "role": "assistant",
        "content": message,
        "blocks": [],
        "error": !is_controlled_stop,
        "controlled_stop": is_controlled_stop,
    }));

    FailureTurnPersistence {
        display_transcript: serde_json::to_string(&transcript).unwrap_or_else(|_| "[]".to_string()),
        model_history: existing_model_history.map(str::to_string),
    }
}

pub fn build_display_transcript(messages: &[Message]) -> Vec<serde_json::Value> {
    // First pass: build assistant entries with ordered `blocks` (text + tool calls)
    // and collect tool-result text keyed by tool-call id. Tool results in rig's
    // history live on a following `Message::User { content: ToolResult }` entry,
    // so a second pass stitches each result onto its matching `tool` block. Any
    // unmatched tool result is emitted as its own block so nothing is lost.
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut pending_tool_results: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for message in messages {
        match message {
            Message::User { content } => {
                let mut text_parts: Vec<String> = Vec::new();
                for item in content.iter() {
                    match item {
                        UserContent::Text(text) => text_parts.push(text.text.clone()),
                        UserContent::ToolResult(tool_result) => {
                            let result_text: String = tool_result
                                .content
                                .iter()
                                .map(|part| match part {
                                    rig::completion::message::ToolResultContent::Text(t) => {
                                        t.text.clone()
                                    }
                                    _ => String::new(),
                                })
                                .collect::<Vec<_>>()
                                .join("");
                            pending_tool_results.insert(tool_result.id.clone(), result_text);
                        }
                        _ => {}
                    }
                }
                if !text_parts.is_empty() {
                    entries.push(json!({
                        "role": "user",
                        "content": text_parts.join(""),
                    }));
                }
            }
            Message::Assistant { content, .. } => {
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                let mut text_parts: Vec<String> = Vec::new();
                for item in content.iter() {
                    match item {
                        AssistantContent::Text(text) => {
                            text_parts.push(text.text.clone());
                            blocks.push(json!({
                                "kind": "text",
                                "id": format!("text-{}", blocks.len()),
                                "text": text.text.clone(),
                            }));
                        }
                        AssistantContent::ToolCall(tool_call) => {
                            blocks.push(json!({
                                "kind": "tool",
                                "id": tool_call.id.clone(),
                                "name": tool_call.function.name.clone(),
                                "arguments": observable_tool_arguments(
                                    &tool_call.function.name,
                                    &tool_call.function.arguments,
                                ),
                                "result": null,
                                "status": "completed",
                            }));
                        }
                        _ => {}
                    }
                }
                if blocks.is_empty() && !text_parts.is_empty() {
                    // Defensive: shouldn't happen given the loop above, but keep
                    // a sane fallback.
                    blocks.push(json!({
                        "kind": "text",
                        "id": "text-0",
                        "text": text_parts.join(""),
                    }));
                }
                if blocks.is_empty() {
                    continue;
                }
                entries.push(json!({
                    "role": "assistant",
                    "content": text_parts.join(""),
                    "blocks": blocks,
                }));
            }
            _ => {}
        }
    }

    // Second pass: attach each collected tool result to its matching `tool`
    // block by id. Unmatched results are appended as their own assistant-level
    // tool block so no tool output is silently dropped.
    if !pending_tool_results.is_empty() {
        for entry in entries.iter_mut() {
            if entry.get("role").and_then(|r| r.as_str()) != Some("assistant") {
                continue;
            }
            if let Some(blocks) = entry.get_mut("blocks").and_then(|b| b.as_array_mut()) {
                for block in blocks.iter_mut() {
                    if block.get("kind").and_then(|k| k.as_str()) != Some("tool") {
                        continue;
                    }
                    if let Some(id) = block.get("id").and_then(|i| i.as_str()) {
                        if let Some(result) = pending_tool_results.remove(id) {
                            block["result"] = serde_json::Value::String(result);
                        }
                    }
                }
            }
        }
        for (id, result) in pending_tool_results.drain() {
            entries.push(json!({
                "role": "assistant",
                "content": "",
                "blocks": [
                    {
                        "kind": "tool",
                        "id": id,
                        "name": "",
                        "arguments": null,
                        "result": result,
                        "status": "completed",
                    }
                ],
            }));
        }
    }

    entries
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SessionTurnEvent {
    TextToken(String),
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ToolResult {
        id: String,
        name: String,
        result: String,
    },
    LoopWarning {
        count: usize,
        tool_name: String,
    },
    LoopStopped {
        count: usize,
        tool_name: String,
        message: String,
    },
    ModelVariantWarning {
        kind: String,
        provider: String,
        model: String,
        variant: String,
        message: String,
    },
}

impl From<StreamEvent> for SessionTurnEvent {
    fn from(event: StreamEvent) -> Self {
        match event {
            StreamEvent::Text(token) => SessionTurnEvent::TextToken(token),
            StreamEvent::ToolCall {
                id,
                name,
                arguments,
            } => SessionTurnEvent::ToolCall {
                id,
                name,
                arguments,
            },
            StreamEvent::ToolResult { id, name, result } => {
                SessionTurnEvent::ToolResult { id, name, result }
            }
            StreamEvent::LoopWarning { count, tool_name } => {
                SessionTurnEvent::LoopWarning { count, tool_name }
            }
            StreamEvent::LoopStopped {
                count,
                tool_name,
                message,
            } => SessionTurnEvent::LoopStopped {
                count,
                tool_name,
                message,
            },
            StreamEvent::ModelVariantWarning {
                kind,
                provider,
                model,
                variant,
                message,
            } => SessionTurnEvent::ModelVariantWarning {
                kind,
                provider,
                model,
                variant,
                message,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiEventPayload {
    pub name: &'static str,
    pub payload: serde_json::Value,
}

pub fn ui_event_payload(event: &SessionTurnEvent) -> UiEventPayload {
    match event {
        SessionTurnEvent::TextToken(token) => UiEventPayload {
            name: "llm-token",
            payload: serde_json::Value::String(token.clone()),
        },
        SessionTurnEvent::ToolCall {
            id,
            name,
            arguments,
        } => UiEventPayload {
            name: "llm-tool-call",
            payload: json!({
                "id": id,
                "name": name,
                "arguments": observable_tool_arguments(name, arguments)
            }),
        },
        SessionTurnEvent::ToolResult { id, name, result } => UiEventPayload {
            name: "llm-tool-result",
            payload: json!({ "id": id, "name": name, "result": result }),
        },
        SessionTurnEvent::LoopWarning { count, tool_name } => UiEventPayload {
            name: "llm-loop-warning",
            payload: json!({ "count": count, "toolName": tool_name }),
        },
        SessionTurnEvent::LoopStopped {
            count,
            tool_name,
            message,
        } => UiEventPayload {
            name: "llm-loop-stopped",
            payload: json!({ "count": count, "toolName": tool_name, "message": message }),
        },
        SessionTurnEvent::ModelVariantWarning {
            kind,
            provider,
            model,
            variant,
            message,
        } => UiEventPayload {
            name: "llm-model-variant-warning",
            payload: json!({
                "kind": kind,
                "provider": provider,
                "model": model,
                "variant": variant,
                "message": message
            }),
        },
    }
}

pub fn trace_event_for_session_turn_event(
    event: &SessionTurnEvent,
    session_id: &str,
    role: &str,
    timestamp: u64,
) -> Option<trace::TraceEvent> {
    match event {
        SessionTurnEvent::TextToken(_) => None,
        SessionTurnEvent::ToolCall {
            name, arguments, ..
        } => Some(trace::TraceEvent::ToolCall {
            session_id: session_id.to_string(),
            role: role.to_string(),
            tool_name: name.clone(),
            arguments_redacted: trace::redacted_json_string(&observable_tool_arguments(
                name, arguments,
            )),
            timestamp,
        }),
        SessionTurnEvent::ToolResult { name, result, .. } => {
            let redacted_result = if let Ok(val) = serde_json::from_str::<serde_json::Value>(result)
            {
                trace::redacted_json_string(&val)
            } else {
                trace::redacted_text(result)
            };
            Some(trace::TraceEvent::ToolResult {
                session_id: session_id.to_string(),
                role: role.to_string(),
                tool_name: name.clone(),
                result_summary: redacted_result.chars().take(200).collect(),
                timestamp,
            })
        }
        SessionTurnEvent::LoopWarning { count, tool_name } => Some(trace::TraceEvent::Warning {
            session_id: session_id.to_string(),
            role: role.to_string(),
            message: format!("Loop warning: {} repeated {} times", tool_name, count),
            timestamp,
        }),
        SessionTurnEvent::LoopStopped { count, message, .. } => Some(trace::TraceEvent::Stopped {
            session_id: session_id.to_string(),
            role: role.to_string(),
            reason: message.clone(),
            count: *count,
            timestamp,
        }),
        SessionTurnEvent::ModelVariantWarning { message, .. } => Some(trace::TraceEvent::Warning {
            session_id: session_id.to_string(),
            role: role.to_string(),
            message: message.clone(),
            timestamp,
        }),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VerificationJobRequest {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub workspace: ActiveWorkspaceContext,
    pub response_to_verify: String,
    pub user_prompt: String,
    pub session_id: Option<String>,
}

pub fn response_requires_verification(response: &str) -> bool {
    response.contains("```")
        || response.contains("write_file")
        || response.contains("write_harness_file")
        || response.len() > 2000
}

fn turn_requires_verification(response: &str, source_edit_succeeded: bool) -> bool {
    source_edit_succeeded || response_requires_verification(response)
}

#[allow(clippy::too_many_arguments)]
pub fn verification_job_request(
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: Option<&ActiveWorkspaceContext>,
    response_to_verify: &str,
    user_prompt: &str,
    session_id: Option<&str>,
    source_edit_succeeded: bool,
) -> Option<VerificationJobRequest> {
    if !turn_requires_verification(response_to_verify, source_edit_succeeded) {
        return None;
    }

    Some(VerificationJobRequest {
        provider: provider.to_string(),
        model: model.to_string(),
        api_key: api_key.to_string(),
        workspace: workspace.cloned()?,
        response_to_verify: response_to_verify.to_string(),
        user_prompt: user_prompt.to_string(),
        session_id: session_id.map(str::to_string),
    })
}

fn observable_tool_arguments(name: &str, arguments: &serde_json::Value) -> serde_json::Value {
    if name != "source_edit" {
        return arguments.clone();
    }

    let mut redacted = arguments.clone();
    if let serde_json::Value::Object(map) = &mut redacted {
        if map.contains_key("old_text") {
            map.insert(
                "old_text".to_string(),
                serde_json::Value::String("[REDACTED]".to_string()),
            );
        }
        if map.contains_key("new_text") {
            map.insert(
                "new_text".to_string(),
                serde_json::Value::String("[REDACTED]".to_string()),
            );
        }
    }
    redacted
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationNoteAction {
    Create { note_type: String, content: String },
    Resolve { note_id: String },
}

pub fn verification_note_actions(
    result: &VerificationResult,
    unresolved_notes: &[SessionNote],
) -> Vec<VerificationNoteAction> {
    match result.status {
        VerificationStatus::Concerns => {
            let unresolved_concerns: std::collections::HashSet<&str> = unresolved_notes
                .iter()
                .filter(|n| n.note_type == "verification_concern")
                .map(|n| n.content.as_str())
                .collect();
            let mut actions = Vec::new();
            for concern in &result.concerns {
                if !unresolved_concerns.contains(concern.as_str()) {
                    actions.push(VerificationNoteAction::Create {
                        note_type: "verification_concern".to_string(),
                        content: concern.clone(),
                    });
                }
            }
            for note in unresolved_notes {
                if note.note_type == "verification_concern"
                    && !result.concerns.iter().any(|c| c == &note.content)
                {
                    actions.push(VerificationNoteAction::Resolve {
                        note_id: note.id.clone(),
                    });
                }
            }
            actions
        }
        VerificationStatus::Fail => {
            let has_fail = unresolved_notes
                .iter()
                .any(|n| n.note_type == "verification_fail");
            let mut actions = Vec::new();
            if !has_fail {
                actions.push(VerificationNoteAction::Create {
                    note_type: "verification_fail".to_string(),
                    content: result.summary.clone(),
                });
            }
            for note in unresolved_notes {
                if note.note_type == "verification_concern" {
                    actions.push(VerificationNoteAction::Resolve {
                        note_id: note.id.clone(),
                    });
                }
            }
            actions
        }
        VerificationStatus::Pass => unresolved_notes
            .iter()
            .filter(|note| note.note_type.starts_with("verification_"))
            .map(|note| VerificationNoteAction::Resolve {
                note_id: note.id.clone(),
            })
            .collect(),
        VerificationStatus::Unavailable => Vec::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWorkspaceSelection {
    pub id: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveWorkspaceProbe {
    NoWorkspace,
    Invalid {
        selection: ActiveWorkspaceSelection,
        reason: String,
    },
    CorpusAvailable {
        selection: ActiveWorkspaceSelection,
    },
    CorpusUnavailable {
        selection: ActiveWorkspaceSelection,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedActiveWorkspaceContext {
    pub workspace_id: Option<String>,
    pub workspace_path: Option<PathBuf>,
    pub tool_context: Option<ActiveWorkspaceContext>,
    pub corpus_failure_reason: Option<String>,
}

pub fn resolve_active_workspace_context(
    probe: ActiveWorkspaceProbe,
) -> ResolvedActiveWorkspaceContext {
    match probe {
        ActiveWorkspaceProbe::NoWorkspace => ResolvedActiveWorkspaceContext {
            workspace_id: None,
            workspace_path: None,
            tool_context: None,
            corpus_failure_reason: None,
        },
        ActiveWorkspaceProbe::Invalid { selection, reason } => ResolvedActiveWorkspaceContext {
            workspace_id: Some(selection.id),
            workspace_path: Some(selection.path),
            tool_context: None,
            corpus_failure_reason: Some(reason),
        },
        ActiveWorkspaceProbe::CorpusAvailable { selection } => ResolvedActiveWorkspaceContext {
            workspace_id: Some(selection.id),
            workspace_path: Some(selection.path.clone()),
            tool_context: Some(ActiveWorkspaceContext {
                workspace_path: selection.path,
                corpus_available: true,
                session_mode: SessionMode::Build,
            }),
            corpus_failure_reason: None,
        },
        ActiveWorkspaceProbe::CorpusUnavailable { selection, reason } => {
            ResolvedActiveWorkspaceContext {
                workspace_id: Some(selection.id),
                workspace_path: Some(selection.path.clone()),
                tool_context: Some(ActiveWorkspaceContext {
                    workspace_path: selection.path,
                    corpus_available: false,
                    session_mode: SessionMode::Build,
                }),
                corpus_failure_reason: Some(reason),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::completion::message::{
        AssistantContent, Text, ToolCall, ToolFunction, ToolResult, ToolResultContent, UserContent,
    };
    use rig::one_or_many::OneOrMany;
    use std::{future, sync::Mutex};

    fn skill(name: &str, description: &str, body: &str) -> Skill {
        Skill {
            name: name.to_string(),
            description: description.to_string(),
            source: skills::SkillSource::Workspace,
            body: body.to_string(),
            argument_hint: None,
            user_invocable: true,
            disable_model_invocation: false,
            allowed_tools: vec![],
            timeout_seconds: None,
            license: None,
            scripts: vec![],
        }
    }

    fn note(id: &str, note_type: &str, content: &str) -> SessionNote {
        SessionNote {
            id: id.to_string(),
            session_id: "session-1".to_string(),
            note_type: note_type.to_string(),
            content: content.to_string(),
            source_message_id: None,
            resolved: false,
            created_at: String::new(),
        }
    }

    fn user_message(text: &str) -> Message {
        Message::User {
            content: OneOrMany::one(UserContent::Text(Text {
                text: text.to_string(),
                additional_params: Some(serde_json::json!({})),
            })),
        }
    }

    fn assistant_message(text: &str) -> Message {
        Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::Text(Text {
                text: text.to_string(),
                additional_params: Some(serde_json::json!({})),
            })),
        }
    }

    /// Build an assistant message with interleaved text + tool-call content parts
    /// in the given order. Each `(name, id)` tuple becomes a tool call.
    fn assistant_message_with_blocks(parts: &[AssistantPart]) -> Message {
        let mut content: Vec<AssistantContent> = Vec::new();
        for part in parts {
            match part {
                AssistantPart::Text(t) => {
                    content.push(AssistantContent::Text(Text {
                        text: t.to_string(),
                        additional_params: Some(serde_json::json!({})),
                    }));
                }
                AssistantPart::ToolCall {
                    id,
                    name,
                    arguments,
                } => {
                    content.push(AssistantContent::ToolCall(ToolCall {
                        id: id.to_string(),
                        call_id: None,
                        function: ToolFunction {
                            name: name.to_string(),
                            arguments: arguments.clone(),
                        },
                        signature: None,
                        additional_params: None,
                    }));
                }
            }
        }
        Message::Assistant {
            id: None,
            content: OneOrMany::many(content).expect("at least one content part"),
        }
    }

    /// Build a user message carrying a tool result for the given tool-call id.
    fn tool_result_message(id: &str, text: &str) -> Message {
        Message::User {
            content: OneOrMany::one(UserContent::ToolResult(ToolResult {
                id: id.to_string(),
                call_id: None,
                content: OneOrMany::one(ToolResultContent::Text(Text {
                    text: text.to_string(),
                    additional_params: Some(serde_json::json!({})),
                })),
            })),
        }
    }

    enum AssistantPart<'a> {
        Text(&'a str),
        ToolCall {
            id: &'a str,
            name: &'a str,
            arguments: serde_json::Value,
        },
    }

    #[derive(Debug)]
    struct PersistedTurn {
        session_id: String,
        display_transcript: String,
        model_history: Option<String>,
    }

    #[derive(Debug)]
    struct CapturedStreamRequest {
        provider: String,
        prompt: String,
        model: String,
        variant: Option<String>,
        api_key: String,
        delegate_provider: String,
        delegate_model: String,
        delegate_api_key: String,
        workspace: Option<ActiveWorkspaceContext>,
        chat_history: Vec<Message>,
        matched_skills_section: Option<String>,
        invoked_skill_section: Option<String>,
        skill_script_available: bool,
    }

    struct FakeSessionTurnAdapters {
        active_workspace: Option<ActiveWorkspaceSelection>,
        validate_result: Mutex<Result<(), String>>,
        ensure_result: Mutex<Result<Option<usize>, String>>,
        corpus_failure_emissions: Mutex<usize>,
        api_key: String,
        validated_bindings: Mutex<Vec<(String, Option<String>)>>,
        session_mode: String,
        unresolved_notes: Vec<SessionNote>,
        failure_snapshot: Mutex<Option<SessionFailureSnapshot>>,
        persisted_turns: Mutex<Vec<PersistedTurn>>,
        statuses: Mutex<Vec<(String, String)>>,
        chat_history: Vec<Message>,
        stored_histories: Mutex<Vec<(String, Vec<Message>)>>,
        skills: Vec<Skill>,
        stream_result: Mutex<Option<Result<StreamCompletionResult, LlmError>>>,
        stream_requests: Mutex<Vec<CapturedStreamRequest>>,
        model_updates: Mutex<Vec<(String, String, String, Option<String>)>>,
        stream_events: Mutex<Vec<(String, String, SessionTurnEvent)>>,
        done_traces: Mutex<Vec<(String, String, usize, usize, usize, usize)>>,
        error_traces: Mutex<Vec<(String, String, String)>>,
        done_responses: Mutex<Vec<String>>,
        emitted_errors: Mutex<Vec<String>>,
        verifications: Mutex<Vec<VerificationJobRequest>>,
    }

    impl FakeSessionTurnAdapters {
        fn with_stream_result(result: Result<StreamCompletionResult, LlmError>) -> Self {
            Self {
                active_workspace: Some(ActiveWorkspaceSelection {
                    id: "workspace-1".to_string(),
                    path: PathBuf::from("/tmp/workspace"),
                }),
                validate_result: Mutex::new(Ok(())),
                ensure_result: Mutex::new(Ok(Some(3))),
                corpus_failure_emissions: Mutex::new(0),
                api_key: "api-key".to_string(),
                validated_bindings: Mutex::new(Vec::new()),
                session_mode: SESSION_MODE_BUILD.to_string(),
                unresolved_notes: Vec::new(),
                failure_snapshot: Mutex::new(None),
                persisted_turns: Mutex::new(Vec::new()),
                statuses: Mutex::new(Vec::new()),
                chat_history: Vec::new(),
                stored_histories: Mutex::new(Vec::new()),
                skills: Vec::new(),
                stream_result: Mutex::new(Some(result)),
                stream_requests: Mutex::new(Vec::new()),
                model_updates: Mutex::new(Vec::new()),
                stream_events: Mutex::new(Vec::new()),
                done_traces: Mutex::new(Vec::new()),
                error_traces: Mutex::new(Vec::new()),
                done_responses: Mutex::new(Vec::new()),
                emitted_errors: Mutex::new(Vec::new()),
                verifications: Mutex::new(Vec::new()),
            }
        }

        fn deps(&self) -> StreamingTurnDependencies<'_> {
            StreamingTurnDependencies {
                workspace: self,
                credentials: self,
                sessions: self,
                conversation: self,
                skills: self,
                llm: self,
                events: self,
                verification: self,
            }
        }
    }

    impl SessionTurnWorkspace for FakeSessionTurnAdapters {
        fn active_workspace_selection(&self) -> Option<ActiveWorkspaceSelection> {
            self.active_workspace.clone()
        }

        fn validate_workspace_path(&self, _workspace_path: &Path) -> Result<(), String> {
            self.validate_result.lock().unwrap().clone()
        }

        fn ensure_workspace_corpus<'a>(
            &'a self,
            _workspace_path: &'a Path,
        ) -> SessionTurnFuture<'a, Result<Option<usize>, String>> {
            Box::pin(future::ready(self.ensure_result.lock().unwrap().clone()))
        }

        fn emit_corpus_auto_build_failure(&self) {
            *self.corpus_failure_emissions.lock().unwrap() += 1;
        }
    }

    impl SessionTurnCredentials for FakeSessionTurnAdapters {
        fn api_key(&self, _provider: &str) -> Result<String, LlmError> {
            Ok(self.api_key.clone())
        }
    }

    impl SessionTurnSessions for FakeSessionTurnAdapters {
        fn validate_workspace_binding(
            &self,
            session_id: &str,
            active_workspace_id: Option<&str>,
        ) -> Result<(), String> {
            self.validated_bindings.lock().unwrap().push((
                session_id.to_string(),
                active_workspace_id.map(str::to_string),
            ));
            Ok(())
        }

        fn session_mode(&self, _session_id: &str) -> Result<String, String> {
            Ok(self.session_mode.clone())
        }

        fn unresolved_notes(&self, _session_id: &str) -> Vec<SessionNote> {
            self.unresolved_notes.clone()
        }

        fn failure_snapshot(&self, _session_id: &str) -> Option<SessionFailureSnapshot> {
            self.failure_snapshot.lock().unwrap().clone()
        }

        fn persist_turn(
            &self,
            session_id: &str,
            display_transcript: &str,
            model_history: Option<&str>,
        ) -> Result<(), String> {
            self.persisted_turns.lock().unwrap().push(PersistedTurn {
                session_id: session_id.to_string(),
                display_transcript: display_transcript.to_string(),
                model_history: model_history.map(str::to_string),
            });
            Ok(())
        }

        fn update_model_selection(
            &self,
            session_id: &str,
            provider: &str,
            model: &str,
            variant: Option<&str>,
        ) -> Result<(), String> {
            self.model_updates.lock().unwrap().push((
                session_id.to_string(),
                provider.to_string(),
                model.to_string(),
                variant.map(str::to_string),
            ));
            Ok(())
        }

        fn update_status(&self, session_id: &str, status: &str) -> Result<(), String> {
            self.statuses
                .lock()
                .unwrap()
                .push((session_id.to_string(), status.to_string()));
            Ok(())
        }
    }

    impl SessionTurnConversation for FakeSessionTurnAdapters {
        fn chat_history(&self, _session_id: Option<&str>) -> Vec<Message> {
            self.chat_history.clone()
        }

        fn store_history(&self, session_id: &str, history: Vec<Message>) {
            self.stored_histories
                .lock()
                .unwrap()
                .push((session_id.to_string(), history));
        }
    }

    impl SessionTurnSkills for FakeSessionTurnAdapters {
        fn load_skills(&self, _workspace_path: Option<&Path>) -> Vec<Skill> {
            self.skills.clone()
        }
    }

    impl SessionTurnLlm for FakeSessionTurnAdapters {
        fn stream_completion<'a>(
            &'a self,
            request: SessionTurnStreamRequest<'a>,
            mut on_event: Box<dyn FnMut(SessionTurnEvent) + Send + 'a>,
        ) -> SessionTurnFuture<'a, Result<StreamCompletionResult, LlmError>> {
            self.stream_requests
                .lock()
                .unwrap()
                .push(CapturedStreamRequest {
                    provider: request.provider.to_string(),
                    prompt: request.prompt.to_string(),
                    model: request.model.to_string(),
                    variant: request.variant.map(str::to_string),
                    api_key: request.api_key.to_string(),
                    delegate_provider: request.delegate_provider.to_string(),
                    delegate_model: request.delegate_model.to_string(),
                    delegate_api_key: request.delegate_api_key.to_string(),
                    workspace: request.workspace.clone(),
                    chat_history: request.chat_history.clone(),
                    matched_skills_section: request.matched_skills_section.clone(),
                    invoked_skill_section: request.invoked_skill_section.clone(),
                    skill_script_available: request.skill_script_tool.is_some(),
                });
            on_event(SessionTurnEvent::TextToken("hello".to_string()));

            let result = self
                .stream_result
                .lock()
                .unwrap()
                .take()
                .expect("stream result should be configured");
            Box::pin(future::ready(result))
        }
    }

    impl SessionTurnEvents for FakeSessionTurnAdapters {
        fn emit_stream_event(&self, session_id: &str, role: &str, event: &SessionTurnEvent) {
            self.stream_events.lock().unwrap().push((
                session_id.to_string(),
                role.to_string(),
                event.clone(),
            ));
        }

        fn trace_done(
            &self,
            session_id: &str,
            role: &str,
            response_length: usize,
            prompt_tokens: usize,
            response_tokens: usize,
            tool_calls: usize,
        ) {
            self.done_traces.lock().unwrap().push((
                session_id.to_string(),
                role.to_string(),
                response_length,
                prompt_tokens,
                response_tokens,
                tool_calls,
            ));
        }

        fn trace_error(&self, session_id: &str, role: &str, error: &LlmError) {
            self.error_traces.lock().unwrap().push((
                session_id.to_string(),
                role.to_string(),
                error.to_dto().code,
            ));
        }

        fn emit_done(
            &self,
            response: &str,
            _prompt_tokens: usize,
            _response_tokens: usize,
            _tool_calls: usize,
        ) {
            self.done_responses
                .lock()
                .unwrap()
                .push(response.to_string());
        }

        fn emit_error(&self, error: &LlmError) {
            self.emitted_errors
                .lock()
                .unwrap()
                .push(error.to_dto().code);
        }
    }

    impl SessionTurnVerification for FakeSessionTurnAdapters {
        fn schedule_verification(&self, job: VerificationJobRequest) {
            self.verifications.lock().unwrap().push(job);
        }
    }

    #[test]
    fn invoked_skill_suppresses_automatic_matching_and_uses_args() {
        let skills = vec![
            skill(
                "tdd",
                "test driven development workflow",
                "Use red-green-refactor.",
            ),
            skill("diagnose", "debug broken behavior", "Reproduce first."),
        ];

        let prepared = prepare_prompt(
            "debug this with tests",
            &skills,
            Some(&InvokedSkillRequest {
                name: "tdd".to_string(),
                args: Some("write regression tests".to_string()),
            }),
            &[],
        );

        assert_eq!(prepared.effective_prompt, "write regression tests");
        assert!(prepared.matched_skills_section.is_none());
        assert!(prepared
            .invoked_skill_section
            .as_deref()
            .unwrap()
            .contains("Use red-green-refactor."));
    }

    #[test]
    fn automatic_skill_match_runs_when_no_skill_is_invoked() {
        let skills = vec![skill(
            "diagnose",
            "debug broken behavior and failing tests",
            "Reproduce first.",
        )];

        let prepared = prepare_prompt("debug the failing stream", &skills, None, &[]);

        assert!(prepared.invoked_skill_section.is_none());
        assert!(prepared
            .matched_skills_section
            .as_deref()
            .unwrap()
            .contains("diagnose"));
    }

    #[test]
    fn unknown_invoked_skill_preserves_current_no_match_fallback() {
        let skills = vec![skill(
            "diagnose",
            "debug broken behavior and failing tests",
            "Reproduce first.",
        )];

        let prepared = prepare_prompt(
            "debug this",
            &skills,
            Some(&InvokedSkillRequest {
                name: "missing".to_string(),
                args: Some("ignored args".to_string()),
            }),
            &[],
        );

        assert_eq!(prepared.effective_prompt, "debug this");
        assert_eq!(prepared.unknown_invoked_skill.as_deref(), Some("missing"));
        assert!(prepared.matched_skills_section.is_none());
        assert!(prepared.invoked_skill_section.is_none());
    }

    #[test]
    fn unresolved_notes_append_to_effective_prompt() {
        let prepared = prepare_prompt(
            "fix it",
            &[],
            None,
            &[note(
                "note-1",
                "verification_concern",
                "Check the edge case",
            )],
        );

        assert!(prepared.effective_prompt.contains("fix it"));
        assert!(prepared
            .effective_prompt
            .contains("## Previous Verification Concerns"));
        assert!(prepared
            .effective_prompt
            .contains("- [verification_concern] Check the edge case"));
    }

    #[test]
    fn successful_turn_with_model_history_persists_transcript_and_history() {
        let history = vec![user_message("hello"), assistant_message("hi")];

        let decision = successful_turn_persistence(Some(&history)).unwrap();

        let display: serde_json::Value =
            serde_json::from_str(&decision.display_transcript).unwrap();
        assert_eq!(
            display,
            json!([
                { "role": "user", "content": "hello" },
                {
                    "role": "assistant",
                    "content": "hi",
                    "blocks": [
                        { "kind": "text", "id": "text-0", "text": "hi" }
                    ]
                }
            ])
        );
        let restored: Vec<Message> =
            serde_json::from_str(decision.model_history.as_ref().unwrap()).unwrap();
        assert_eq!(restored, history);
    }

    #[test]
    fn successful_turn_without_model_history_still_does_not_persist() {
        assert!(successful_turn_persistence(None).is_none());
    }

    #[test]
    fn provider_failure_appends_error_without_replacing_model_history() {
        let decision = failure_turn_persistence(
            r#"[{"role":"user","content":"hi"}]"#,
            Some(r#"[{"provider":"history"}]"#),
            &LlmError::ProviderError("network down".to_string()),
        );

        let display: serde_json::Value =
            serde_json::from_str(&decision.display_transcript).unwrap();
        assert_eq!(display[0], json!({"role":"user","content":"hi"}));
        assert_eq!(display[1]["role"], "assistant");
        assert_eq!(display[1]["error"], true);
        assert_eq!(display[1]["controlled_stop"], false);
        assert_eq!(
            decision.model_history.as_deref(),
            Some(r#"[{"provider":"history"}]"#)
        );
    }

    #[test]
    fn controlled_stop_appends_stop_without_replacing_model_history() {
        let decision = failure_turn_persistence(
            "[]",
            Some(r#"[{"provider":"history"}]"#),
            &LlmError::ControlledStop("Agent stopped".to_string()),
        );

        let display: serde_json::Value =
            serde_json::from_str(&decision.display_transcript).unwrap();
        assert_eq!(display[0]["content"], "Agent stopped");
        assert_eq!(display[0]["error"], false);
        assert_eq!(display[0]["controlled_stop"], true);
        assert_eq!(
            decision.model_history.as_deref(),
            Some(r#"[{"provider":"history"}]"#)
        );
    }

    #[test]
    fn build_display_transcript_emits_ordered_blocks_with_attached_tool_results() {
        let history = vec![
            user_message("please read foo"),
            assistant_message_with_blocks(&[
                AssistantPart::Text("Let me check."),
                AssistantPart::ToolCall {
                    id: "call-1",
                    name: "read_file",
                    arguments: json!({ "path": "src/lib.rs" }),
                },
                AssistantPart::Text("Now I'll edit it."),
                AssistantPart::ToolCall {
                    id: "call-2",
                    name: "source_edit",
                    arguments: json!({ "path": "src/lib.rs", "old_text": "x", "new_text": "y" }),
                },
            ]),
            tool_result_message("call-1", "file contents here"),
            tool_result_message("call-2", "edited"),
        ];

        let display = build_display_transcript(&history);

        assert_eq!(display.len(), 2);
        assert_eq!(display[0]["role"], "user");
        assert_eq!(display[0]["content"], "please read foo");

        let assistant = &display[1];
        assert_eq!(assistant["role"], "assistant");
        // content keeps joined text for back-compat.
        assert_eq!(assistant["content"], "Let me check.Now I'll edit it.");

        let blocks = assistant["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 4);

        // text / tool / text / tool in native order
        assert_eq!(blocks[0]["kind"], "text");
        assert_eq!(blocks[0]["text"], "Let me check.");
        assert_eq!(blocks[1]["kind"], "tool");
        assert_eq!(blocks[1]["id"], "call-1");
        assert_eq!(blocks[1]["name"], "read_file");
        assert_eq!(blocks[1]["result"], "file contents here");
        assert_eq!(blocks[1]["status"], "completed");
        assert_eq!(blocks[2]["kind"], "text");
        assert_eq!(blocks[2]["text"], "Now I'll edit it.");
        assert_eq!(blocks[3]["kind"], "tool");
        assert_eq!(blocks[3]["id"], "call-2");
        assert_eq!(blocks[3]["name"], "source_edit");
        // source_edit arguments must be redacted.
        assert_eq!(blocks[3]["arguments"]["old_text"], "[REDACTED]");
        assert_eq!(blocks[3]["arguments"]["new_text"], "[REDACTED]");
        assert_eq!(blocks[3]["result"], "edited");
    }

    #[test]
    fn build_display_transcript_emits_unmatched_tool_result_as_own_block() {
        let history = vec![
            user_message("hi"),
            assistant_message_with_blocks(&[AssistantPart::Text("hello")]),
            // tool result with no matching assistant tool call in the prior turn
            tool_result_message("orphan-1", "orphan output"),
        ];

        let display = build_display_transcript(&history);
        // user, assistant(text), assistant(tool orphan)
        assert_eq!(display.len(), 3);
        assert_eq!(display[2]["role"], "assistant");
        let blocks = display[2]["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["kind"], "tool");
        assert_eq!(blocks[0]["id"], "orphan-1");
        assert_eq!(blocks[0]["result"], "orphan output");
    }

    #[test]
    fn ui_event_contract_matches_existing_tauri_names_and_payloads() {
        let token = ui_event_payload(&SessionTurnEvent::TextToken("hello".to_string()));
        assert_eq!(token.name, "llm-token");
        assert_eq!(token.payload, json!("hello"));

        let tool_event = SessionTurnEvent::ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: json!({ "path": "src/lib.rs" }),
        };
        let payload = ui_event_payload(&tool_event);

        assert_eq!(payload.name, "llm-tool-call");
        assert_eq!(
            payload.payload,
            json!({ "id": "call-1", "name": "read_file", "arguments": { "path": "src/lib.rs" } })
        );

        let edit_payload = ui_event_payload(&SessionTurnEvent::ToolCall {
            id: "call-2".to_string(),
            name: "source_edit".to_string(),
            arguments: json!({
                "path": "src/lib.rs",
                "old_text": "secret old snippet",
                "new_text": "secret new snippet"
            }),
        });
        assert_eq!(edit_payload.name, "llm-tool-call");
        assert_eq!(
            edit_payload.payload,
            json!({
                "id": "call-2",
                "name": "source_edit",
                "arguments": {
                    "path": "src/lib.rs",
                    "old_text": "[REDACTED]",
                    "new_text": "[REDACTED]"
                }
            })
        );

        let result = ui_event_payload(&SessionTurnEvent::ToolResult {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            result: "contents".to_string(),
        });
        assert_eq!(result.name, "llm-tool-result");
        assert_eq!(
            result.payload,
            json!({ "id": "call-1", "name": "read_file", "result": "contents" })
        );

        let warning = ui_event_payload(&SessionTurnEvent::LoopWarning {
            count: 3,
            tool_name: "read_file".to_string(),
        });
        assert_eq!(warning.name, "llm-loop-warning");
        assert_eq!(
            warning.payload,
            json!({ "count": 3, "toolName": "read_file" })
        );

        let stopped = ui_event_payload(&SessionTurnEvent::LoopStopped {
            count: 5,
            tool_name: "read_file".to_string(),
            message: "Agent stopped".to_string(),
        });
        assert_eq!(stopped.name, "llm-loop-stopped");
        assert_eq!(
            stopped.payload,
            json!({ "count": 5, "toolName": "read_file", "message": "Agent stopped" })
        );
    }

    #[test]
    fn trace_events_are_derived_without_a_tauri_app_handle() {
        let trace = trace_event_for_session_turn_event(
            &SessionTurnEvent::ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: json!({ "path": "src/lib.rs", "api_key": "sk-test" }),
            },
            "session-1",
            "main",
            123,
        )
        .unwrap();

        let value = serde_json::to_value(trace).unwrap();
        assert_eq!(value["type"], "tool_call");
        assert_eq!(value["session_id"], "session-1");
        assert_eq!(value["tool_name"], "read_file");
        let arguments: serde_json::Value =
            serde_json::from_str(value["arguments_redacted"].as_str().unwrap()).unwrap();
        assert_eq!(arguments["path"], "src/lib.rs");
        assert_eq!(arguments["api_key"], "[REDACTED]");
        assert!(!value["arguments_redacted"]
            .as_str()
            .unwrap()
            .contains("sk-test"));

        let edit_trace = trace_event_for_session_turn_event(
            &SessionTurnEvent::ToolCall {
                id: "call-2".to_string(),
                name: "source_edit".to_string(),
                arguments: json!({
                    "path": "src/lib.rs",
                    "old_text": "raw old",
                    "new_text": "raw new"
                }),
            },
            "session-1",
            "main",
            123,
        )
        .unwrap();
        let value = serde_json::to_value(edit_trace).unwrap();
        let arguments: serde_json::Value =
            serde_json::from_str(value["arguments_redacted"].as_str().unwrap()).unwrap();
        assert_eq!(arguments["path"], "src/lib.rs");
        assert_eq!(arguments["old_text"], "[REDACTED]");
        assert_eq!(arguments["new_text"], "[REDACTED]");
        assert!(!value["arguments_redacted"]
            .as_str()
            .unwrap()
            .contains("raw old"));
        assert!(!value["arguments_redacted"]
            .as_str()
            .unwrap()
            .contains("raw new"));

        let trace = trace_event_for_session_turn_event(
            &SessionTurnEvent::ToolResult {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                result: "x".repeat(250),
            },
            "session-1",
            "main",
            123,
        )
        .unwrap();

        let value = serde_json::to_value(trace).unwrap();
        assert_eq!(value["type"], "tool_result");
        assert_eq!(value["session_id"], "session-1");
        assert_eq!(value["tool_name"], "read_file");
        assert_eq!(value["result_summary"].as_str().unwrap().len(), 200);

        let trace = trace_event_for_session_turn_event(
            &SessionTurnEvent::ToolResult {
                id: "call-2".to_string(),
                name: "read_file".to_string(),
                result: r#"read failed: {"token":"sk-result"}"#.to_string(),
            },
            "session-1",
            "main",
            123,
        )
        .unwrap();

        let value = serde_json::to_value(trace).unwrap();
        assert_eq!(
            value["result_summary"],
            r#"read failed: {"token":"[REDACTED]"}"#
        );
        assert!(!value["result_summary"]
            .as_str()
            .unwrap()
            .contains("sk-result"));
    }

    #[test]
    fn verification_job_requires_high_risk_response_and_workspace() {
        let workspace = ActiveWorkspaceContext {
            workspace_path: PathBuf::from("/tmp/workspace"),
            corpus_available: true,
            session_mode: SessionMode::Build,
        };

        assert!(verification_job_request(
            "openai",
            "gpt-4",
            "key",
            Some(&workspace),
            "plain response",
            "prompt",
            Some("session-1"),
            false,
        )
        .is_none());

        let job = verification_job_request(
            "openai",
            "gpt-4",
            "key",
            Some(&workspace),
            "```rust\nfn main() {}\n```",
            "prompt",
            Some("session-1"),
            false,
        )
        .unwrap();

        assert_eq!(job.workspace, workspace);
        assert_eq!(job.session_id.as_deref(), Some("session-1"));

        let source_edit_job = verification_job_request(
            "openai",
            "gpt-4",
            "key",
            Some(&workspace),
            "done",
            "prompt",
            Some("session-1"),
            true,
        )
        .unwrap();
        assert_eq!(source_edit_job.response_to_verify, "done");
    }

    #[test]
    fn verification_note_actions_preserve_existing_outcomes() {
        let concerns = VerificationResult {
            status: VerificationStatus::Concerns,
            concerns: vec!["check one".to_string(), "check two".to_string()],
            summary: "concerns".to_string(),
        };
        assert_eq!(
            verification_note_actions(&concerns, &[]),
            vec![
                VerificationNoteAction::Create {
                    note_type: "verification_concern".to_string(),
                    content: "check one".to_string(),
                },
                VerificationNoteAction::Create {
                    note_type: "verification_concern".to_string(),
                    content: "check two".to_string(),
                },
            ]
        );

        let pass = VerificationResult {
            status: VerificationStatus::Pass,
            concerns: vec![],
            summary: "ok".to_string(),
        };
        assert_eq!(
            verification_note_actions(
                &pass,
                &[
                    note("a", "verification_concern", "old"),
                    note("b", "human_note", "keep"),
                ],
            ),
            vec![VerificationNoteAction::Resolve {
                note_id: "a".to_string()
            }]
        );

        let fail = VerificationResult {
            status: VerificationStatus::Fail,
            concerns: vec![],
            summary: "unsafe".to_string(),
        };
        assert_eq!(
            verification_note_actions(&fail, &[]),
            vec![VerificationNoteAction::Create {
                note_type: "verification_fail".to_string(),
                content: "unsafe".to_string()
            }]
        );

        let unavailable = VerificationResult {
            status: VerificationStatus::Unavailable,
            concerns: vec![],
            summary: "timeout".to_string(),
        };
        assert!(verification_note_actions(&unavailable, &[]).is_empty());
    }

    #[test]
    fn active_workspace_resolution_covers_none_invalid_available_and_unavailable() {
        let none = resolve_active_workspace_context(ActiveWorkspaceProbe::NoWorkspace);
        assert!(none.workspace_id.is_none());
        assert!(none.tool_context.is_none());

        let selection = ActiveWorkspaceSelection {
            id: "ws1".to_string(),
            path: PathBuf::from("/tmp/workspace"),
        };
        let invalid = resolve_active_workspace_context(ActiveWorkspaceProbe::Invalid {
            selection: selection.clone(),
            reason: "missing".to_string(),
        });
        assert_eq!(invalid.workspace_id.as_deref(), Some("ws1"));
        assert!(invalid.tool_context.is_none());
        assert_eq!(invalid.corpus_failure_reason.as_deref(), Some("missing"));

        let available = resolve_active_workspace_context(ActiveWorkspaceProbe::CorpusAvailable {
            selection: selection.clone(),
        });
        assert!(available.tool_context.unwrap().corpus_available);

        let unavailable =
            resolve_active_workspace_context(ActiveWorkspaceProbe::CorpusUnavailable {
                selection,
                reason: "build failed".to_string(),
            });
        let tool_context = unavailable.tool_context.unwrap();
        assert!(!tool_context.corpus_available);
        assert_eq!(
            unavailable.corpus_failure_reason.as_deref(),
            Some("build failed")
        );
    }

    #[tokio::test]
    async fn streaming_turn_success_uses_adapters_for_persistence_events_and_verification() {
        let history = vec![
            user_message("write code"),
            assistant_message("```rust\nfn main() {}\n```"),
        ];
        let adapters = FakeSessionTurnAdapters::with_stream_result(Ok(StreamCompletionResult {
            full_response: "```rust\nfn main() {}\n```".to_string(),
            history: Some(history.clone()),
            source_edit_succeeded: false,
            prompt_tokens: 18,
            response_tokens: 6,
            tool_calls: 1,
        }));

        let result = run_streaming_turn(
            adapters.deps(),
            StreamingTurnRequest {
                provider: "openai".to_string(),
                prompt: "write code".to_string(),
                model: "gpt-test".to_string(),
                variant: Some("reasoning-high".to_string()),
                session_id: Some("session-1".to_string()),
                invoked_skill: None,
                delegate_provider: "openai".to_string(),
                delegate_model: "gpt-4o-mini".to_string(),
                delegate_api_key: "api-key".to_string(),
            },
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        assert_eq!(
            *adapters.validated_bindings.lock().unwrap(),
            vec![("session-1".to_string(), Some("workspace-1".to_string()))]
        );
        assert_eq!(
            adapters.stream_requests.lock().unwrap()[0]
                .variant
                .as_deref(),
            Some("reasoning-high")
        );

        let stream_requests = adapters.stream_requests.lock().unwrap();
        assert_eq!(stream_requests.len(), 1);
        let stream_request = &stream_requests[0];
        assert_eq!(stream_request.provider, "openai");
        assert_eq!(stream_request.prompt, "write code");
        assert_eq!(stream_request.model, "gpt-test");
        assert_eq!(stream_request.api_key, "api-key");
        assert_eq!(stream_request.delegate_provider, "openai");
        assert_eq!(stream_request.delegate_model, "gpt-4o-mini");
        assert_eq!(stream_request.delegate_api_key, "api-key");
        assert_eq!(stream_request.chat_history, Vec::<Message>::new());
        assert!(stream_request.matched_skills_section.is_none());
        assert!(stream_request.invoked_skill_section.is_none());
        assert!(!stream_request.skill_script_available);
        assert_eq!(
            stream_request.workspace,
            Some(ActiveWorkspaceContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: true,
                session_mode: SessionMode::Build,
            })
        );
        drop(stream_requests);

        assert_eq!(
            *adapters.stream_events.lock().unwrap(),
            vec![(
                "session-1".to_string(),
                "main".to_string(),
                SessionTurnEvent::TextToken("hello".to_string()),
            )]
        );
        assert_eq!(
            *adapters.done_traces.lock().unwrap(),
            vec![(
                "session-1".to_string(),
                "main".to_string(),
                "```rust\nfn main() {}\n```".len(),
                18,
                6,
                1,
            )]
        );
        assert_eq!(
            *adapters.done_responses.lock().unwrap(),
            vec!["```rust\nfn main() {}\n```".to_string()]
        );

        let stored_histories = adapters.stored_histories.lock().unwrap();
        assert_eq!(stored_histories.len(), 1);
        assert_eq!(stored_histories[0].0, "session-1");
        assert_eq!(stored_histories[0].1, history);
        drop(stored_histories);

        let persisted_turns = adapters.persisted_turns.lock().unwrap();
        assert_eq!(persisted_turns.len(), 1);
        assert_eq!(persisted_turns[0].session_id, "session-1");
        let display: serde_json::Value =
            serde_json::from_str(&persisted_turns[0].display_transcript).unwrap();
        assert_eq!(
            display,
            json!([
                { "role": "user", "content": "write code" },
                {
                    "role": "assistant",
                    "content": "```rust\nfn main() {}\n```",
                    "blocks": [
                        { "kind": "text", "id": "text-0", "text": "```rust\nfn main() {}\n```" }
                    ]
                },
            ])
        );
        let model_history: Vec<Message> =
            serde_json::from_str(persisted_turns[0].model_history.as_ref().unwrap()).unwrap();
        assert_eq!(model_history, history);
        drop(persisted_turns);

        assert_eq!(
            *adapters.statuses.lock().unwrap(),
            vec![("session-1".to_string(), "active".to_string())]
        );

        let verifications = adapters.verifications.lock().unwrap();
        assert_eq!(verifications.len(), 1);
        assert_eq!(verifications[0].provider, "openai");
        assert_eq!(verifications[0].model, "gpt-test");
        assert_eq!(verifications[0].api_key, "api-key");
        assert_eq!(verifications[0].session_id.as_deref(), Some("session-1"));
        assert!(verifications[0].workspace.corpus_available);
    }

    #[tokio::test]
    async fn streaming_turn_persists_resolved_default_when_variant_falls_back() {
        let adapters = FakeSessionTurnAdapters::with_stream_result(Ok(StreamCompletionResult {
            full_response: "done".to_string(),
            history: None,
            source_edit_succeeded: false,
            prompt_tokens: 1,
            response_tokens: 1,
            tool_calls: 0,
        }));

        let result = run_streaming_turn(
            adapters.deps(),
            StreamingTurnRequest {
                provider: "openai".to_string(),
                prompt: "hello".to_string(),
                model: "gpt-5.2".to_string(),
                variant: Some("missing-variant".to_string()),
                session_id: Some("session-1".to_string()),
                invoked_skill: None,
                delegate_provider: "openai".to_string(),
                delegate_model: "gpt-4o-mini".to_string(),
                delegate_api_key: "api-key".to_string(),
            },
        )
        .await;

        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        assert_eq!(
            adapters.stream_requests.lock().unwrap()[0]
                .variant
                .as_deref(),
            Some("missing-variant")
        );
        assert_eq!(
            *adapters.model_updates.lock().unwrap(),
            vec![(
                "session-1".to_string(),
                "openai".to_string(),
                "gpt-5.2".to_string(),
                None,
            )]
        );
    }

    #[tokio::test]
    async fn streaming_turn_passes_read_only_session_mode_to_workspace_tools() {
        let history = vec![user_message("inspect"), assistant_message("Done.")];
        let mut adapters =
            FakeSessionTurnAdapters::with_stream_result(Ok(StreamCompletionResult {
                full_response: "Done.".to_string(),
                history: Some(history),
                source_edit_succeeded: false,
                prompt_tokens: 2,
                response_tokens: 1,
                tool_calls: 0,
            }));
        adapters.session_mode = crate::session_mode::SESSION_MODE_READ_ONLY.to_string();

        let result = run_streaming_turn(
            adapters.deps(),
            StreamingTurnRequest {
                provider: "openai".to_string(),
                prompt: "inspect".to_string(),
                model: "gpt-test".to_string(),
                variant: None,
                session_id: Some("session-1".to_string()),
                invoked_skill: None,
                delegate_provider: "openai".to_string(),
                delegate_model: "gpt-4o-mini".to_string(),
                delegate_api_key: "api-key".to_string(),
            },
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        let stream_requests = adapters.stream_requests.lock().unwrap();
        assert_eq!(
            stream_requests[0]
                .workspace
                .as_ref()
                .map(|workspace| workspace.session_mode.as_str()),
            Some(crate::session_mode::SESSION_MODE_READ_ONLY)
        );
    }

    #[tokio::test]
    async fn successful_source_edit_turn_schedules_verification_for_short_response() {
        let history = vec![user_message("edit it"), assistant_message("Done.")];
        let adapters = FakeSessionTurnAdapters::with_stream_result(Ok(StreamCompletionResult {
            full_response: "Done.".to_string(),
            history: Some(history),
            source_edit_succeeded: true,
            prompt_tokens: 4,
            response_tokens: 1,
            tool_calls: 0,
        }));

        let result = run_streaming_turn(
            adapters.deps(),
            StreamingTurnRequest {
                provider: "openai".to_string(),
                prompt: "edit it".to_string(),
                model: "gpt-test".to_string(),
                variant: None,
                session_id: Some("session-1".to_string()),
                invoked_skill: None,
                delegate_provider: "openai".to_string(),
                delegate_model: "gpt-4o-mini".to_string(),
                delegate_api_key: "api-key".to_string(),
            },
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        let verifications = adapters.verifications.lock().unwrap();
        assert_eq!(verifications.len(), 1);
        assert_eq!(verifications[0].response_to_verify, "Done.");
    }

    #[tokio::test]
    async fn unsuccessful_source_edit_attempt_does_not_schedule_short_response_verification() {
        let history = vec![
            user_message("edit it"),
            assistant_message("Could not edit."),
        ];
        let adapters = FakeSessionTurnAdapters::with_stream_result(Ok(StreamCompletionResult {
            full_response: "Could not edit.".to_string(),
            history: Some(history),
            source_edit_succeeded: false,
            prompt_tokens: 3,
            response_tokens: 3,
            tool_calls: 0,
        }));

        let result = run_streaming_turn(
            adapters.deps(),
            StreamingTurnRequest {
                provider: "openai".to_string(),
                prompt: "edit it".to_string(),
                model: "gpt-test".to_string(),
                variant: None,
                session_id: Some("session-1".to_string()),
                invoked_skill: None,
                delegate_provider: "openai".to_string(),
                delegate_model: "gpt-4o-mini".to_string(),
                delegate_api_key: "api-key".to_string(),
            },
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        assert!(adapters.verifications.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn streaming_turn_failure_persists_controlled_stop_without_replacing_model_history() {
        let adapters = FakeSessionTurnAdapters::with_stream_result(Err(LlmError::ControlledStop(
            "Agent stopped".to_string(),
        )));
        *adapters.failure_snapshot.lock().unwrap() = Some(SessionFailureSnapshot {
            display_transcript: r#"[{"role":"user","content":"hi"}]"#.to_string(),
            model_history: Some(r#"[{"provider":"history"}]"#.to_string()),
        });

        let err = run_streaming_turn(
            adapters.deps(),
            StreamingTurnRequest {
                provider: "openai".to_string(),
                prompt: "hi".to_string(),
                model: "gpt-test".to_string(),
                variant: None,
                session_id: Some("session-1".to_string()),
                invoked_skill: None,
                delegate_provider: "openai".to_string(),
                delegate_model: "gpt-4o-mini".to_string(),
                delegate_api_key: "api-key".to_string(),
            },
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, "CONTROLLED_STOP");
        assert_eq!(
            *adapters.error_traces.lock().unwrap(),
            vec![(
                "session-1".to_string(),
                "main".to_string(),
                "CONTROLLED_STOP".to_string(),
            )]
        );
        assert_eq!(
            *adapters.emitted_errors.lock().unwrap(),
            vec!["CONTROLLED_STOP".to_string()]
        );
        assert!(adapters.done_responses.lock().unwrap().is_empty());
        assert!(adapters.verifications.lock().unwrap().is_empty());

        let persisted_turns = adapters.persisted_turns.lock().unwrap();
        assert_eq!(persisted_turns.len(), 1);
        assert_eq!(persisted_turns[0].session_id, "session-1");
        assert_eq!(
            persisted_turns[0].model_history.as_deref(),
            Some(r#"[{"provider":"history"}]"#)
        );
        let display: serde_json::Value =
            serde_json::from_str(&persisted_turns[0].display_transcript).unwrap();
        assert_eq!(display[0], json!({ "role": "user", "content": "hi" }));
        assert_eq!(display[1]["content"], "Agent stopped");
        assert_eq!(display[1]["error"], false);
        assert_eq!(display[1]["controlled_stop"], true);
    }
}
