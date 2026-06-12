use crate::llm::{self, LlmError, StreamCompletionResult, StreamEvent, WorkspaceToolContext};
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
    pub session_id: Option<String>,
    pub invoked_skill: Option<InvokedSkillRequest>,
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

    fn unresolved_notes(&self, session_id: &str) -> Vec<SessionNote>;

    fn failure_snapshot(&self, session_id: &str) -> Option<SessionFailureSnapshot>;

    fn persist_turn(
        &self,
        session_id: &str,
        display_transcript: &str,
        model_history: Option<&str>,
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
    pub api_key: &'a str,
    pub workspace: Option<WorkspaceToolContext>,
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
    fn trace_done(&self, session_id: &str, role: &str, response_length: usize);
    fn trace_error(&self, session_id: &str, role: &str, error: &LlmError);
    fn emit_done(&self, response: &str);
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
    let workspace_context = workspace_resolution.tool_context.clone();

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

    if let Some(sid) = &request.session_id {
        if let Err(e) = deps
            .sessions
            .validate_workspace_binding(sid, workspace_resolution.workspace_id.as_deref())
        {
            return Err(LlmError::ProviderError(e.to_string()).to_dto());
        }
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
                api_key: &api_key,
                workspace: workspace_context,
                chat_history,
                matched_skills_section: prompt_preparation.matched_skills_section.clone(),
                invoked_skill_section: prompt_preparation.invoked_skill_section.clone(),
                skill_script_tool,
            },
            Box::new(move |event| {
                events.emit_stream_event(&trace_sid, &trace_role, &event);
            }),
        )
        .await;

    match result {
        Ok(stream_result) => {
            deps.events.trace_done(
                request.session_id.as_deref().unwrap_or(""),
                "main",
                stream_result.full_response.len(),
            );
            if let (Some(sid), Some(persistence)) = (
                &request.session_id,
                successful_turn_persistence(stream_result.history.as_deref()),
            ) {
                deps.conversation
                    .store_history(sid, persistence.history.clone());

                if let Err(e) = deps.sessions.persist_turn(
                    sid,
                    &persistence.display_transcript,
                    persistence.model_history.as_deref(),
                ) {
                    tracing::warn!("Failed to persist session {}: {}", sid, e);
                }
                let _ = deps.sessions.update_status(sid, "active");
            }

            let response_for_verify = stream_result.full_response.clone();
            deps.events.emit_done(&response_for_verify);

            if let Some(job) = verification_job_request(
                &request.provider,
                &request.model,
                &api_key,
                workspace_for_verify.as_ref(),
                &response_for_verify,
                &prompt_preparation.effective_prompt,
                request.session_id.as_deref(),
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
        "error": !is_controlled_stop,
        "controlled_stop": is_controlled_stop,
    }));

    FailureTurnPersistence {
        display_transcript: serde_json::to_string(&transcript).unwrap_or_else(|_| "[]".to_string()),
        model_history: existing_model_history.map(str::to_string),
    }
}

pub fn build_display_transcript(messages: &[Message]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .filter_map(|message| match message {
            Message::User { content } => {
                let text_parts: Vec<String> = content
                    .iter()
                    .filter_map(|content| match content {
                        UserContent::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .collect();
                if text_parts.is_empty() {
                    None
                } else {
                    Some(json!({
                        "role": "user",
                        "content": text_parts.join(""),
                    }))
                }
            }
            Message::Assistant { content, .. } => {
                let text_parts: Vec<String> = content
                    .iter()
                    .filter_map(|content| match content {
                        AssistantContent::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .collect();
                if text_parts.is_empty() {
                    None
                } else {
                    Some(json!({
                        "role": "assistant",
                        "content": text_parts.join(""),
                    }))
                }
            }
            _ => None,
        })
        .collect()
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
            payload: json!({ "id": id, "name": name, "arguments": arguments }),
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
            arguments_redacted: serde_json::to_string(arguments).unwrap_or_default(),
            timestamp,
        }),
        SessionTurnEvent::ToolResult { name, result, .. } => Some(trace::TraceEvent::ToolResult {
            session_id: session_id.to_string(),
            role: role.to_string(),
            tool_name: name.clone(),
            result_summary: result.chars().take(200).collect(),
            timestamp,
        }),
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
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VerificationJobRequest {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub workspace: WorkspaceToolContext,
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

pub fn verification_job_request(
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: Option<&WorkspaceToolContext>,
    response_to_verify: &str,
    user_prompt: &str,
    session_id: Option<&str>,
) -> Option<VerificationJobRequest> {
    if !response_requires_verification(response_to_verify) {
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
        VerificationStatus::Concerns => result
            .concerns
            .iter()
            .map(|concern| VerificationNoteAction::Create {
                note_type: "verification_concern".to_string(),
                content: concern.clone(),
            })
            .collect(),
        VerificationStatus::Fail => vec![VerificationNoteAction::Create {
            note_type: "verification_fail".to_string(),
            content: result.summary.clone(),
        }],
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
    pub tool_context: Option<WorkspaceToolContext>,
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
            tool_context: Some(WorkspaceToolContext {
                workspace_path: selection.path,
                corpus_available: true,
            }),
            corpus_failure_reason: None,
        },
        ActiveWorkspaceProbe::CorpusUnavailable { selection, reason } => {
            ResolvedActiveWorkspaceContext {
                workspace_id: Some(selection.id),
                workspace_path: Some(selection.path.clone()),
                tool_context: Some(WorkspaceToolContext {
                    workspace_path: selection.path,
                    corpus_available: false,
                }),
                corpus_failure_reason: Some(reason),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::completion::message::{AssistantContent, Text, UserContent};
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
            })),
        }
    }

    fn assistant_message(text: &str) -> Message {
        Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::Text(Text {
                text: text.to_string(),
            })),
        }
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
        api_key: String,
        workspace: Option<WorkspaceToolContext>,
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
        unresolved_notes: Vec<SessionNote>,
        failure_snapshot: Mutex<Option<SessionFailureSnapshot>>,
        persisted_turns: Mutex<Vec<PersistedTurn>>,
        statuses: Mutex<Vec<(String, String)>>,
        chat_history: Vec<Message>,
        stored_histories: Mutex<Vec<(String, Vec<Message>)>>,
        skills: Vec<Skill>,
        stream_result: Mutex<Option<Result<StreamCompletionResult, LlmError>>>,
        stream_requests: Mutex<Vec<CapturedStreamRequest>>,
        stream_events: Mutex<Vec<(String, String, SessionTurnEvent)>>,
        done_traces: Mutex<Vec<(String, String, usize)>>,
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
                unresolved_notes: Vec::new(),
                failure_snapshot: Mutex::new(None),
                persisted_turns: Mutex::new(Vec::new()),
                statuses: Mutex::new(Vec::new()),
                chat_history: Vec::new(),
                stored_histories: Mutex::new(Vec::new()),
                skills: Vec::new(),
                stream_result: Mutex::new(Some(result)),
                stream_requests: Mutex::new(Vec::new()),
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
                    api_key: request.api_key.to_string(),
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

        fn trace_done(&self, session_id: &str, role: &str, response_length: usize) {
            self.done_traces.lock().unwrap().push((
                session_id.to_string(),
                role.to_string(),
                response_length,
            ));
        }

        fn trace_error(&self, session_id: &str, role: &str, error: &LlmError) {
            self.error_traces.lock().unwrap().push((
                session_id.to_string(),
                role.to_string(),
                error.to_dto().code,
            ));
        }

        fn emit_done(&self, response: &str) {
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
                { "role": "assistant", "content": "hi" }
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
    }

    #[test]
    fn verification_job_requires_high_risk_response_and_workspace() {
        let workspace = WorkspaceToolContext {
            workspace_path: PathBuf::from("/tmp/workspace"),
            corpus_available: true,
        };

        assert!(verification_job_request(
            "openai",
            "gpt-4",
            "key",
            Some(&workspace),
            "plain response",
            "prompt",
            Some("session-1"),
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
        )
        .unwrap();

        assert_eq!(job.workspace, workspace);
        assert_eq!(job.session_id.as_deref(), Some("session-1"));
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
        }));

        let result = run_streaming_turn(
            adapters.deps(),
            StreamingTurnRequest {
                provider: "openai".to_string(),
                prompt: "write code".to_string(),
                model: "gpt-test".to_string(),
                session_id: Some("session-1".to_string()),
                invoked_skill: None,
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

        let stream_requests = adapters.stream_requests.lock().unwrap();
        assert_eq!(stream_requests.len(), 1);
        let stream_request = &stream_requests[0];
        assert_eq!(stream_request.provider, "openai");
        assert_eq!(stream_request.prompt, "write code");
        assert_eq!(stream_request.model, "gpt-test");
        assert_eq!(stream_request.api_key, "api-key");
        assert_eq!(stream_request.chat_history, Vec::<Message>::new());
        assert!(stream_request.matched_skills_section.is_none());
        assert!(stream_request.invoked_skill_section.is_none());
        assert!(!stream_request.skill_script_available);
        assert_eq!(
            stream_request.workspace,
            Some(WorkspaceToolContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: true,
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
                { "role": "assistant", "content": "```rust\nfn main() {}\n```" },
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
                session_id: Some("session-1".to_string()),
                invoked_skill: None,
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
