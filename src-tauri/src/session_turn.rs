use crate::agents::{AgentDefinition, AgentOrchestration};
use crate::harness_profile::{
    resolve_custom_agent_profile, ActiveWorkspaceContext, CustomAgentProfileRequest, HarnessProfile,
};
use crate::llm::{
    self, LlmError, ReasoningPayload, ReasoningPhase, StreamCompletionResult, StreamEvent,
};
use crate::models::ModelRegistry;
use crate::session_store::SessionNote;
use crate::skills::{self, RunSkillScriptTool, Skill};
use crate::subagent::{SubagentKind, SubagentStep};
use crate::trace;
use crate::verification::{VerificationResult, VerificationStatus};
use rig::completion::message::{AssistantContent, Message, ToolResultContent, UserContent};
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

#[derive(Debug, Clone, Deserialize)]
pub struct DelegatedAgentRequest {
    pub name: String,
    pub task: String,
}

pub struct StreamingTurnRequest {
    pub run_id: String,
    pub provider: String,
    pub prompt: String,
    pub model: String,
    pub variant: Option<String>,
    pub session_id: Option<String>,
    pub invoked_skill: Option<InvokedSkillRequest>,
    pub agent_id: Option<String>,
    pub delegated_agent: Option<DelegatedAgentRequest>,
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
    pub agents: &'a dyn SessionTurnAgents,
    pub llm: &'a dyn SessionTurnLlm,
    pub events: &'a dyn SessionTurnEvents,
    pub verification: &'a dyn SessionTurnVerification,
    pub memory: &'a dyn SessionTurnMemory,
}

pub trait SessionTurnWorkspace: Send + Sync {
    fn active_workspace_selection(&self) -> Option<ActiveWorkspaceSelection>;

    fn validate_workspace_path(&self, workspace_path: &Path) -> Result<(), String>;
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

    fn persisted_snapshot(&self, session_id: &str) -> Option<SessionSnapshot>;

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
pub struct SessionSnapshot {
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

pub trait SessionTurnAgents: Send + Sync {
    fn load_agents(&self, workspace_path: Option<&Path>) -> Vec<AgentDefinition>;
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
    pub memory_section: Option<String>,
    pub skill_script_tool: Option<RunSkillScriptTool>,
    pub custom_profile: Option<HarnessProfile>,
    /// Conversation id forwarded to providers that accept a session header
    /// (OpenCode Go) for routing and prompt-cache optimization.
    pub session_id: Option<&'a str>,
    /// Streaming run id for this turn. Approval wrappers tag broker requests
    /// with it so the frontend can route approval events to the session's
    /// own live turn instead of whichever session is focused.
    pub run_id: Option<&'a str>,
}

pub trait SessionTurnLlm: Send + Sync {
    fn stream_completion<'a>(
        &'a self,
        request: SessionTurnStreamRequest<'a>,
        on_event: Box<dyn FnMut(SessionTurnEvent) + Send + 'a>,
    ) -> SessionTurnFuture<'a, Result<StreamCompletionResult, LlmError>>;
}

pub trait SessionTurnEvents: Send + Sync {
    fn emit_stream_event(
        &self,
        session_id: &str,
        role: &str,
        run_id: &str,
        event: &SessionTurnEvent,
    );
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
        run_id: &str,
        response: &str,
        prompt_tokens: usize,
        response_tokens: usize,
        tool_calls: usize,
    );
    fn emit_error(&self, run_id: &str, error: &LlmError);
}

pub trait SessionTurnVerification: Send + Sync {
    fn schedule_verification(&self, job: VerificationJobRequest);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRecallRequest {
    pub prompt: String,
    pub session_id: Option<String>,
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryIngestJobRequest {
    pub display_transcript: String,
    pub session_id: String,
    pub workspace_id: Option<String>,
}

pub trait SessionTurnMemory: Send + Sync {
    fn recall(&self, request: MemoryRecallRequest) -> Result<Option<String>, String>;
    fn schedule_ingest(&self, job: MemoryIngestJobRequest);
}

pub struct NoOpMemory;

impl SessionTurnMemory for NoOpMemory {
    fn recall(&self, _request: MemoryRecallRequest) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn schedule_ingest(&self, _job: MemoryIngestJobRequest) {}
}

// ============================================================================
// Consolidated Deep Turn Orchestrator Seams (ADR-0004, ADR-0005, CONTEXT.md)
// ============================================================================

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnSummary {
    pub full_response: String,
    pub prompt_tokens: usize,
    pub response_tokens: usize,
    pub tool_calls: usize,
}

#[allow(dead_code)]
pub trait TurnPersistenceAdapter: Send + Sync {
    fn activate_draft_if_needed(&self, session_id: &str) -> Result<(), String>;
    fn save_turn_success(
        &self,
        session_id: &str,
        user_prompt: &str,
        assistant_reply: &str,
        model_history: Option<&str>,
    ) -> Result<(), String>;
    fn save_turn_failure(
        &self,
        session_id: &str,
        user_prompt: &str,
        error_message: &str,
    ) -> Result<(), String>;
    fn save_turn_stopped(
        &self,
        session_id: &str,
        user_prompt: &str,
        stopped_reason: &str,
    ) -> Result<(), String>;
}

impl<T: TurnPersistenceAdapter + ?Sized> TurnPersistenceAdapter for std::sync::Arc<T> {
    fn activate_draft_if_needed(&self, session_id: &str) -> Result<(), String> {
        (**self).activate_draft_if_needed(session_id)
    }

    fn save_turn_success(
        &self,
        session_id: &str,
        user_prompt: &str,
        assistant_reply: &str,
        model_history: Option<&str>,
    ) -> Result<(), String> {
        (**self).save_turn_success(session_id, user_prompt, assistant_reply, model_history)
    }

    fn save_turn_failure(
        &self,
        session_id: &str,
        user_prompt: &str,
        error_message: &str,
    ) -> Result<(), String> {
        (**self).save_turn_failure(session_id, user_prompt, error_message)
    }

    fn save_turn_stopped(
        &self,
        session_id: &str,
        user_prompt: &str,
        stopped_reason: &str,
    ) -> Result<(), String> {
        (**self).save_turn_stopped(session_id, user_prompt, stopped_reason)
    }
}

#[allow(dead_code)]
pub trait TurnEventEmitter: Send + Sync {
    fn emit_event(&self, session_id: &str, run_id: &str, event: &SessionTurnEvent);
}

impl<T: TurnEventEmitter + ?Sized> TurnEventEmitter for std::sync::Arc<T> {
    fn emit_event(&self, session_id: &str, run_id: &str, event: &SessionTurnEvent) {
        (**self).emit_event(session_id, run_id, event)
    }
}

#[allow(dead_code)]
pub trait TurnLlmAdapter: Send + Sync {
    fn stream_turn<'a>(
        &'a self,
        prompt: &'a str,
        on_event: Box<dyn FnMut(SessionTurnEvent) + Send + 'a>,
    ) -> SessionTurnFuture<'a, Result<StreamCompletionResult, LlmError>>;
}

#[allow(dead_code)]
pub struct TurnOrchestrator<P, E, L> {
    pub persistence: P,
    pub emitter: E,
    pub llm: L,
}

#[allow(dead_code)]
impl<P: TurnPersistenceAdapter, E: TurnEventEmitter, L: TurnLlmAdapter> TurnOrchestrator<P, E, L> {
    pub fn new(persistence: P, emitter: E, llm: L) -> Self {
        Self {
            persistence,
            emitter,
            llm,
        }
    }

    pub async fn execute_turn(
        &self,
        session_id: &str,
        run_id: &str,
        prompt: &str,
    ) -> Result<TurnSummary, LlmError> {
        let _ = self.persistence.activate_draft_if_needed(session_id);

        let emitter_ref = &self.emitter;
        let result = self
            .llm
            .stream_turn(
                prompt,
                Box::new(move |evt| {
                    emitter_ref.emit_event(session_id, run_id, &evt);
                }),
            )
            .await;

        match result {
            Ok(completion) => {
                let history_json = completion
                    .history
                    .as_ref()
                    .map(|h| serde_json::to_string(h).unwrap_or_default());
                self.persistence
                    .save_turn_success(
                        session_id,
                        prompt,
                        &completion.full_response,
                        history_json.as_deref(),
                    )
                    .map_err(LlmError::ProviderError)?;

                Ok(TurnSummary {
                    full_response: completion.full_response,
                    prompt_tokens: completion.prompt_tokens,
                    response_tokens: completion.response_tokens,
                    tool_calls: completion.tool_calls,
                })
            }
            Err(LlmError::ControlledStop(reason)) => {
                self.persistence
                    .save_turn_stopped(session_id, prompt, &reason)
                    .ok();
                Err(LlmError::ControlledStop(reason))
            }
            Err(err) => {
                let err_msg = err.to_string();
                self.persistence
                    .save_turn_failure(session_id, prompt, &err_msg)
                    .ok();
                Err(err)
            }
        }
    }
}

pub async fn run_streaming_turn(
    deps: StreamingTurnDependencies<'_>,
    request: StreamingTurnRequest,
) -> Result<String, llm::LlmErrorDto> {
    let active_workspace = deps.workspace.active_workspace_selection();
    let workspace_resolution =
        resolve_streaming_active_workspace(deps.workspace, active_workspace).await;
    let workspace_path = workspace_resolution.workspace_path.clone();
    let workspace_context = workspace_resolution.tool_context.clone();

    tracing::debug!(
        "streaming turn workspace path: {}",
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
    let history_before_turn = chat_history.clone();

    let all_skills = deps.skills.load_skills(workspace_path.as_deref());
    let session_agent = match request.agent_id.as_deref() {
        Some(name) => {
            if workspace_path.is_none() {
                return Err(LlmError::ProviderError(
                    "custom agents require an active workspace".to_string(),
                )
                .to_dto());
            }
            let agents = deps.agents.load_agents(workspace_path.as_deref());
            Some(
                agents
                    .into_iter()
                    .find(|agent| agent.name == name)
                    .ok_or_else(|| {
                        LlmError::ProviderError(format!(
                            "agent '{name}' was not found in .agents/agents"
                        ))
                        .to_dto()
                    })?,
            )
        }
        None => None,
    };
    let custom_profile = match session_agent
        .as_ref()
        .filter(|agent| agent.orchestration.is_none())
    {
        Some(agent) => {
            let workspace_path = workspace_path.as_ref().expect("checked above");
            let attached_skills_section = {
                let attached: Vec<&Skill> = all_skills
                    .iter()
                    .filter(|skill| agent.skills.iter().any(|name| name == &skill.name))
                    .collect();
                if attached.is_empty() {
                    None
                } else {
                    let mut section = String::from("## Attached Skills\n\n");
                    for skill in attached {
                        section.push_str(&format!("### {}\n\n{}\n\n", skill.name, skill.body));
                    }
                    Some(section)
                }
            };
            Some(resolve_custom_agent_profile(CustomAgentProfileRequest {
                agent: agent.clone(),
                workspace_path: workspace_path.clone(),
                run_id: request.run_id.clone(),
                attached_skills_section,
            }))
        }
        None => None,
    };
    // An orchestrated session agent is a pipeline, not a conversational
    // profile: every prompt becomes an implicit @agent delegation.
    let effective_delegation = request.delegated_agent.clone().or_else(|| {
        session_agent
            .as_ref()
            .filter(|agent| agent.orchestration.is_some())
            .map(|agent| DelegatedAgentRequest {
                name: agent.name.clone(),
                task: request.prompt.clone(),
            })
    });
    let effective_prompt = match effective_delegation.as_ref() {
        Some(delegation) => {
            let agents = deps.agents.load_agents(workspace_path.as_deref());
            match agents
                .into_iter()
                .find(|agent| agent.name == delegation.name)
            {
                Some(agent) => {
                    let (agent_provider, agent_model) = match agent.model.as_ref() {
                        Some(model) => (
                            model.provider(&request.provider).to_string(),
                            model.model_name().to_string(),
                        ),
                        None => (request.provider.clone(), request.model.clone()),
                    };
                    let agent_api_key = if agent_provider == request.provider {
                        api_key.clone()
                    } else {
                        deps.credentials
                            .api_key(&agent_provider)
                            .map_err(|error| error.to_dto())?
                    };
                    let Some(workspace_path) = workspace_path.as_ref() else {
                        return Err(LlmError::ProviderError(
                            "delegated agents require an active workspace".to_string(),
                        )
                        .to_dto());
                    };
                    let workspace = ActiveWorkspaceContext {
                        workspace_path: workspace_path.clone(),
                        source_edit_allowed: false,
                    };
                    let delegation_run_id =
                        format!("{}-delegated-{}", request.run_id, delegation.name);
                    let delegation_tool_id = format!("delegate-{}", request.run_id);
                    let trace_sid = request.session_id.as_deref().unwrap_or("").to_string();
                    deps.events.emit_stream_event(
                        &trace_sid,
                        "main",
                        &request.run_id,
                        &SessionTurnEvent::ToolCall {
                            id: delegation_tool_id.clone(),
                            name: "delegate_agent".to_string(),
                            arguments: json!({
                                "agent": delegation.name,
                                "task": delegation.task,
                            }),
                        },
                    );
                    let report = match agent.orchestration {
                        Some(AgentOrchestration::Sweep) => {
                            let (worker_provider, worker_model) = match agent
                                .worker_model
                                .as_ref()
                            {
                                Some(model) => (
                                    model.provider(&agent_provider).to_string(),
                                    model.model_name().to_string(),
                                ),
                                None => (agent_provider.clone(), agent_model.clone()),
                            };
                            let worker_api_key = if worker_provider == request.provider {
                                api_key.clone()
                            } else if worker_provider == agent_provider {
                                agent_api_key.clone()
                            } else {
                                deps.credentials
                                    .api_key(&worker_provider)
                                    .map_err(|error| error.to_dto())?
                            };
                            let on_event = |event: SessionTurnEvent| {
                                deps.events.emit_stream_event(
                                    &trace_sid,
                                    "main",
                                    &request.run_id,
                                    &event,
                                );
                            };
                            crate::bounty::sweep::run_sweep(
                                &crate::bounty::sweep::SweepRequest {
                                    agent: &agent,
                                    task: &delegation.task,
                                    lead_provider: &agent_provider,
                                    lead_model: &agent_model,
                                    lead_api_key: &agent_api_key,
                                    worker_provider: &worker_provider,
                                    worker_model: &worker_model,
                                    worker_api_key: &worker_api_key,
                                    workspace: &workspace,
                                    run_id: &delegation_run_id,
                                    session_id: request.session_id.as_deref(),
                                },
                                &on_event,
                            )
                            .await
                            .unwrap_or_else(|error| format!("Sweep failed: {error}"))
                        }
                        None => llm::run_delegated_agent(
                            &agent_provider,
                            &agent_model,
                            &agent_api_key,
                            &workspace,
                            &agent,
                            &delegation.task,
                            &delegation_run_id,
                            request.session_id.as_deref(),
                        )
                        .await
                        .unwrap_or_else(|error| format!("Delegation failed: {error}")),
                    };
                    deps.events.emit_stream_event(
                        &trace_sid,
                        "main",
                        &request.run_id,
                        &SessionTurnEvent::ToolResult {
                            id: delegation_tool_id,
                            name: "delegate_agent".to_string(),
                            result: report.clone(),
                        },
                    );
                    format!(
                        "## Delegated report from @{}\n\n{}\n\n---\n\n{}",
                        delegation.name, report, request.prompt
                    )
                }
                None => format!(
                    "## Delegated report from @{}\n\nDelegation failed: agent not found.\n\n---\n\n{}",
                    delegation.name, request.prompt
                ),
            }
        }
        None => request.prompt.clone(),
    };
    let unresolved_notes = if let Some(sid) = request.session_id.as_deref() {
        deps.sessions.unresolved_notes(sid)
    } else {
        Vec::new()
    };
    let prompt_preparation = prepare_prompt(
        &effective_prompt,
        &all_skills,
        request.invoked_skill.as_ref(),
        &unresolved_notes,
    );
    if let Some(name) = prompt_preparation.unknown_invoked_skill.as_deref() {
        tracing::warn!("Unknown skill '{}'; proceeding as normal turn", name);
    }
    let skill_script_tool = skill_script_tool(&all_skills, workspace_path.clone());
    let memory_section = match deps.memory.recall(MemoryRecallRequest {
        prompt: request.prompt.clone(),
        session_id: request.session_id.clone(),
        workspace_id: workspace_resolution.workspace_id.clone(),
    }) {
        Ok(section) => section.filter(|value| !value.trim().is_empty()),
        Err(error) => {
            tracing::warn!(
                "Memory recall failed for session {}: {}",
                request.session_id.as_deref().unwrap_or("<none>"),
                error
            );
            None
        }
    };

    let trace_sid = request.session_id.clone().unwrap_or_default();
    let trace_role = request
        .agent_id
        .clone()
        .unwrap_or_else(|| "main".to_string());
    let trace_role_for_closure = trace_role.clone();
    let events = deps.events;
    let workspace_for_verify = workspace_context.clone();
    let run_id = request.run_id;
    let run_id_for_closure = run_id.clone();
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
                memory_section,
                skill_script_tool,
                custom_profile,
                session_id: request.session_id.as_deref(),
                run_id: Some(run_id.as_str()),
            },
            Box::new(move |event| {
                events.emit_stream_event(
                    &trace_sid,
                    &trace_role_for_closure,
                    &run_id_for_closure,
                    &event,
                );
            }),
        )
        .await;

    match result {
        Ok(stream_result) => {
            deps.events.trace_done(
                request.session_id.as_deref().unwrap_or(""),
                &trace_role,
                stream_result.full_response.len(),
                stream_result.prompt_tokens,
                stream_result.response_tokens,
                stream_result.tool_calls,
            );
            if let Some(sid) = &request.session_id {
                if let Some(persistence) = successful_turn_persistence(
                    stream_result.history.as_deref(),
                    &stream_result.full_response,
                ) {
                    let display_transcript = merge_display_transcripts(
                        deps.sessions
                            .persisted_snapshot(sid)
                            .map(|snapshot| snapshot.display_transcript)
                            .as_deref()
                            .unwrap_or("[]"),
                        &persistence.display_transcript,
                    );
                    if let Err(e) = deps.sessions.persist_turn(
                        sid,
                        &display_transcript,
                        persistence.model_history.as_deref(),
                    ) {
                        tracing::warn!("Failed to persist session {}: {}", sid, e);
                    } else {
                        if let Err(e) = deps.sessions.update_status(sid, "active") {
                            tracing::warn!("Failed to update session {} status: {}", sid, e);
                        }
                        deps.conversation
                            .store_history(sid, persistence.history.clone());
                        deps.memory.schedule_ingest(MemoryIngestJobRequest {
                            display_transcript: display_transcript.clone(),
                            session_id: sid.clone(),
                            workspace_id: workspace_resolution.workspace_id.clone(),
                        });
                    }
                }
            }

            let response_for_verify = stream_result.full_response.clone();
            deps.events.emit_done(
                &run_id,
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
                &run_id,
                stream_result.source_edit_succeeded,
            ) {
                deps.verification.schedule_verification(job);
            }

            Ok(run_id.clone())
        }
        Err(e) => {
            deps.events
                .trace_error(request.session_id.as_deref().unwrap_or(""), &trace_role, &e);
            if let Some(sid) = &request.session_id {
                let mut failed_history = history_before_turn;
                failed_history.push(Message::user(request.prompt.clone()));
                deps.conversation.store_history(sid, failed_history);
                if let Some(snapshot) = deps.sessions.persisted_snapshot(sid) {
                    let persistence = failure_turn_persistence(
                        &snapshot.display_transcript,
                        snapshot.model_history.as_deref(),
                        &request.prompt,
                        &e,
                    );
                    if let Err(error) = deps.sessions.persist_turn(
                        sid,
                        &persistence.display_transcript,
                        persistence.model_history.as_deref(),
                    ) {
                        tracing::warn!("Failed to persist failed session {} turn: {}", sid, error);
                    }
                }
            }
            deps.events.emit_error(&run_id, &e);
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
            Ok(()) => ActiveWorkspaceProbe::Valid { selection },
            Err(reason) => {
                tracing::warn!(
                    "workspace tools unavailable for {}: {}",
                    selection.path.display(),
                    reason
                );
                ActiveWorkspaceProbe::Invalid { selection, reason }
            }
        },
        None => ActiveWorkspaceProbe::NoWorkspace,
    };

    resolve_active_workspace_context(probe)
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

fn transcript_entry_role(entry: &serde_json::Value) -> Option<&str> {
    entry.get("role").and_then(|role| role.as_str())
}

/// Marks a failed/stopped turn in the transcript; Model History keeps its
/// prompt, but not an assistant response.
fn is_terminal_failure_entry(entry: &serde_json::Value) -> bool {
    entry.get("error").and_then(|v| v.as_bool()) == Some(true)
        || entry.get("controlled_stop").and_then(|v| v.as_bool()) == Some(true)
}

/// An assistant entry rebuilt from a tool result whose call was pruned from
/// Model History: empty content and a single unnamed tool block.
fn is_orphan_transcript_block(entry: &serde_json::Value) -> bool {
    if transcript_entry_role(entry) != Some("assistant")
        || !entry
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .is_empty()
    {
        return false;
    }
    if let Some(blocks) = entry.get("blocks").and_then(|b| b.as_array()) {
        if blocks.len() == 1 {
            return blocks[0]
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .is_empty();
        }
    }
    false
}

/// Drop everything before the first user entry — a transcript's initiating
/// user message is always item 0 — and remove orphan tool-result residue.
fn sanitize_transcript_entries(mut entries: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    if let Some(first_user) = entries
        .iter()
        .position(|entry| transcript_entry_role(entry) == Some("user"))
    {
        entries.drain(..first_user);
    }
    entries.retain(|entry| !is_orphan_transcript_block(entry));
    entries
}

fn remove_rebuilt_failed_prompts(
    existing: &[serde_json::Value],
    rebuilt: Vec<serde_json::Value>,
) -> Vec<serde_json::Value> {
    let existing_users: Vec<usize> = existing
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (transcript_entry_role(entry) == Some("user")).then_some(index)
        })
        .collect();
    let mut failed_prompts = std::collections::HashMap::<&str, usize>::new();
    for (position, &start) in existing_users.iter().enumerate() {
        let end = existing_users
            .get(position + 1)
            .copied()
            .unwrap_or(existing.len());
        if !existing[start..end].iter().any(is_terminal_failure_entry) {
            continue;
        }
        if let Some(prompt) = existing[start]
            .get("content")
            .and_then(|content| content.as_str())
        {
            *failed_prompts.entry(prompt).or_default() += 1;
        }
    }

    let rebuilt_users: Vec<usize> = rebuilt
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (transcript_entry_role(entry) == Some("user")).then_some(index)
        })
        .collect();
    let mut remove = vec![false; rebuilt.len()];
    for (position, &start) in rebuilt_users.iter().enumerate() {
        let end = rebuilt_users
            .get(position + 1)
            .copied()
            .unwrap_or(rebuilt.len());
        if rebuilt[start + 1..end]
            .iter()
            .any(|entry| transcript_entry_role(entry) == Some("assistant"))
        {
            continue;
        }
        let Some(prompt) = rebuilt[start]
            .get("content")
            .and_then(|content| content.as_str())
        else {
            continue;
        };
        if let Some(count) = failed_prompts.get_mut(prompt).filter(|count| **count > 0) {
            *count -= 1;
            remove[start] = true;
        }
    }

    rebuilt
        .into_iter()
        .enumerate()
        .filter_map(|(index, entry)| (!remove[index]).then_some(entry))
        .collect()
}

/// Merge a freshly rebuilt Display Transcript with the persisted one.
///
/// Model History is pruned on a token budget, so the rebuild only covers a
/// suffix of the conversation while the persisted transcript records
/// everything the user saw. The overlap is positional rather than
/// content-matched: the rebuild's last user entry is the prompt whose turn
/// just completed, so its earlier user entries re-cover exactly the last
/// `new_users - 1` persisted turns. Identical repeated prompts therefore
/// cannot merge wrongly. Failed turns are not overlap candidates. If a failed
/// prompt is in Model History, it is removed from the rebuilt suffix and
/// restored with its failure row, so it is not duplicated.
pub fn merge_display_transcripts(existing_json: &str, new_json: &str) -> String {
    let existing: Vec<serde_json::Value> = serde_json::from_str(existing_json).unwrap_or_default();
    let new_entries: Vec<serde_json::Value> = serde_json::from_str(new_json).unwrap_or_default();
    let new_entries = remove_rebuilt_failed_prompts(&existing, new_entries);

    let Some(new_user_idx) = new_entries
        .iter()
        .position(|entry| transcript_entry_role(entry) == Some("user"))
    else {
        // The rebuild produced no user entry; keep history and append any
        // non-orphan assistant entries it did produce.
        let mut merged = sanitize_transcript_entries(existing);
        merged.extend(new_entries.into_iter().filter(|entry| {
            transcript_entry_role(entry) == Some("assistant") && !is_orphan_transcript_block(entry)
        }));
        return serde_json::to_string(&merged).unwrap_or_else(|_| "[]".to_string());
    };
    if existing.is_empty() {
        return serde_json::to_string(&sanitize_transcript_entries(new_entries))
            .unwrap_or_else(|_| "[]".to_string());
    }

    let rebuilt_user_turns = new_entries[new_user_idx..]
        .iter()
        .filter(|entry| transcript_entry_role(entry) == Some("user"))
        .count()
        .saturating_sub(1);

    let existing_users: Vec<usize> = existing
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (transcript_entry_role(entry) == Some("user")).then_some(index)
        })
        .collect();
    // Only turns that reached Model History can be re-covered by the rebuild.
    let persisted_users: Vec<usize> = existing_users
        .iter()
        .enumerate()
        .filter_map(|(position, &start)| {
            let end = existing_users
                .get(position + 1)
                .copied()
                .unwrap_or(existing.len());
            (!existing[start..end].iter().any(is_terminal_failure_entry)).then_some(start)
        })
        .collect();

    let overlap = rebuilt_user_turns.min(persisted_users.len());
    let cut = if overlap == 0 {
        existing.len()
    } else {
        persisted_users[persisted_users.len() - overlap]
    };

    let mut merged = sanitize_transcript_entries(existing[..cut].to_vec());

    // Display-only turns in the dropped region (a prompt whose turn failed
    // and was never retried) are re-inserted after the persisted turn they
    // originally followed, so pruning cannot erase them. `failure_groups[i]`
    // holds the failures that sat after the i-th dropped persisted turn.
    let mut failure_groups: Vec<Vec<serde_json::Value>> = vec![Vec::new(); overlap];
    if overlap > 0 {
        let pos_cut = existing_users
            .iter()
            .position(|&index| index == cut)
            .unwrap_or(existing_users.len());
        let mut dropped_persisted = 0usize;
        for pos in pos_cut..existing_users.len() {
            let start = existing_users[pos];
            let end = existing_users
                .get(pos + 1)
                .copied()
                .unwrap_or(existing.len());
            let turn = &existing[start..end];
            if turn.iter().any(is_terminal_failure_entry) {
                if dropped_persisted > 0 {
                    failure_groups[dropped_persisted - 1].extend(turn.iter().cloned());
                }
            } else {
                dropped_persisted += 1;
            }
        }
    }

    // The i-th dropped persisted turn re-covers `new_users[rebuilt - overlap
    // + i]`; its failures slot in before the next user entry, which for the
    // last one is the current prompt.
    let new_users: Vec<usize> = new_entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (transcript_entry_role(entry) == Some("user")).then_some(index)
        })
        .collect();
    let keep = |entry: &&serde_json::Value| !is_orphan_transcript_block(entry);
    let mut cursor = new_user_idx;
    for (i, group) in failure_groups.iter().enumerate() {
        let boundary = new_users[rebuilt_user_turns - overlap + i + 1];
        merged.extend(new_entries[cursor..boundary].iter().filter(keep).cloned());
        merged.extend_from_slice(group);
        cursor = boundary;
    }
    merged.extend(new_entries[cursor..].iter().filter(keep).cloned());
    serde_json::to_string(&merged).unwrap_or_else(|_| "[]".to_string())
}

pub fn successful_turn_persistence(
    history: Option<&[Message]>,
    response: &str,
) -> Option<SuccessfulTurnPersistence> {
    let history = history?;
    let mut display = build_display_transcript(history);
    let last_assistant_has_text = history.iter().rev().find_map(|message| match message {
        Message::Assistant(assistant) => Some(assistant.content.iter().any(
            |part| matches!(part, AssistantContent::Text(text) if !text.text.trim().is_empty()),
        )),
        _ => None,
    });
    if last_assistant_has_text != Some(true) {
        add_fallback_response(&mut display, response);
    }
    let display_transcript = serde_json::to_string(&display).unwrap_or_else(|_| "[]".to_string());
    let model_history = serde_json::to_string(history).ok();

    Some(SuccessfulTurnPersistence {
        display_transcript,
        model_history,
        history: history.to_vec(),
    })
}

fn add_fallback_response(display: &mut Vec<serde_json::Value>, response: &str) {
    if response.trim().is_empty() {
        return;
    }

    if let Some(entry) = display
        .iter_mut()
        .rev()
        .find(|entry| entry.get("role").and_then(|role| role.as_str()) == Some("assistant"))
    {
        if entry
            .get("content")
            .and_then(|content| content.as_str())
            .is_none_or(|content| content.trim().is_empty())
        {
            entry["content"] = json!(response);
            let blocks = entry
                .get_mut("blocks")
                .and_then(|blocks| blocks.as_array_mut());
            if let Some(blocks) = blocks {
                if let Some(empty_text) = blocks.iter_mut().find(|block| {
                    block.get("kind").and_then(|kind| kind.as_str()) == Some("text")
                        && block
                            .get("text")
                            .and_then(|text| text.as_str())
                            .is_none_or(|text| text.trim().is_empty())
                }) {
                    empty_text["text"] = json!(response);
                    return;
                }
                let id = format!("text-{}", blocks.len());
                blocks.push(json!({"kind": "text", "id": id, "text": response}));
            } else {
                entry["blocks"] = json!([{"kind": "text", "id": "text-0", "text": response}]);
            }
            return;
        }
    }

    display.push(json!({
        "role": "assistant",
        "content": response,
        "blocks": [{"kind": "text", "id": "text-0", "text": response}],
    }));
}

pub fn failure_turn_persistence(
    existing_display_transcript: &str,
    existing_model_history: Option<&str>,
    failed_prompt: &str,
    error: &LlmError,
) -> FailureTurnPersistence {
    let mut transcript: Vec<serde_json::Value> =
        serde_json::from_str(existing_display_transcript).unwrap_or_default();
    let (message, is_controlled_stop) = match error {
        LlmError::ControlledStop(message) => (message.clone(), true),
        _ => (format!("Error: {}", error.to_dto().message), false),
    };

    if !transcript.last().is_some_and(|entry| {
        transcript_entry_role(entry) == Some("user")
            && entry.get("content").and_then(|content| content.as_str()) == Some(failed_prompt)
    }) {
        transcript.push(json!({"role": "user", "content": failed_prompt}));
    }
    transcript.push(json!({
        "role": "assistant",
        "content": message,
        "blocks": [],
        "error": !is_controlled_stop,
        "controlled_stop": is_controlled_stop,
    }));

    let model_history = match existing_model_history {
        Some(raw) => match crate::model_history::parse_model_history(raw) {
            Ok(mut history) => {
                history.push(Message::user(failed_prompt));
                serde_json::to_string(&history)
                    .ok()
                    .or_else(|| Some(raw.to_string()))
            }
            Err(error) => {
                tracing::warn!("Failed to append failed prompt to model history: {}", error);
                Some(raw.to_string())
            }
        },
        None => serde_json::to_string(&vec![Message::user(failed_prompt)]).ok(),
    };

    FailureTurnPersistence {
        display_transcript: serde_json::to_string(&transcript).unwrap_or_else(|_| "[]".to_string()),
        model_history,
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
                                    ToolResultContent::Text(t) => t.text.clone(),
                                    ToolResultContent::Json { value } => value.to_string(),
                                    _ => String::new(),
                                })
                                .collect::<Vec<_>>()
                                .join("");
                            pending_tool_results.insert(tool_result.call.to_string(), result_text);
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
            Message::Assistant(message) => {
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                let mut text_parts: Vec<String> = Vec::new();
                for item in message.content.iter() {
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
                                "id": tool_call.id.to_string(),
                                "name": tool_call.function.name.as_str(),
                                "arguments": observable_tool_arguments(
                                    tool_call.function.name.as_str(),
                                    &tool_call.function.arguments_value(),
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

    // A transcript starts at its first user message. Model History pruned for
    // the token budget can begin mid-turn, leaving assistant blocks orphaned
    // in front of the initiating prompt.
    if let Some(first_user) = entries
        .iter()
        .position(|entry| entry.get("role").and_then(|r| r.as_str()) == Some("user"))
    {
        entries.drain(..first_user);
    }

    entries
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SessionTurnEvent {
    RetryReset {
        text: String,
        reasoning_ids: Vec<String>,
    },
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
    Reasoning {
        id: String,
        text: String,
        phase: ReasoningPhase,
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
    Subagent {
        parent_id: String,
        agent: SubagentKind,
        step: SubagentStep,
    },
}

impl From<StreamEvent> for SessionTurnEvent {
    fn from(event: StreamEvent) -> Self {
        match event {
            StreamEvent::RetryReset {
                text,
                reasoning_ids,
            } => Self::RetryReset {
                text,
                reasoning_ids,
            },
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
            StreamEvent::Reasoning(ReasoningPayload { id, text, phase }) => {
                SessionTurnEvent::Reasoning { id, text, phase }
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
            StreamEvent::Subagent {
                parent_id,
                agent,
                step,
            } => SessionTurnEvent::Subagent {
                parent_id,
                agent,
                step,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiEventPayload {
    pub name: &'static str,
    pub payload: serde_json::Value,
}

pub fn ui_event_payload(event: &SessionTurnEvent, run_id: &str) -> UiEventPayload {
    match event {
        SessionTurnEvent::RetryReset {
            text,
            reasoning_ids,
        } => UiEventPayload {
            name: "llm-retry-reset",
            payload: json!({ "runId": run_id, "text": text, "reasoningIds": reasoning_ids }),
        },
        SessionTurnEvent::TextToken(token) => UiEventPayload {
            name: "llm-token",
            payload: json!({ "runId": run_id, "token": token }),
        },
        SessionTurnEvent::ToolCall {
            id,
            name,
            arguments,
        } => UiEventPayload {
            name: "llm-tool-call",
            payload: json!({
                "runId": run_id,
                "id": id,
                "name": name,
                "arguments": observable_tool_arguments(name, arguments)
            }),
        },
        SessionTurnEvent::ToolResult { id, name, result } => UiEventPayload {
            name: "llm-tool-result",
            payload: json!({ "runId": run_id, "id": id, "name": name, "result": result }),
        },
        SessionTurnEvent::Reasoning { id, text, phase } => UiEventPayload {
            name: "llm-reasoning",
            payload: json!({
                "runId": run_id,
                "id": id,
                "text": text,
                "phase": phase,
            }),
        },
        SessionTurnEvent::LoopWarning { count, tool_name } => UiEventPayload {
            name: "llm-loop-warning",
            payload: json!({ "runId": run_id, "count": count, "toolName": tool_name }),
        },
        SessionTurnEvent::LoopStopped {
            count,
            tool_name,
            message,
        } => UiEventPayload {
            name: "llm-loop-stopped",
            payload: json!({ "runId": run_id, "count": count, "toolName": tool_name, "message": message }),
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
                "runId": run_id,
                "kind": kind,
                "provider": provider,
                "model": model,
                "variant": variant,
                "message": message
            }),
        },
        SessionTurnEvent::Subagent {
            parent_id,
            agent,
            step,
        } => subagent_ui_event_payload(run_id, Some(parent_id), *agent, step),
    }
}

/// Live sub-agent step for the frontend. `parent_id` names the parent tool
/// call; verification has none because it runs after the turn.
pub fn subagent_ui_event_payload(
    run_id: &str,
    parent_id: Option<&str>,
    agent: SubagentKind,
    step: &SubagentStep,
) -> UiEventPayload {
    let step = match step {
        SubagentStep::ToolCall {
            id,
            name,
            arguments,
        } => SubagentStep::ToolCall {
            id: id.clone(),
            name: name.clone(),
            arguments: observable_tool_arguments(name, arguments),
        },
        step => step.clone(),
    };
    UiEventPayload {
        name: "llm-subagent-event",
        payload: json!({
            "runId": run_id,
            "parentId": parent_id,
            "agent": agent,
            "step": step,
        }),
    }
}

pub fn trace_event_for_session_turn_event(
    event: &SessionTurnEvent,
    session_id: &str,
    role: &str,
    timestamp: u64,
) -> Option<trace::TraceEvent> {
    match event {
        SessionTurnEvent::TextToken(_) | SessionTurnEvent::RetryReset { .. } => None,
        SessionTurnEvent::Reasoning { .. } | SessionTurnEvent::Subagent { .. } => None,
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
    pub run_id: String,
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
    run_id: &str,
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
        run_id: run_id.to_string(),
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
    Valid {
        selection: ActiveWorkspaceSelection,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedActiveWorkspaceContext {
    pub workspace_id: Option<String>,
    pub workspace_path: Option<PathBuf>,
    pub tool_context: Option<ActiveWorkspaceContext>,
}

pub fn resolve_active_workspace_context(
    probe: ActiveWorkspaceProbe,
) -> ResolvedActiveWorkspaceContext {
    match probe {
        ActiveWorkspaceProbe::NoWorkspace => ResolvedActiveWorkspaceContext {
            workspace_id: None,
            workspace_path: None,
            tool_context: None,
        },
        ActiveWorkspaceProbe::Invalid { selection, .. } => ResolvedActiveWorkspaceContext {
            workspace_id: Some(selection.id),
            workspace_path: Some(selection.path),
            tool_context: None,
        },
        ActiveWorkspaceProbe::Valid { selection } => ResolvedActiveWorkspaceContext {
            workspace_id: Some(selection.id),
            workspace_path: Some(selection.path.clone()),
            tool_context: Some(ActiveWorkspaceContext {
                workspace_path: selection.path,
                source_edit_allowed: true,
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::completion::message::{CallId, ToolName};
    use std::{future, sync::Mutex};

    type ModelUpdate = (String, String, String, Option<String>);
    type DoneTrace = (String, String, usize, usize, usize, usize);
    type SavedTurn = (String, String, String, Option<String>);

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
        Message::user(text)
    }

    fn assistant_message(text: &str) -> Message {
        Message::assistant(text)
    }

    /// Build an assistant message with interleaved text + tool-call content parts
    /// in the given order. Each `(name, id)` tuple becomes a tool call.
    fn assistant_message_with_blocks(parts: &[AssistantPart]) -> Message {
        let mut content: Vec<AssistantContent> = Vec::new();
        for part in parts {
            match part {
                AssistantPart::Text(t) => {
                    content.push(AssistantContent::text(*t));
                }
                AssistantPart::ToolCall {
                    id,
                    name,
                    arguments,
                } => {
                    content.push(AssistantContent::tool_call(
                        *id,
                        ToolName::new(*name).unwrap(),
                        arguments.clone(),
                    ));
                }
            }
        }
        assert!(!content.is_empty(), "at least one content part");
        Message::from(content)
    }

    /// Build a user message carrying a tool result for the given tool-call id.
    fn tool_result_message(id: &str, text: &str) -> Message {
        Message::tool_result(
            CallId::from_wire(id),
            ToolName::new("read_file").unwrap(),
            text,
        )
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
        memory_section: Option<String>,
        skill_script_available: bool,
    }

    struct FakeSessionTurnAdapters {
        active_workspace: Option<ActiveWorkspaceSelection>,
        validate_result: Mutex<Result<(), String>>,
        api_key: String,
        validated_bindings: Mutex<Vec<(String, Option<String>)>>,
        unresolved_notes: Vec<SessionNote>,
        persisted_snapshot: Mutex<Option<SessionSnapshot>>,
        persisted_turns: Mutex<Vec<PersistedTurn>>,
        statuses: Mutex<Vec<(String, String)>>,
        chat_history: Vec<Message>,
        stored_histories: Mutex<Vec<(String, Vec<Message>)>>,
        skills: Vec<Skill>,
        agents: Vec<AgentDefinition>,
        stream_result: Mutex<Option<Result<StreamCompletionResult, LlmError>>>,
        stream_requests: Mutex<Vec<CapturedStreamRequest>>,
        model_updates: Mutex<Vec<ModelUpdate>>,
        stream_events: Mutex<Vec<(String, String, SessionTurnEvent)>>,
        done_traces: Mutex<Vec<DoneTrace>>,
        error_traces: Mutex<Vec<(String, String, String)>>,
        done_responses: Mutex<Vec<(String, String)>>,
        emitted_errors: Mutex<Vec<(String, String)>>,
        verifications: Mutex<Vec<VerificationJobRequest>>,
        persist_result: Mutex<Result<(), String>>,
        recalled_section: Option<String>,
        recall_error: Option<String>,
        recall_requests: Mutex<Vec<MemoryRecallRequest>>,
        ingests: Mutex<Vec<MemoryIngestJobRequest>>,
    }

    impl FakeSessionTurnAdapters {
        fn with_stream_result(result: Result<StreamCompletionResult, LlmError>) -> Self {
            Self {
                active_workspace: Some(ActiveWorkspaceSelection {
                    id: "workspace-1".to_string(),
                    path: PathBuf::from("/tmp/workspace"),
                }),
                validate_result: Mutex::new(Ok(())),
                api_key: "api-key".to_string(),
                validated_bindings: Mutex::new(Vec::new()),
                unresolved_notes: Vec::new(),
                persisted_snapshot: Mutex::new(None),
                persisted_turns: Mutex::new(Vec::new()),
                statuses: Mutex::new(Vec::new()),
                chat_history: Vec::new(),
                stored_histories: Mutex::new(Vec::new()),
                skills: Vec::new(),
                agents: Vec::new(),
                stream_result: Mutex::new(Some(result)),
                stream_requests: Mutex::new(Vec::new()),
                model_updates: Mutex::new(Vec::new()),
                stream_events: Mutex::new(Vec::new()),
                done_traces: Mutex::new(Vec::new()),
                error_traces: Mutex::new(Vec::new()),
                done_responses: Mutex::new(Vec::new()),
                emitted_errors: Mutex::new(Vec::new()),
                verifications: Mutex::new(Vec::new()),
                persist_result: Mutex::new(Ok(())),
                recalled_section: None,
                recall_error: None,
                recall_requests: Mutex::new(Vec::new()),
                ingests: Mutex::new(Vec::new()),
            }
        }

        fn deps(&self) -> StreamingTurnDependencies<'_> {
            StreamingTurnDependencies {
                workspace: self,
                credentials: self,
                sessions: self,
                conversation: self,
                skills: self,
                agents: self,
                llm: self,
                events: self,
                verification: self,
                memory: self,
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

        fn persisted_snapshot(&self, _session_id: &str) -> Option<SessionSnapshot> {
            self.persisted_snapshot.lock().unwrap().clone()
        }

        fn persist_turn(
            &self,
            session_id: &str,
            display_transcript: &str,
            model_history: Option<&str>,
        ) -> Result<(), String> {
            self.persist_result.lock().unwrap().clone()?;
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

    impl SessionTurnAgents for FakeSessionTurnAdapters {
        fn load_agents(&self, _workspace_path: Option<&Path>) -> Vec<AgentDefinition> {
            self.agents.clone()
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
                    memory_section: request.memory_section.clone(),
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
        fn emit_stream_event(
            &self,
            session_id: &str,
            role: &str,
            _run_id: &str,
            event: &SessionTurnEvent,
        ) {
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
            run_id: &str,
            response: &str,
            _prompt_tokens: usize,
            _response_tokens: usize,
            _tool_calls: usize,
        ) {
            self.done_responses
                .lock()
                .unwrap()
                .push((run_id.to_string(), response.to_string()));
        }

        fn emit_error(&self, run_id: &str, error: &LlmError) {
            self.emitted_errors
                .lock()
                .unwrap()
                .push((run_id.to_string(), error.to_dto().code));
        }
    }

    impl SessionTurnVerification for FakeSessionTurnAdapters {
        fn schedule_verification(&self, job: VerificationJobRequest) {
            self.verifications.lock().unwrap().push(job);
        }
    }

    impl SessionTurnMemory for FakeSessionTurnAdapters {
        fn recall(&self, request: MemoryRecallRequest) -> Result<Option<String>, String> {
            self.recall_requests.lock().unwrap().push(request);
            if let Some(error) = &self.recall_error {
                return Err(error.clone());
            }
            Ok(self.recalled_section.clone())
        }

        fn schedule_ingest(&self, job: MemoryIngestJobRequest) {
            self.ingests.lock().unwrap().push(job);
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

        let decision = successful_turn_persistence(Some(&history), "hi").unwrap();

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
        assert!(successful_turn_persistence(None, "").is_none());
    }

    #[test]
    fn successful_turn_persists_fallback_for_textless_assistant() {
        let response = "The agent ended this turn without a text response.";
        let history = vec![user_message("do the task"), assistant_message("")];

        let decision = successful_turn_persistence(Some(&history), response).unwrap();
        let display: serde_json::Value =
            serde_json::from_str(&decision.display_transcript).unwrap();

        assert_eq!(display[1]["content"], response);
        assert_eq!(display[1]["blocks"][0]["text"], response);
    }

    #[test]
    fn merge_preserves_historical_turns_when_context_is_pruned() {
        let existing = json!([
            { "role": "user", "content": "Assess what is needed" },
            {
                "role": "assistant",
                "content": "Short answer: app is wired up",
                "blocks": [{ "kind": "text", "id": "text-0", "text": "Short answer: app is wired up" }]
            }
        ]);
        let existing_str = serde_json::to_string(&existing).unwrap();

        // Turn 2 rebuild: earlier context pruned, leaving an orphaned tool
        // result before the turn prompt.
        let new_json = serde_json::to_string(&json!([
            {
                "role": "assistant",
                "content": "",
                "blocks": [{ "kind": "tool", "id": "call-orphan", "name": "", "result": "x" }]
            },
            { "role": "user", "content": "proceed" },
            {
                "role": "assistant",
                "content": "Done. I fixed everything.",
                "blocks": [{ "kind": "text", "id": "text-0", "text": "Done. I fixed everything." }]
            }
        ]))
        .unwrap();

        let display: Vec<serde_json::Value> =
            serde_json::from_str(&merge_display_transcripts(&existing_str, &new_json)).unwrap();

        assert_eq!(display.len(), 4);
        assert_eq!(display[0]["role"], "user");
        assert_eq!(display[0]["content"], "Assess what is needed");
        assert_eq!(display[1]["role"], "assistant");
        assert_eq!(display[1]["content"], "Short answer: app is wired up");
        assert_eq!(display[2]["role"], "user");
        assert_eq!(display[2]["content"], "proceed");
        assert_eq!(display[3]["role"], "assistant");
        assert_eq!(display[3]["content"], "Done. I fixed everything.");
    }

    #[test]
    fn merge_keeps_earlier_turn_when_prompts_repeat_verbatim() {
        // Two identical prompts ("proceed" twice): the rebuild's last user
        // entry is the new turn, so the earlier identical turn must survive.
        let existing = json!([
            { "role": "user", "content": "proceed" },
            { "role": "assistant", "content": "first answer", "blocks": [] }
        ]);
        let existing_str = serde_json::to_string(&existing).unwrap();
        let new_json = serde_json::to_string(&json!([
            { "role": "user", "content": "proceed" },
            { "role": "assistant", "content": "second answer", "blocks": [] }
        ]))
        .unwrap();

        let display: Vec<serde_json::Value> =
            serde_json::from_str(&merge_display_transcripts(&existing_str, &new_json)).unwrap();

        assert_eq!(display.len(), 4);
        assert_eq!(display[1]["content"], "first answer");
        assert_eq!(display[3]["content"], "second answer");
    }

    #[test]
    fn merge_replaces_only_the_rebuilt_suffix_of_existing() {
        let existing = json!([
            { "role": "user", "content": "one" },
            { "role": "assistant", "content": "answer one", "blocks": [] },
            { "role": "user", "content": "two" },
            { "role": "assistant", "content": "answer two", "blocks": [] }
        ]);
        let existing_str = serde_json::to_string(&existing).unwrap();
        // Model history still covers turn 2 plus the new turn: overlap is the
        // last persisted turn only.
        let new_json = serde_json::to_string(&json!([
            { "role": "user", "content": "two" },
            { "role": "assistant", "content": "answer two", "blocks": [] },
            { "role": "user", "content": "three" },
            { "role": "assistant", "content": "answer three", "blocks": [] }
        ]))
        .unwrap();

        let display: Vec<serde_json::Value> =
            serde_json::from_str(&merge_display_transcripts(&existing_str, &new_json)).unwrap();

        assert_eq!(display.len(), 6);
        assert_eq!(display[0]["content"], "one");
        assert_eq!(display[4]["content"], "three");
    }

    #[test]
    fn merge_preserves_failed_turn_rows_before_the_new_prompt() {
        let existing = json!([
            { "role": "user", "content": "first" },
            { "role": "assistant", "content": "answer", "blocks": [] },
            { "role": "user", "content": "second" },
            { "role": "assistant", "content": "Error: down", "blocks": [], "error": true }
        ]);
        let existing_str = serde_json::to_string(&existing).unwrap();
        // Legacy turn 2 failed before its prompt reached Model History; turn 3 now succeeds.
        let new_json = serde_json::to_string(&json!([
            { "role": "user", "content": "first" },
            { "role": "assistant", "content": "answer", "blocks": [] },
            { "role": "user", "content": "third" },
            { "role": "assistant", "content": "ok", "blocks": [] }
        ]))
        .unwrap();

        let display: Vec<serde_json::Value> =
            serde_json::from_str(&merge_display_transcripts(&existing_str, &new_json)).unwrap();

        let contents: Vec<&str> = display
            .iter()
            .map(|entry| entry["content"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(
            contents,
            vec!["first", "answer", "second", "Error: down", "third", "ok"]
        );
    }

    #[test]
    fn merge_does_not_duplicate_failed_prompt_rebuilt_from_model_history() {
        let existing = json!([
            { "role": "user", "content": "first" },
            { "role": "assistant", "content": "answer", "blocks": [] },
            { "role": "user", "content": "second" },
            { "role": "assistant", "content": "Error: down", "blocks": [], "error": true }
        ]);
        let new = json!([
            { "role": "user", "content": "first" },
            { "role": "assistant", "content": "answer", "blocks": [] },
            { "role": "user", "content": "second" },
            { "role": "user", "content": "third" },
            { "role": "assistant", "content": "ok", "blocks": [] }
        ]);

        let merged: Vec<serde_json::Value> = serde_json::from_str(&merge_display_transcripts(
            &existing.to_string(),
            &new.to_string(),
        ))
        .unwrap();

        assert_eq!(
            merged,
            existing
                .as_array()
                .unwrap()
                .iter()
                .cloned()
                .chain([
                    json!({"role": "user", "content": "third"}),
                    json!({"role": "assistant", "content": "ok", "blocks": []}),
                ])
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn merge_repairs_existing_transcript_starting_with_orphans() {
        let existing = json!([
            {
                "role": "assistant",
                "content": "",
                "blocks": [{ "kind": "tool", "id": "orphan", "name": "", "result": "x" }]
            },
            { "role": "user", "content": "first" },
            { "role": "assistant", "content": "answer", "blocks": [] }
        ]);
        let existing_str = serde_json::to_string(&existing).unwrap();
        let new_json = serde_json::to_string(&json!([
            { "role": "user", "content": "next" },
            { "role": "assistant", "content": "ok", "blocks": [] }
        ]))
        .unwrap();

        let display: Vec<serde_json::Value> =
            serde_json::from_str(&merge_display_transcripts(&existing_str, &new_json)).unwrap();

        assert_eq!(display[0]["role"], "user");
        assert_eq!(display[0]["content"], "first");
        assert_eq!(display.len(), 4);
    }

    #[test]
    fn merge_preserves_failed_turn_between_persisted_turns() {
        // Turn 2 failed; turns 1 and 3 reached Model History and are both
        // re-covered by the rebuild. The failed turn must be spliced back
        // after the turn it followed, not dropped with the rebuilt suffix.
        let existing = json!([
            { "role": "user", "content": "first" },
            { "role": "assistant", "content": "one", "blocks": [] },
            { "role": "user", "content": "second" },
            { "role": "assistant", "content": "Error: down", "blocks": [], "error": true },
            { "role": "user", "content": "third" },
            { "role": "assistant", "content": "three", "blocks": [] }
        ]);
        let existing_str = serde_json::to_string(&existing).unwrap();
        let new_json = serde_json::to_string(&json!([
            { "role": "user", "content": "first" },
            { "role": "assistant", "content": "one", "blocks": [] },
            { "role": "user", "content": "third" },
            { "role": "assistant", "content": "three", "blocks": [] },
            { "role": "user", "content": "fourth" },
            { "role": "assistant", "content": "four", "blocks": [] }
        ]))
        .unwrap();

        let display: Vec<serde_json::Value> =
            serde_json::from_str(&merge_display_transcripts(&existing_str, &new_json)).unwrap();

        let contents: Vec<&str> = display
            .iter()
            .map(|entry| entry["content"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(
            contents,
            vec![
                "first", "one", "second", "Error: down", "third", "three", "fourth", "four"
            ]
        );
    }

    #[test]
    fn merge_preserves_controlled_stop_rows() {
        let existing = json!([
            { "role": "user", "content": "first" },
            { "role": "assistant", "content": "answer", "blocks": [] },
            { "role": "user", "content": "second" },
            { "role": "assistant", "content": "Stopped", "blocks": [], "controlled_stop": true }
        ]);
        let existing_str = serde_json::to_string(&existing).unwrap();
        let new_json = serde_json::to_string(&json!([
            { "role": "user", "content": "first" },
            { "role": "assistant", "content": "answer", "blocks": [] },
            { "role": "user", "content": "third" },
            { "role": "assistant", "content": "ok", "blocks": [] }
        ]))
        .unwrap();

        let display: Vec<serde_json::Value> =
            serde_json::from_str(&merge_display_transcripts(&existing_str, &new_json)).unwrap();

        let contents: Vec<&str> = display
            .iter()
            .map(|entry| entry["content"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(
            contents,
            vec!["first", "answer", "second", "Stopped", "third", "ok"]
        );
    }

    #[test]
    fn merge_filters_orphans_when_rebuild_has_no_user() {
        let existing = json!([
            { "role": "user", "content": "first" },
            { "role": "assistant", "content": "answer", "blocks": [] }
        ]);
        let existing_str = serde_json::to_string(&existing).unwrap();
        let new_json = serde_json::to_string(&json!([
            {
                "role": "assistant",
                "content": "",
                "blocks": [{ "kind": "tool", "id": "orphan", "name": "", "result": "x" }]
            },
            { "role": "assistant", "content": "note", "blocks": [] }
        ]))
        .unwrap();

        let display: Vec<serde_json::Value> =
            serde_json::from_str(&merge_display_transcripts(&existing_str, &new_json)).unwrap();

        let contents: Vec<&str> = display
            .iter()
            .map(|entry| entry["content"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(contents, vec!["first", "answer", "note"]);
    }

    #[test]
    fn build_display_transcript_starts_at_first_user_message() {
        // Pruned history can begin with assistant tool calls whose prompt was
        // dropped; the initiating user message is still item 0.
        let history = vec![
            assistant_message_with_blocks(&[AssistantPart::ToolCall {
                id: "call-0",
                name: "read_file",
                arguments: json!({ "path": "src/lib.rs" }),
            }]),
            tool_result_message("call-0", "contents"),
            user_message("proceed"),
            assistant_message("done"),
        ];

        let display = build_display_transcript(&history);

        assert_eq!(display[0]["role"], "user");
        assert_eq!(display[0]["content"], "proceed");
    }

    #[test]
    fn provider_failure_appends_error_and_failed_prompt_to_history() {
        let mut history = vec![user_message("before"), assistant_message("answer")];
        let existing_history = serde_json::to_string(&history).unwrap();
        let failed_prompt = "continue after the failure";
        let decision = failure_turn_persistence(
            r#"[{"role":"user","content":"hi"}]"#,
            Some(&existing_history),
            failed_prompt,
            &LlmError::ProviderError("network down".to_string()),
        );

        let display: serde_json::Value =
            serde_json::from_str(&decision.display_transcript).unwrap();
        assert_eq!(display[0], json!({"role":"user","content":"hi"}));
        assert_eq!(
            display[1],
            json!({"role":"user","content":"continue after the failure"})
        );
        assert_eq!(display[2]["role"], "assistant");
        assert_eq!(display[2]["error"], true);
        assert_eq!(display[2]["controlled_stop"], false);
        history.push(user_message(failed_prompt));
        let persisted_history: Vec<Message> =
            serde_json::from_str(decision.model_history.as_deref().unwrap()).unwrap();
        assert_eq!(persisted_history, history);
    }

    #[test]
    fn controlled_stop_persists_the_interrupted_prompt() {
        let failed_prompt = "continue the interrupted task";
        let decision = failure_turn_persistence(
            "[]",
            None,
            failed_prompt,
            &LlmError::ControlledStop("Agent stopped".to_string()),
        );

        let display: serde_json::Value =
            serde_json::from_str(&decision.display_transcript).unwrap();
        assert_eq!(display[0], json!({"role":"user","content":failed_prompt}));
        assert_eq!(display[1]["content"], "Agent stopped");
        assert_eq!(display[1]["error"], false);
        assert_eq!(display[1]["controlled_stop"], true);
        let persisted_history: Vec<Message> =
            serde_json::from_str(decision.model_history.as_deref().unwrap()).unwrap();
        assert_eq!(persisted_history, vec![user_message(failed_prompt)]);
    }

    #[test]
    fn provider_failure_persists_sanitized_message_without_raw_token() {
        let raw = format!(
            "request failed: sk-{} leaked\ndangerous second line",
            "a".repeat(30)
        );
        let error = LlmError::ProviderError(raw);
        let expected = format!("Error: {}", error.to_dto().message);
        let existing_history = serde_json::to_string(&vec![
            user_message("earlier prompt"),
            assistant_message("earlier answer"),
        ])
        .unwrap();
        let decision = failure_turn_persistence(
            r#"[{"role":"user","content":"hi"}]"#,
            Some(&existing_history),
            "hi",
            &error,
        );

        let display: serde_json::Value =
            serde_json::from_str(&decision.display_transcript).unwrap();
        let persisted = display[1]["content"].as_str().unwrap();
        assert_eq!(persisted, expected);
        assert_eq!(display[1]["error"], true);
        assert!(!persisted.contains("sk-"));
        assert!(!persisted.contains("dangerous second line"));
        assert!(persisted.contains("[REDACTED]"));
        let persisted_history: Vec<Message> =
            serde_json::from_str(decision.model_history.as_deref().unwrap()).unwrap();
        assert_eq!(
            persisted_history,
            vec![
                user_message("earlier prompt"),
                assistant_message("earlier answer"),
                user_message("hi"),
            ]
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
        let run_id = "run-1";
        let token = ui_event_payload(&SessionTurnEvent::TextToken("hello".to_string()), run_id);
        assert_eq!(token.name, "llm-token");
        assert_eq!(token.payload, json!({ "runId": "run-1", "token": "hello" }));

        let tool_event = SessionTurnEvent::ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: json!({ "path": "src/lib.rs" }),
        };
        let payload = ui_event_payload(&tool_event, run_id);

        assert_eq!(payload.name, "llm-tool-call");
        assert_eq!(
            payload.payload,
            json!({ "runId": "run-1", "id": "call-1", "name": "read_file", "arguments": { "path": "src/lib.rs" } })
        );

        let edit_payload = ui_event_payload(
            &SessionTurnEvent::ToolCall {
                id: "call-2".to_string(),
                name: "source_edit".to_string(),
                arguments: json!({
                    "path": "src/lib.rs",
                    "old_text": "secret old snippet",
                    "new_text": "secret new snippet"
                }),
            },
            run_id,
        );
        assert_eq!(edit_payload.name, "llm-tool-call");
        assert_eq!(
            edit_payload.payload,
            json!({
                "runId": "run-1",
                "id": "call-2",
                "name": "source_edit",
                "arguments": {
                    "path": "src/lib.rs",
                    "old_text": "[REDACTED]",
                    "new_text": "[REDACTED]"
                }
            })
        );

        let result = ui_event_payload(
            &SessionTurnEvent::ToolResult {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                result: "contents".to_string(),
            },
            run_id,
        );
        assert_eq!(result.name, "llm-tool-result");
        assert_eq!(
            result.payload,
            json!({ "runId": "run-1", "id": "call-1", "name": "read_file", "result": "contents" })
        );

        let warning = ui_event_payload(
            &SessionTurnEvent::LoopWarning {
                count: 3,
                tool_name: "read_file".to_string(),
            },
            run_id,
        );
        assert_eq!(warning.name, "llm-loop-warning");
        assert_eq!(
            warning.payload,
            json!({ "runId": "run-1", "count": 3, "toolName": "read_file" })
        );

        let stopped = ui_event_payload(
            &SessionTurnEvent::LoopStopped {
                count: 5,
                tool_name: "read_file".to_string(),
                message: "Agent stopped".to_string(),
            },
            run_id,
        );
        assert_eq!(stopped.name, "llm-loop-stopped");
        assert_eq!(
            stopped.payload,
            json!({ "runId": "run-1", "count": 5, "toolName": "read_file", "message": "Agent stopped" })
        );
    }

    #[test]
    fn subagent_event_payload_names_its_parent_and_redacts_edit_arguments() {
        let event = SessionTurnEvent::from(StreamEvent::Subagent {
            parent_id: "call-7".into(),
            agent: SubagentKind::Exploration,
            step: SubagentStep::ToolCall {
                id: "inner-1".into(),
                name: "source_edit".into(),
                arguments: json!({"path": "a.rs", "old_text": "secret", "new_text": "x"}),
            },
        });
        let payload = ui_event_payload(&event, "run-1");
        assert_eq!(payload.name, "llm-subagent-event");
        assert_eq!(
            payload.payload,
            json!({
                "runId": "run-1",
                "parentId": "call-7",
                "agent": "exploration",
                "step": {
                    "type": "toolCall",
                    "id": "inner-1",
                    "name": "source_edit",
                    "arguments": {"path": "a.rs", "old_text": "[REDACTED]", "new_text": "[REDACTED]"}
                }
            })
        );
        assert!(trace_event_for_session_turn_event(&event, "s", "main", 0).is_none());
    }

    #[test]
    fn reasoning_event_emits_ephemeral_payload_and_no_trace() {
        let run_id = "run-1";
        let delta = ui_event_payload(
            &SessionTurnEvent::Reasoning {
                id: "rs-1".to_string(),
                text: "thinking ".to_string(),
                phase: crate::llm::ReasoningPhase::Delta,
            },
            run_id,
        );
        assert_eq!(delta.name, "llm-reasoning");
        assert_eq!(
            delta.payload,
            json!({ "runId": "run-1", "id": "rs-1", "text": "thinking ", "phase": "delta" })
        );

        let complete = ui_event_payload(
            &SessionTurnEvent::Reasoning {
                id: "rs-1".to_string(),
                text: "thinking done".to_string(),
                phase: crate::llm::ReasoningPhase::Complete,
            },
            run_id,
        );
        assert_eq!(complete.name, "llm-reasoning");
        assert_eq!(
            complete.payload,
            json!({ "runId": "run-1", "id": "rs-1", "text": "thinking done", "phase": "complete" })
        );

        // Reasoning must never produce a trace event — the trace is intended
        // for tool calls, results, and warnings, not ephemeral model
        // reasoning content.
        let trace = trace_event_for_session_turn_event(
            &SessionTurnEvent::Reasoning {
                id: "rs-1".to_string(),
                text: "should not trace".to_string(),
                phase: crate::llm::ReasoningPhase::Complete,
            },
            "session-1",
            "main",
            123,
        );
        assert!(trace.is_none());
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
            source_edit_allowed: true,
        };

        assert!(verification_job_request(
            "openai",
            "gpt-4",
            "key",
            Some(&workspace),
            "plain response",
            "prompt",
            Some("session-1"),
            "run-1",
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
            "run-1",
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
            "run-1",
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
    fn active_workspace_resolution_covers_none_invalid_and_valid() {
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

        let valid = resolve_active_workspace_context(ActiveWorkspaceProbe::Valid { selection });
        assert_eq!(valid.workspace_id.as_deref(), Some("ws1"));
        let tool_context = valid.tool_context.unwrap();
        assert_eq!(tool_context.workspace_path, PathBuf::from("/tmp/workspace"));
        assert!(tool_context.source_edit_allowed);
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
                run_id: "run-success".to_string(),
                provider: "openai".to_string(),
                prompt: "write code".to_string(),
                model: "gpt-test".to_string(),
                variant: Some("reasoning-high".to_string()),
                session_id: Some("session-1".to_string()),
                invoked_skill: None,
                agent_id: None,
                delegated_agent: None,
                delegate_provider: "openai".to_string(),
                delegate_model: "gpt-4o-mini".to_string(),
                delegate_api_key: "api-key".to_string(),
            },
        )
        .await;
        let completed_run_id = match result {
            Ok(run_id) => run_id,
            Err(err) => panic!("turn failed: {} {}", err.code, err.message),
        };
        assert_eq!(completed_run_id, "run-success");

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
                source_edit_allowed: true,
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
        assert_eq!(adapters.done_responses.lock().unwrap().len(), 1);
        assert_eq!(
            adapters.done_responses.lock().unwrap()[0].0,
            "run-success".to_string()
        );
        assert_eq!(
            adapters.done_responses.lock().unwrap()[0].1,
            "```rust\nfn main() {}\n```".to_string()
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
        assert_eq!(
            verifications[0].workspace.workspace_path,
            PathBuf::from("/tmp/workspace")
        );
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
                run_id: "run-variant-fallback".to_string(),
                provider: "openai".to_string(),
                prompt: "hello".to_string(),
                model: "gpt-5.2".to_string(),
                variant: Some("missing-variant".to_string()),
                session_id: Some("session-1".to_string()),
                invoked_skill: None,
                agent_id: None,
                delegated_agent: None,
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
                run_id: "run-source-edit".to_string(),
                provider: "openai".to_string(),
                prompt: "edit it".to_string(),
                model: "gpt-test".to_string(),
                variant: None,
                session_id: Some("session-1".to_string()),
                invoked_skill: None,
                agent_id: None,
                delegated_agent: None,
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
                run_id: "run-unsuccessful-edit".to_string(),
                provider: "openai".to_string(),
                prompt: "edit it".to_string(),
                model: "gpt-test".to_string(),
                variant: None,
                session_id: Some("session-1".to_string()),
                invoked_skill: None,
                agent_id: None,
                delegated_agent: None,
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
    async fn streaming_turn_failure_persists_controlled_stop_with_failed_prompt() {
        let adapters = FakeSessionTurnAdapters::with_stream_result(Err(LlmError::ControlledStop(
            "Agent stopped".to_string(),
        )));
        let prior_history = vec![user_message("before"), assistant_message("answer")];
        let prior_history_json = serde_json::to_string(&prior_history).unwrap();
        *adapters.persisted_snapshot.lock().unwrap() = Some(SessionSnapshot {
            display_transcript: r#"[{"role":"user","content":"hi"}]"#.to_string(),
            model_history: Some(prior_history_json),
        });

        let err = run_streaming_turn(
            adapters.deps(),
            StreamingTurnRequest {
                run_id: "run-controlled-stop".to_string(),
                provider: "openai".to_string(),
                prompt: "hi".to_string(),
                model: "gpt-test".to_string(),
                variant: None,
                session_id: Some("session-1".to_string()),
                invoked_skill: None,
                agent_id: None,
                delegated_agent: None,
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
        assert_eq!(adapters.emitted_errors.lock().unwrap().len(), 1);
        assert_eq!(
            adapters.emitted_errors.lock().unwrap()[0].1,
            "CONTROLLED_STOP".to_string()
        );
        assert!(adapters.done_responses.lock().unwrap().is_empty());
        assert!(adapters.verifications.lock().unwrap().is_empty());

        let persisted_turns = adapters.persisted_turns.lock().unwrap();
        assert_eq!(persisted_turns.len(), 1);
        assert_eq!(persisted_turns[0].session_id, "session-1");
        let model_history: Vec<Message> =
            serde_json::from_str(persisted_turns[0].model_history.as_deref().unwrap()).unwrap();
        assert_eq!(
            model_history,
            vec![
                user_message("before"),
                assistant_message("answer"),
                user_message("hi"),
            ]
        );
        let display: serde_json::Value =
            serde_json::from_str(&persisted_turns[0].display_transcript).unwrap();
        assert_eq!(display[0], json!({ "role": "user", "content": "hi" }));
        assert_eq!(display[1]["content"], "Agent stopped");
        assert_eq!(display[1]["error"], false);
        assert_eq!(display[1]["controlled_stop"], true);
        let stored_histories = adapters.stored_histories.lock().unwrap();
        assert_eq!(stored_histories.len(), 1);
        assert_eq!(stored_histories[0].0, "session-1");
        assert_eq!(stored_histories[0].1.last(), Some(&user_message("hi")));
        assert!(adapters.ingests.lock().unwrap().is_empty());
    }

    #[test]
    fn production_memory_adapter_is_a_noop() {
        let memory = NoOpMemory;
        assert_eq!(
            memory
                .recall(MemoryRecallRequest {
                    prompt: "write code".to_string(),
                    session_id: Some("session-1".to_string()),
                    workspace_id: Some("workspace-1".to_string()),
                })
                .unwrap(),
            None
        );
        memory.schedule_ingest(MemoryIngestJobRequest {
            display_transcript: "[]".to_string(),
            session_id: "session-1".to_string(),
            workspace_id: Some("workspace-1".to_string()),
        });
    }

    fn successful_history_turn(prompt: &str, reply: &str) -> StreamCompletionResult {
        StreamCompletionResult {
            full_response: reply.to_string(),
            history: Some(vec![user_message(prompt), assistant_message(reply)]),
            source_edit_succeeded: false,
            prompt_tokens: 4,
            response_tokens: 2,
            tool_calls: 0,
        }
    }

    #[tokio::test]
    async fn unknown_delegated_agent_injects_failure_note_and_proceeds() {
        let adapters = FakeSessionTurnAdapters::with_stream_result(Ok(successful_history_turn(
            "original prompt",
            "handled",
        )));
        let mut request = streaming_request("run-delegation", "original prompt", Some("session-1"));
        request.delegated_agent = Some(DelegatedAgentRequest {
            name: "missing".to_string(),
            task: "do the thing".to_string(),
        });

        let result = run_streaming_turn(adapters.deps(), request).await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        let streams = adapters.stream_requests.lock().unwrap();
        assert!(streams[0].prompt.contains("Delegated report from @missing"));
        assert!(streams[0].prompt.contains("agent not found"));
        assert!(streams[0].prompt.contains("original prompt"));
    }

    fn streaming_request(
        run_id: &str,
        prompt: &str,
        session_id: Option<&str>,
    ) -> StreamingTurnRequest {
        StreamingTurnRequest {
            run_id: run_id.to_string(),
            provider: "openai".to_string(),
            prompt: prompt.to_string(),
            model: "gpt-test".to_string(),
            variant: None,
            session_id: session_id.map(str::to_string),
            invoked_skill: None,
            agent_id: None,
            delegated_agent: None,
            delegate_provider: "openai".to_string(),
            delegate_model: "gpt-4o-mini".to_string(),
            delegate_api_key: "api-key".to_string(),
        }
    }

    #[tokio::test]
    async fn streaming_turn_recalls_memory_from_user_prompt_before_stream() {
        let adapters = FakeSessionTurnAdapters::with_stream_result(Ok(successful_history_turn(
            "write code",
            "```rust\nfn main() {}\n```",
        )));

        let result = run_streaming_turn(
            adapters.deps(),
            streaming_request("run-recall", "write code", Some("session-1")),
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        let recalls = adapters.recall_requests.lock().unwrap();
        assert_eq!(recalls.len(), 1);
        assert_eq!(recalls[0].prompt, "write code");
        assert_eq!(recalls[0].session_id.as_deref(), Some("session-1"));
        assert_eq!(recalls[0].workspace_id.as_deref(), Some("workspace-1"));
        assert!(adapters.stream_requests.lock().unwrap().len() == 1);
    }

    #[tokio::test]
    async fn empty_memory_recall_injects_no_section_and_still_streams() {
        let adapters = FakeSessionTurnAdapters::with_stream_result(Ok(successful_history_turn(
            "write code",
            "```rust\nfn main() {}\n```",
        )));

        let result = run_streaming_turn(
            adapters.deps(),
            streaming_request("run-empty-recall", "write code", Some("session-1")),
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        assert_eq!(adapters.recall_requests.lock().unwrap().len(), 1);
        assert!(adapters.stream_requests.lock().unwrap()[0]
            .memory_section
            .is_none());
        assert_eq!(adapters.done_responses.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn recalled_memory_is_a_harness_profile_sibling_not_appended_to_the_user_prompt() {
        let mut adapters = FakeSessionTurnAdapters::with_stream_result(Ok(
            successful_history_turn("write code", "```rust\nfn main() {}\n```"),
        ));
        adapters.recalled_section = Some("## Memory\nprefer bun".to_string());

        let result = run_streaming_turn(
            adapters.deps(),
            streaming_request("run-memory-section", "write code", Some("session-1")),
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        let stream_request = &adapters.stream_requests.lock().unwrap()[0];
        assert_eq!(stream_request.prompt, "write code");
        assert_eq!(
            stream_request.memory_section.as_deref(),
            Some("## Memory\nprefer bun")
        );
    }

    #[tokio::test]
    async fn memory_recall_error_is_ignored_and_the_turn_still_streams() {
        let mut adapters = FakeSessionTurnAdapters::with_stream_result(Ok(
            successful_history_turn("write code", "```rust\nfn main() {}\n```"),
        ));
        adapters.recall_error = Some("store unavailable".to_string());

        let result = run_streaming_turn(
            adapters.deps(),
            streaming_request("run-recall-error", "write code", Some("session-1")),
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        assert_eq!(adapters.recall_requests.lock().unwrap().len(), 1);
        assert!(adapters.stream_requests.lock().unwrap()[0]
            .memory_section
            .is_none());
        assert_eq!(adapters.done_responses.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn skill_match_keys_off_the_user_prompt_not_recalled_memory() {
        let mut adapters = FakeSessionTurnAdapters::with_stream_result(Ok(
            successful_history_turn("debug the failing stream", "ok"),
        ));
        adapters.skills = vec![skill(
            "diagnose",
            "debug broken behavior and failing tests",
            "Reproduce first.",
        )];
        adapters.recalled_section = Some("## Memory\nprefer bun not npm".to_string());

        let result = run_streaming_turn(
            adapters.deps(),
            streaming_request(
                "run-skill-match",
                "debug the failing stream",
                Some("session-1"),
            ),
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        let stream_request = &adapters.stream_requests.lock().unwrap()[0];
        assert_eq!(stream_request.prompt, "debug the failing stream");
        assert!(stream_request
            .matched_skills_section
            .as_deref()
            .unwrap()
            .contains("diagnose"));
        assert_eq!(
            stream_request.memory_section.as_deref(),
            Some("## Memory\nprefer bun not npm")
        );
        assert_eq!(
            adapters.recall_requests.lock().unwrap()[0].prompt,
            "debug the failing stream"
        );
    }

    #[tokio::test]
    async fn invoked_skill_still_keys_off_the_user_prompt_not_recalled_memory() {
        let mut adapters = FakeSessionTurnAdapters::with_stream_result(Ok(
            successful_history_turn("write regression tests", "ok"),
        ));
        adapters.skills = vec![skill(
            "tdd",
            "test driven development workflow",
            "Use red-green-refactor.",
        )];
        adapters.recalled_section = Some("## Memory\nprefer bun not npm".to_string());

        let mut request = streaming_request(
            "run-invoked-skill",
            "/tdd write regression tests",
            Some("session-1"),
        );
        request.invoked_skill = Some(InvokedSkillRequest {
            name: "tdd".to_string(),
            args: Some("write regression tests".to_string()),
        });

        let result = run_streaming_turn(adapters.deps(), request).await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        let stream_request = &adapters.stream_requests.lock().unwrap()[0];
        assert_eq!(stream_request.prompt, "write regression tests");
        assert!(stream_request.matched_skills_section.is_none());
        assert!(stream_request
            .invoked_skill_section
            .as_deref()
            .unwrap()
            .contains("Use red-green-refactor."));
        assert_eq!(
            adapters.recall_requests.lock().unwrap()[0].prompt,
            "/tdd write regression tests"
        );
    }

    #[tokio::test]
    async fn successful_persist_schedules_memory_ingest_once() {
        let adapters = FakeSessionTurnAdapters::with_stream_result(Ok(successful_history_turn(
            "write code",
            "```rust\nfn main() {}\n```",
        )));

        let result = run_streaming_turn(
            adapters.deps(),
            streaming_request("run-ingest", "write code", Some("session-1")),
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        let ingests = adapters.ingests.lock().unwrap();
        assert_eq!(ingests.len(), 1);
        assert_eq!(ingests[0].session_id, "session-1");
        assert_eq!(ingests[0].workspace_id.as_deref(), Some("workspace-1"));
        let display: serde_json::Value =
            serde_json::from_str(&ingests[0].display_transcript).unwrap();
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
    }

    #[tokio::test]
    async fn persist_failure_does_not_schedule_memory_ingest() {
        let adapters = FakeSessionTurnAdapters::with_stream_result(Ok(successful_history_turn(
            "write code",
            "```rust\nfn main() {}\n```",
        )));
        *adapters.persist_result.lock().unwrap() = Err("disk full".to_string());

        let result = run_streaming_turn(
            adapters.deps(),
            streaming_request("run-persist-fail", "write code", Some("session-1")),
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        assert_eq!(adapters.recall_requests.lock().unwrap().len(), 1);
        assert!(adapters.ingests.lock().unwrap().is_empty());
        assert!(adapters.statuses.lock().unwrap().is_empty());
        assert!(adapters.persisted_turns.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn provider_failure_does_not_schedule_memory_ingest() {
        let adapters = FakeSessionTurnAdapters::with_stream_result(Err(LlmError::ProviderError(
            "boom".to_string(),
        )));
        let model_history =
            serde_json::to_string(&vec![user_message("prior"), assistant_message("answer")])
                .unwrap();
        *adapters.persisted_snapshot.lock().unwrap() = Some(SessionSnapshot {
            display_transcript: r#"[{"role":"user","content":"hi"}]"#.to_string(),
            model_history: Some(model_history),
        });

        let err = run_streaming_turn(
            adapters.deps(),
            streaming_request("run-provider-fail", "hi", Some("session-1")),
        )
        .await
        .unwrap_err();

        assert_eq!(err.code, "PROVIDER_ERROR");
        assert!(adapters.ingests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn streaming_turn_recalls_and_schedules_memory_ingest() {
        let adapters = FakeSessionTurnAdapters::with_stream_result(Ok(successful_history_turn(
            "inspect",
            "looks fine",
        )));

        let result = run_streaming_turn(
            adapters.deps(),
            streaming_request("run-memory-ingest", "inspect", Some("session-1")),
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        let recalls = adapters.recall_requests.lock().unwrap();
        assert_eq!(recalls.len(), 1);
        assert_eq!(recalls[0].prompt, "inspect");
        drop(recalls);

        let ingests = adapters.ingests.lock().unwrap();
        assert_eq!(ingests.len(), 1);
        assert_eq!(ingests[0].session_id, "session-1");
        assert_eq!(ingests[0].workspace_id.as_deref(), Some("workspace-1"));
    }

    #[tokio::test]
    async fn unscoped_session_recalls_memory_without_a_workspace_id() {
        let mut adapters = FakeSessionTurnAdapters::with_stream_result(Ok(
            successful_history_turn("hello", "hi there"),
        ));
        adapters.active_workspace = None;

        let result = run_streaming_turn(
            adapters.deps(),
            streaming_request("run-unscoped", "hello", Some("session-1")),
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        let recalls = adapters.recall_requests.lock().unwrap();
        assert_eq!(recalls.len(), 1);
        assert_eq!(recalls[0].prompt, "hello");
        assert_eq!(recalls[0].session_id.as_deref(), Some("session-1"));
        assert!(recalls[0].workspace_id.is_none());
        drop(recalls);

        let ingests = adapters.ingests.lock().unwrap();
        assert_eq!(ingests.len(), 1);
        assert_eq!(ingests[0].session_id, "session-1");
        assert!(ingests[0].workspace_id.is_none());
    }

    #[tokio::test]
    async fn successful_stream_without_model_history_does_not_schedule_memory_ingest() {
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
            streaming_request("run-no-history", "hello", Some("session-1")),
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        assert_eq!(adapters.recall_requests.lock().unwrap().len(), 1);
        assert!(adapters.ingests.lock().unwrap().is_empty());
        assert!(adapters.persisted_turns.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn local_only_turn_recalls_memory_without_scheduling_ingest() {
        let adapters = FakeSessionTurnAdapters::with_stream_result(Ok(successful_history_turn(
            "hello", "hi there",
        )));

        let result = run_streaming_turn(
            adapters.deps(),
            streaming_request("run-local-only", "hello", None),
        )
        .await;
        if let Err(err) = result {
            panic!("turn failed: {} {}", err.code, err.message);
        }

        let recalls = adapters.recall_requests.lock().unwrap();
        assert_eq!(recalls.len(), 1);
        assert_eq!(recalls[0].prompt, "hello");
        assert!(recalls[0].session_id.is_none());
        assert_eq!(recalls[0].workspace_id.as_deref(), Some("workspace-1"));
        drop(recalls);

        assert_eq!(adapters.stream_requests.lock().unwrap().len(), 1);
        assert!(adapters.ingests.lock().unwrap().is_empty());
        assert_eq!(adapters.done_responses.lock().unwrap().len(), 1);
    }

    struct TestPersistence {
        saved_turns: std::sync::Mutex<Vec<SavedTurn>>,
        drafts_activated: std::sync::Mutex<Vec<String>>,
    }

    impl TestPersistence {
        fn new() -> Self {
            Self {
                saved_turns: std::sync::Mutex::new(Vec::new()),
                drafts_activated: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl TurnPersistenceAdapter for TestPersistence {
        fn activate_draft_if_needed(&self, session_id: &str) -> Result<(), String> {
            self.drafts_activated
                .lock()
                .unwrap()
                .push(session_id.to_string());
            Ok(())
        }

        fn save_turn_success(
            &self,
            session_id: &str,
            user_prompt: &str,
            assistant_reply: &str,
            model_history: Option<&str>,
        ) -> Result<(), String> {
            self.saved_turns.lock().unwrap().push((
                session_id.to_string(),
                user_prompt.to_string(),
                assistant_reply.to_string(),
                model_history.map(|s| s.to_string()),
            ));
            Ok(())
        }

        fn save_turn_failure(
            &self,
            _session_id: &str,
            _user_prompt: &str,
            _error_message: &str,
        ) -> Result<(), String> {
            Ok(())
        }

        fn save_turn_stopped(
            &self,
            _session_id: &str,
            _user_prompt: &str,
            _stopped_reason: &str,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    struct TestEmitter {
        emitted: std::sync::Mutex<Vec<(String, String, SessionTurnEvent)>>,
    }

    impl TestEmitter {
        fn new() -> Self {
            Self {
                emitted: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl TurnEventEmitter for TestEmitter {
        fn emit_event(&self, session_id: &str, run_id: &str, event: &SessionTurnEvent) {
            self.emitted.lock().unwrap().push((
                session_id.to_string(),
                run_id.to_string(),
                event.clone(),
            ));
        }
    }

    struct TestLlm {
        response: String,
    }

    impl TurnLlmAdapter for TestLlm {
        fn stream_turn<'a>(
            &'a self,
            _prompt: &'a str,
            mut on_event: Box<dyn FnMut(SessionTurnEvent) + Send + 'a>,
        ) -> SessionTurnFuture<'a, Result<StreamCompletionResult, LlmError>> {
            on_event(SessionTurnEvent::TextToken(self.response.clone()));
            let result = StreamCompletionResult {
                full_response: self.response.clone(),
                history: Some(vec![user_message("Hi"), assistant_message(&self.response)]),
                source_edit_succeeded: false,
                prompt_tokens: 10,
                response_tokens: 15,
                tool_calls: 0,
            };
            Box::pin(async move { Ok(result) })
        }
    }

    #[tokio::test]
    async fn orchestrator_executes_successful_turn() {
        use std::sync::Arc;
        let persistence = Arc::new(TestPersistence::new());
        let emitter = Arc::new(TestEmitter::new());
        let llm = TestLlm {
            response: "Hello! How can I help?".to_string(),
        };

        let orchestrator = TurnOrchestrator::new(persistence.clone(), emitter.clone(), llm);
        let summary: TurnSummary = orchestrator
            .execute_turn("session-1", "run-1", "Hi")
            .await
            .unwrap();

        assert_eq!(summary.full_response, "Hello! How can I help?");
        assert_eq!(summary.prompt_tokens, 10);
        assert_eq!(summary.response_tokens, 15);
        assert_eq!(summary.tool_calls, 0);

        let saved = persistence.saved_turns.lock().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].0, "session-1");
        assert_eq!(saved[0].1, "Hi");
        assert_eq!(saved[0].2, "Hello! How can I help?");

        let emitted = emitter.emitted.lock().unwrap();
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].0, "session-1");
        assert_eq!(emitted[0].1, "run-1");
        assert!(matches!(
            &emitted[0].2,
            SessionTurnEvent::TextToken(t) if t == "Hello! How can I help?"
        ));
    }

    struct TestFailingLlm {
        err: LlmError,
    }

    impl TurnLlmAdapter for TestFailingLlm {
        fn stream_turn<'a>(
            &'a self,
            _prompt: &'a str,
            _on_event: Box<dyn FnMut(SessionTurnEvent) + Send + 'a>,
        ) -> SessionTurnFuture<'a, Result<StreamCompletionResult, LlmError>> {
            let err = match &self.err {
                LlmError::ControlledStop(r) => LlmError::ControlledStop(r.clone()),
                LlmError::ProviderError(e) => LlmError::ProviderError(e.clone()),
                _ => LlmError::ProviderError("failed".to_string()),
            };
            Box::pin(async move { Err(err) })
        }
    }

    #[tokio::test]
    async fn orchestrator_handles_controlled_stop() {
        use std::sync::Arc;
        let persistence = Arc::new(TestPersistence::new());
        let emitter = Arc::new(TestEmitter::new());
        let llm = TestFailingLlm {
            err: LlmError::ControlledStop("Loop threshold exceeded".to_string()),
        };

        let orchestrator = TurnOrchestrator::new(persistence.clone(), emitter.clone(), llm);
        let err = orchestrator
            .execute_turn("session-1", "run-1", "Hi")
            .await
            .unwrap_err();

        assert!(matches!(err, LlmError::ControlledStop(_)));

        let saved = persistence.saved_turns.lock().unwrap();
        assert!(saved.is_empty());

        let drafts = persistence.drafts_activated.lock().unwrap();
        assert_eq!(*drafts, vec!["session-1".to_string()]);
    }

    #[tokio::test]
    async fn orchestrator_activates_draft_session_on_turn_start() {
        use std::sync::Arc;
        let persistence = Arc::new(TestPersistence::new());
        let emitter = Arc::new(TestEmitter::new());
        let llm = TestLlm {
            response: "Activated!".to_string(),
        };

        let orchestrator = TurnOrchestrator::new(persistence.clone(), emitter.clone(), llm);
        orchestrator
            .execute_turn("draft-session-99", "run-99", "Hello")
            .await
            .unwrap();

        let drafts = persistence.drafts_activated.lock().unwrap();
        assert_eq!(*drafts, vec!["draft-session-99".to_string()]);
    }
}
