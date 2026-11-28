//! ACP turn execution: prompt streaming, tool-call tracing, cancellation, and
//! session fork for Factory Droid.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use agent_client_protocol::v1::schema::{
    CancelNotification, ContentBlock, RequestPermissionOutcome, RequestPermissionRequest,
    SessionId, SessionNotification, SessionUpdate, ToolCallStatus, ToolKind,
};
use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::{Agent, Error, ErrorCode, UntypedMessage};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::client::{agent_config, connect, initialize_request, with_timeout, AcpSessionHandler};
use super::dialect::{
    session_scope_params, set_config_option_params, DialectSessionSetup,
};
use super::workspace::{
    build_approval_request, is_read_only_execute, resolve_workspace_path, slice_lines,
    MAX_FS_READ_BYTES, MAX_FS_WRITE_BYTES,
};
use crate::approval_broker::{
    ApprovalBroker, ApprovalDecision, ApprovalOutcome,
};
use crate::harness_profile::ActiveWorkspaceContext;
use crate::llm::{LlmError, ReasoningPayload, ReasoningPhase, StreamCompletionResult, StreamEvent};
use crate::text_utils::truncate_text_bytes;

const SESSION_OP_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_TOOL_RESULT_DISPLAY_BYTES: usize = 24 * 1024;

/// Keys in edit-like tool raw inputs that carry file contents. Gospel redacts
/// raw replacement snippets from UI tool-call payloads and Trace Log
/// arguments, matching the `source_edit` contract.
const CONTENT_INPUT_KEYS: &[&str] = &[
    "content",
    "new_text",
    "old_text",
    "new_string",
    "old_string",
    "replacement",
];

// ============================================================================
// Active ACP Run Cancellation Registry
// ============================================================================

pub struct ActiveAcpRun {
    connection: StdMutex<Option<ConnectionTo<Agent>>>,
    acp_session_id: StdMutex<Option<String>>,
    abort: StdMutex<Option<tokio::task::AbortHandle>>,
    cancelled: AtomicBool,
}

#[derive(Default)]
pub struct AcpRunRegistry {
    runs: StdMutex<HashMap<String, Arc<ActiveAcpRun>>>,
}

impl AcpRunRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn register(&self, gospel_session_id: &str) -> Arc<ActiveAcpRun> {
        let handle = Arc::new(ActiveAcpRun {
            connection: StdMutex::new(None),
            acp_session_id: StdMutex::new(None),
            abort: StdMutex::new(None),
            cancelled: AtomicBool::new(false),
        });
        self.runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(gospel_session_id.to_string(), Arc::clone(&handle));
        handle
    }

    fn unregister(&self, gospel_session_id: &str, handle: &Arc<ActiveAcpRun>) {
        let mut guard = self.runs.lock().unwrap_or_else(|p| p.into_inner());
        if guard
            .get(gospel_session_id)
            .is_some_and(|existing| Arc::ptr_eq(existing, handle))
        {
            guard.remove(gospel_session_id);
        }
    }

    pub fn cancel_session(&self, gospel_session_id: &str) {
        let entry = self
            .runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(gospel_session_id);
        let Some(handle) = entry else {
            return;
        };
        handle.cancelled.store(true, Ordering::SeqCst);
        let connection = handle
            .connection
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let acp_sid = handle
            .acp_session_id
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let abort = handle
            .abort
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        tokio::spawn(async move {
            if let (Some(connection), Some(sid)) = (connection, acp_sid) {
                let _ = connection.send_notification(CancelNotification::new(SessionId::new(sid)));
                // Give the agent a brief chance to stop the turn gracefully
                // before tearing the process down.
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            if let Some(abort) = abort {
                abort.abort();
            }
        });
    }
}

// ============================================================================
// Turn trace state: typed session updates -> Gospel stream events + blocks
// ============================================================================

#[derive(Default)]
struct TurnTraceState {
    accepting_updates: bool,
    full_response: String,
    blocks: Vec<Value>,
    tool_calls: usize,
    source_edit_succeeded: bool,
    active_reasoning_id: Option<String>,
    reasoning_text: String,
    reasoning_seq: usize,
    tool_names_by_id: HashMap<String, String>,
}

fn redact_tool_input(raw: &Value) -> Value {
    let Some(obj) = raw.as_object() else {
        return raw.clone();
    };
    let mut redacted = obj.clone();
    for key in CONTENT_INPUT_KEYS {
        if redacted.contains_key(*key) {
            redacted.insert((*key).to_string(), json!("[redacted]"));
        }
    }
    Value::Object(redacted)
}

fn content_block_text(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text(text) => text.text.clone(),
        _ => String::new(),
    }
}

fn normalize_acp_tool_name(title: &str, kind: Option<ToolKind>, name: Option<&str>) -> String {
    match kind {
        Some(ToolKind::Read) => "read_file".to_string(),
        Some(ToolKind::Edit) => "edit_file".to_string(),
        Some(ToolKind::Execute) => "bash".to_string(),
        Some(ToolKind::Search) => "search_code".to_string(),
        Some(ToolKind::Delete) | Some(ToolKind::Move) => "edit_file".to_string(),
        _ => {
            if let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) {
                return name.to_string();
            }
            let lower = title.trim().to_ascii_lowercase();
            if lower.contains("read") {
                "read_file".to_string()
            } else if lower.contains("edit") || lower.contains("patch") {
                "edit_file".to_string()
            } else if lower.contains("write") || lower.contains("create") {
                "write_file".to_string()
            } else if lower.contains("grep") || lower.contains("search") {
                "search_code".to_string()
            } else if lower.contains("glob") || lower.contains("find") {
                "find_files".to_string()
            } else if lower.contains("bash") || lower.contains("exec") || lower.contains("run ") {
                "bash".to_string()
            } else {
                let clean = title.trim();
                if clean.is_empty() {
                    "tool".to_string()
                } else {
                    clean.to_string()
                }
            }
        }
    }
}

impl TurnTraceState {
    fn close_reasoning(&mut self, on_event: &mut (dyn FnMut(StreamEvent) + Send)) {
        if let Some(id) = self.active_reasoning_id.take() {
            let text = std::mem::take(&mut self.reasoning_text);
            if !text.is_empty() {
                on_event(StreamEvent::Reasoning(ReasoningPayload {
                    id,
                    text,
                    phase: ReasoningPhase::Complete,
                }));
            }
        }
    }

    fn append_text(&mut self, chunk: &str, on_event: &mut (dyn FnMut(StreamEvent) + Send)) {
        if chunk.is_empty() {
            return;
        }
        self.close_reasoning(on_event);
        self.full_response.push_str(chunk);
        on_event(StreamEvent::Text(chunk.to_string()));

        if let Some(last) = self.blocks.last_mut() {
            if last.get("kind").and_then(Value::as_str) == Some("text") {
                let prev = last.get("text").and_then(Value::as_str).unwrap_or("");
                let combined = format!("{prev}{chunk}");
                last["text"] = json!(combined);
                return;
            }
        }
        let id = format!("text-{}", self.blocks.len());
        self.blocks.push(json!({
            "kind": "text",
            "id": id,
            "text": chunk,
        }));
    }

    fn append_thought(&mut self, chunk: &str, on_event: &mut (dyn FnMut(StreamEvent) + Send)) {
        if chunk.is_empty() {
            return;
        }
        let id = match &self.active_reasoning_id {
            Some(id) => id.clone(),
            None => {
                self.reasoning_seq += 1;
                let next = format!("reasoning-{}", self.reasoning_seq);
                self.active_reasoning_id = Some(next.clone());
                next
            }
        };
        self.reasoning_text.push_str(chunk);
        on_event(StreamEvent::Reasoning(ReasoningPayload {
            id,
            text: chunk.to_string(),
            phase: ReasoningPhase::Delta,
        }));
    }

    #[allow(clippy::too_many_arguments)]
    fn start_tool(
        &mut self,
        id: String,
        title: String,
        kind: Option<ToolKind>,
        name: Option<&str>,
        raw_input: Option<Value>,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) {
        self.close_reasoning(on_event);
        let id = if id.is_empty() {
            format!("acp-tool-{}", self.tool_calls + 1)
        } else {
            id
        };
        let normalized = normalize_acp_tool_name(&title, kind, name);
        let args = raw_input
            .as_ref()
            .map(redact_tool_input)
            .unwrap_or_else(|| json!({ "title": title }));

        if let Some(existing) = self
            .blocks
            .iter_mut()
            .find(|b| b.get("kind").and_then(Value::as_str) == Some("tool") && b.get("id").and_then(Value::as_str) == Some(id.as_str()))
        {
            existing["name"] = json!(normalized);
            existing["arguments"] = args.clone();
            return;
        }

        self.tool_calls += 1;
        self.tool_names_by_id.insert(id.clone(), normalized.clone());
        on_event(StreamEvent::ToolCall {
            id: id.clone(),
            name: normalized.clone(),
            arguments: args.clone(),
        });
        self.blocks.push(json!({
            "kind": "tool",
            "id": id,
            "name": normalized,
            "arguments": args,
            "status": "calling",
        }));
    }

    #[allow(clippy::too_many_arguments)]
    fn update_tool(
        &mut self,
        id: String,
        title: Option<&str>,
        kind: Option<ToolKind>,
        name: Option<&str>,
        status: Option<ToolCallStatus>,
        raw_input: Option<Value>,
        raw_output: Option<Value>,
        content_text: Option<String>,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) {
        if id.is_empty() {
            return;
        }
        let existing_name = self.tool_names_by_id.get(&id).cloned().unwrap_or_else(|| {
            normalize_acp_tool_name(title.unwrap_or("tool"), kind, name)
        });

        let is_terminal = matches!(
            status,
            Some(ToolCallStatus::Completed | ToolCallStatus::Failed)
        );
        let ui_status = match status {
            Some(ToolCallStatus::Completed | ToolCallStatus::Failed) => "completed",
            _ => "calling",
        };

        let result_text = raw_output
            .as_ref()
            .and_then(|v| serde_json::to_string_pretty(v).ok())
            .or(content_text);

        if let Some(block) = self.blocks.iter_mut().find(|b| {
            b.get("kind").and_then(Value::as_str) == Some("tool")
                && b.get("id").and_then(Value::as_str) == Some(id.as_str())
        }) {
            if let Some(input) = raw_input {
                block["arguments"] = redact_tool_input(&input);
            }
            if is_terminal || result_text.is_some() {
                block["status"] = json!(ui_status);
            }
            if let Some(ref text) = result_text {
                let (capped, _) = truncate_text_bytes(text, MAX_TOOL_RESULT_DISPLAY_BYTES);
                block["result"] = json!(capped);
            }
        } else if is_terminal {
            self.tool_calls += 1;
            let args = raw_input
                .as_ref()
                .map(redact_tool_input)
                .unwrap_or_else(|| json!({}));
            on_event(StreamEvent::ToolCall {
                id: id.clone(),
                name: existing_name.clone(),
                arguments: args.clone(),
            });
            let mut block = json!({
                "kind": "tool",
                "id": id,
                "name": existing_name,
                "arguments": args,
                "status": ui_status,
            });
            if let Some(ref text) = result_text {
                let (capped, _) = truncate_text_bytes(text, MAX_TOOL_RESULT_DISPLAY_BYTES);
                block["result"] = json!(capped);
            }
            self.blocks.push(block);
        }

        if is_terminal {
            if (matches!(kind, Some(ToolKind::Edit | ToolKind::Delete | ToolKind::Move))
                || matches!(existing_name.as_str(), "edit_file" | "write_file" | "source_edit"))
                && status == Some(ToolCallStatus::Completed)
            {
                self.source_edit_succeeded = true;
            }
            let emitted_result = result_text.unwrap_or_else(|| "Completed".to_string());
            let (capped, _) = truncate_text_bytes(&emitted_result, MAX_TOOL_RESULT_DISPLAY_BYTES);
            on_event(StreamEvent::ToolResult {
                id,
                name: existing_name,
                result: capped,
            });
        }
    }
}

// ============================================================================
// Turn handler: notifications + permission/fs callbacks
// ============================================================================

struct AcpTurnHandler {
    workspace: Option<PathBuf>,
    source_edit_allowed: bool,
    approval_broker: Option<Arc<ApprovalBroker>>,
    run_id: Option<String>,
    state: Mutex<TurnTraceState>,
    event_tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
}

fn permission_option_kind_rank(kind: &agent_client_protocol::v1::schema::PermissionOptionKind) -> &'static str {
    use agent_client_protocol::v1::schema::PermissionOptionKind;
    match kind {
        PermissionOptionKind::AllowAlways => "allow_always",
        PermissionOptionKind::AllowOnce => "allow_once",
        PermissionOptionKind::RejectAlways => "reject_always",
        PermissionOptionKind::RejectOnce => "reject_once",
        _ => "other",
    }
}

fn select_permission_option(
    options: &[agent_client_protocol::v1::schema::PermissionOption],
    preferred: &[&str],
) -> Option<RequestPermissionOutcome> {
    use agent_client_protocol::v1::schema::{RequestPermissionOutcome, SelectedPermissionOutcome};
    for kind in preferred {
        if let Some(opt) = options
            .iter()
            .find(|o| permission_option_kind_rank(&o.kind) == *kind)
        {
            return Some(RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                opt.option_id.clone(),
            )));
        }
    }
    None
}

fn map_approval_to_acp_outcome(
    outcome: ApprovalOutcome,
    decision: Option<ApprovalDecision>,
    options: &[agent_client_protocol::v1::schema::PermissionOption],
) -> RequestPermissionOutcome {
    let selected = match (outcome, decision) {
        (ApprovalOutcome::Approved, Some(ApprovalDecision::AlwaysAllow)) => {
            select_permission_option(options, &["allow_always", "allow_once"])
        }
        (ApprovalOutcome::Approved | ApprovalOutcome::AutoApproved, _) => {
            select_permission_option(options, &["allow_once", "allow_always"])
        }
        (ApprovalOutcome::Denied, Some(ApprovalDecision::AlwaysDeny)) => {
            select_permission_option(options, &["reject_always", "reject_once"])
        }
        (ApprovalOutcome::Denied | ApprovalOutcome::AutoDenied, _) => {
            select_permission_option(options, &["reject_once", "reject_always"])
        }
        (ApprovalOutcome::TimedOut, _) => None,
    };
    selected.unwrap_or(RequestPermissionOutcome::Cancelled)
}

#[async_trait]
impl AcpSessionHandler for AcpTurnHandler {
    async fn on_session_update(&self, notification: &SessionNotification) {
        let mut state = self.state.lock().await;
        if !state.accepting_updates {
            return;
        }
        let mut emit = |ev: StreamEvent| {
            let _ = self.event_tx.send(ev);
        };
        match &notification.update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                state.append_text(&content_block_text(&chunk.content), &mut emit);
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                state.append_thought(&content_block_text(&chunk.content), &mut emit);
            }
            SessionUpdate::ToolCall(tool_call) => {
                state.start_tool(
                    tool_call.tool_call_id.to_string(),
                    tool_call.title.clone(),
                    Some(tool_call.kind),
                    tool_call.name.as_deref(),
                    tool_call.raw_input.clone(),
                    &mut emit,
                );
            }
            SessionUpdate::ToolCallUpdate(update) => {
                let fields = &update.fields;
                let content_text = fields
                    .content
                    .as_ref()
                    .map(|contents| {
                        contents
                            .iter()
                            .filter_map(|content| match content {
                                agent_client_protocol::v1::schema::ToolCallContent::Content(c) => {
                                    Some(content_block_text(&c.content))
                                }
                                agent_client_protocol::v1::schema::ToolCallContent::Diff(diff) => {
                                    Some(format!("--- {}\n+++ {}", diff.path.display(), diff.path.display()))
                                }
                                _ => None,
                            })
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .filter(|s| !s.is_empty());
                if fields.kind == Some(ToolKind::Edit) && fields.status == Some(ToolCallStatus::Completed) {
                    state.source_edit_succeeded = true;
                }
                state.update_tool(
                    update.tool_call_id.to_string(),
                    fields.title.as_deref(),
                    fields.kind,
                    fields.name.as_deref(),
                    fields.status,
                    fields.raw_input.clone(),
                    fields.raw_output.clone(),
                    content_text,
                    &mut emit,
                );
            }
            _ => {}
        }
    }

    async fn request_permission(
        &self,
        request: &RequestPermissionRequest,
    ) -> RequestPermissionOutcome {
        let Some(broker) = self.approval_broker.as_ref() else {
            return RequestPermissionOutcome::Cancelled;
        };

        // Read-only shell commands run without approval in Gospel's own shell
        // tools; mirror that instead of surfacing a prompt.
        if is_read_only_execute(request, self.workspace.as_deref()) {
            if let Some(outcome) = select_permission_option(&request.options, &["allow_once", "allow_always"]) {
                return outcome;
            }
        }

        let approval = build_approval_request(request, self.workspace.as_deref(), self.run_id.as_deref());
        let (outcome, decision) = broker.request_with_decision(approval).await;
        map_approval_to_acp_outcome(outcome, decision, &request.options)
    }

    async fn read_text_file(
        &self,
        request: &agent_client_protocol::v1::schema::ReadTextFileRequest,
    ) -> Result<String, String> {
        let Some(root) = self.workspace.as_deref() else {
            return Err("No active workspace is open".to_string());
        };
        let resolved = resolve_workspace_path(root, &request.path, false)?;
        let bytes = tokio::fs::read(&resolved)
            .await
            .map_err(|e| format!("Failed to read {}: {e}", request.path.display()))?;
        if bytes.len() > MAX_FS_READ_BYTES {
            return Err(format!("File {} exceeds {MAX_FS_READ_BYTES} bytes", request.path.display()));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| format!("File {} is not valid UTF-8", request.path.display()))?;
        Ok(slice_lines(&text, request.line, request.limit))
    }

    async fn write_text_file(
        &self,
        request: &agent_client_protocol::v1::schema::WriteTextFileRequest,
    ) -> Result<(), String> {
        if !self.source_edit_allowed {
            return Err("Workspace writes are disabled for this run".to_string());
        }
        let Some(root) = self.workspace.as_deref() else {
            return Err("No active workspace is open".to_string());
        };
        if request.content.len() > MAX_FS_WRITE_BYTES {
            return Err(format!(
                "Write exceeds {MAX_FS_WRITE_BYTES} bytes for {}",
                request.path.display()
            ));
        }
        let resolved = resolve_workspace_path(root, &request.path, true)?;
        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create parent directory: {e}"))?;
        }
        tokio::fs::write(&resolved, request.content.as_bytes())
            .await
            .map_err(|e| format!("Failed to write {}: {e}", request.path.display()))?;
        self.state.lock().await.source_edit_succeeded = true;
        Ok(())
    }
}

// ============================================================================
// Turn execution
// ============================================================================

pub struct AcpTurnRequest<'a> {
    pub prompt: &'a str,
    pub model: &'a str,
    pub variant: Option<&'a str>,
    pub api_key: &'a str,
    pub workspace: Option<&'a ActiveWorkspaceContext>,
    pub existing_acp_session_id: Option<&'a str>,
    pub fallback_context_summary: Option<&'a str>,
    pub matched_skills_section: Option<&'a str>,
    pub invoked_skill_section: Option<&'a str>,
    pub memory_section: Option<&'a str>,
    pub gospel_session_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub approval_broker: Option<Arc<ApprovalBroker>>,
    pub run_registry: Option<Arc<AcpRunRegistry>>,
}

/// Composes the ACP user message from Harness context sections and the user
/// prompt. NOTE: ACP has no system-prompt channel that Droid advertises, so
/// Harness Profile context travels inside the user message as delimited
/// sections. See ADR-0020 for the external-agent exception to the preamble
/// contract.
fn compose_acp_prompt(
    invoked_skill_section: Option<&str>,
    matched_skills_section: Option<&str>,
    memory_section: Option<&str>,
    seed_context: Option<&str>,
    user_prompt: &str,
) -> String {
    let mut sections = Vec::new();
    if let Some(invoked) = invoked_skill_section.filter(|s| !s.trim().is_empty()) {
        sections.push(invoked.trim().to_string());
    }
    if let Some(matched) = matched_skills_section.filter(|s| !s.trim().is_empty()) {
        sections.push(matched.trim().to_string());
    }
    if let Some(memory) = memory_section.filter(|s| !s.trim().is_empty()) {
        sections.push(memory.trim().to_string());
    }
    if let Some(ctx) = seed_context.filter(|s| !s.trim().is_empty()) {
        sections.push(format!("## Previous Conversation Context\n\n{ctx}"));
    }
    sections.push(user_prompt.to_string());
    sections.join("\n\n---\n\n")
}

fn variant_to_droid_effort(variant: Option<&str>) -> Option<&str> {
    let raw = variant.map(str::trim).filter(|v| !v.is_empty())?;
    let stripped = raw.strip_prefix("reasoning-").unwrap_or(raw);
    match stripped {
        "extra-high" | "extra_high" => Some("xhigh"),
        other => Some(other),
    }
}

fn map_acp_error(err: &Error, stage: &str) -> LlmError {
    if err.code == ErrorCode::AUTH_REQUIRED {
        super::droid::set_cached_authenticated(Some(false));
        LlmError::ProviderError(format!("401 Unauthorized: {}", err.message))
    } else {
        LlmError::ProviderError(format!("Factory Droid ACP {stage} failed: {err}"))
    }
}

struct TurnOutcome {
    acp_session_id: String,
    stop_reason: String,
    prompt_tokens: u64,
    response_tokens: u64,
}

async fn apply_session_config(
    connection: &ConnectionTo<Agent>,
    session_id: &str,
    model: &str,
    effort: Option<&str>,
    setup: &DialectSessionSetup,
) {
    // Route every mutation through Gospel's approval bridge: `normal` is
    // Droid's "Auto (Off)" level, so mutating and destructive actions arrive
    // as session/request_permission calls. Gospel's broker applies the
    // user-facing Ask/Auto policy uniformly from there.
    if let Some(autonomy) = setup.config_option("autonomy_level") {
        if autonomy.current_value.as_str() != Some("normal")
            && autonomy.all_choices().iter().any(|c| c.value == "normal")
        {
            let params = set_config_option_params(session_id, &autonomy.id, "normal");
            if let Ok(msg) = UntypedMessage::new("session/set_config_option", &params) {
                let _ = with_timeout(
                    "session/set_config_option",
                    SESSION_OP_TIMEOUT,
                    connection.send_request(msg).block_task(),
                )
                .await;
            }
        }
    }

    if !model.trim().is_empty() && model != "auto" {
        if let Some(model_opt) = setup.config_option("model") {
            if model_opt.current_value.as_str() != Some(model)
                && model_opt.all_choices().iter().any(|c| c.value == model)
            {
                let params = set_config_option_params(session_id, &model_opt.id, model);
                if let Ok(msg) = UntypedMessage::new("session/set_config_option", &params) {
                    let _ = with_timeout(
                        "session/set_config_option",
                        SESSION_OP_TIMEOUT,
                        connection.send_request(msg).block_task(),
                    )
                    .await;
                }
            }
        }
    }

    if let Some(effort_value) = effort {
        if let Some(effort_opt) = setup
            .config_option("thought_level")
            .or_else(|| setup.config_option("reasoning_effort"))
        {
            if effort_opt.current_value.as_str() != Some(effort_value)
                && effort_opt.all_choices().iter().any(|c| c.value == effort_value)
            {
                let params = set_config_option_params(session_id, &effort_opt.id, effort_value);
                if let Ok(msg) = UntypedMessage::new("session/set_config_option", &params) {
                    let _ = with_timeout(
                        "session/set_config_option",
                        SESSION_OP_TIMEOUT,
                        connection.send_request(msg).block_task(),
                    )
                    .await;
                }
            }
        }
    }
}

pub async fn run_acp_turn<F>(
    request: AcpTurnRequest<'_>,
    mut on_event: F,
) -> Result<StreamCompletionResult, LlmError>
where
    F: FnMut(StreamEvent) + Send,
{
    let binary = super::droid::resolve_droid_binary().ok_or_else(|| {
        LlmError::ProviderError(
            "Factory Droid CLI not found. Install droid or set GOSPEL_DROID_PATH.".to_string(),
        )
    })?;

    let workspace_path = request.workspace.map(|w| w.workspace_path.clone());
    let source_edit_allowed = request
        .workspace
        .map(|w| w.source_edit_allowed)
        .unwrap_or(false);
    let cwd_path = workspace_path
        .clone()
        .unwrap_or_else(std::env::temp_dir);
    let cwd_str = cwd_path.to_string_lossy().to_string();

    let effort = variant_to_droid_effort(request.variant);
    let mut args: Vec<String> = vec![
        "exec".to_string(),
        "--output-format".to_string(),
        "acp".to_string(),
        "--cwd".to_string(),
        cwd_str.clone(),
    ];
    if !request.model.trim().is_empty() {
        args.push("-m".to_string());
        args.push(request.model.to_string());
    }
    if let Some(level) = effort {
        args.push("-r".to_string());
        args.push(level.to_string());
    }

    let api_key_override = super::droid::effective_api_key(Some(request.api_key));
    let mut envs = Vec::new();
    if let Some(ref key) = api_key_override {
        envs.push(("FACTORY_API_KEY".to_string(), key.clone()));
    }

    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handler = Arc::new(AcpTurnHandler {
        workspace: workspace_path.clone(),
        source_edit_allowed,
        approval_broker: request.approval_broker.clone(),
        run_id: request.run_id.map(str::to_string),
        state: Mutex::new(TurnTraceState::default()),
        event_tx,
    });

    let run_handle = request
        .gospel_session_id
        .zip(request.run_registry.as_ref())
        .map(|(sid, registry)| (sid.to_string(), Arc::clone(registry), registry.register(sid)));

    struct RunGuard {
        run_handle: Option<(String, Arc<AcpRunRegistry>, Arc<ActiveAcpRun>)>,
    }
    impl Drop for RunGuard {
        fn drop(&mut self) {
            if let Some((sid, registry, handle)) = self.run_handle.take() {
                registry.unregister(&sid, &handle);
            }
        }
    }
    let _guard = RunGuard {
        run_handle: run_handle.clone(),
    };

    let existing_sid = request.existing_acp_session_id.map(str::to_string);
    let seed_context_owned = request.fallback_context_summary.map(str::to_string);
    let invoked_owned = request.invoked_skill_section.map(str::to_string);
    let matched_owned = request.matched_skills_section.map(str::to_string);
    let memory_owned = request.memory_section.map(str::to_string);
    let user_prompt_owned = request.prompt.to_string();
    let model_owned = request.model.to_string();
    let effort_owned = effort.map(str::to_string);
    let handler_for_closure = Arc::clone(&handler);
    let handler_for_events = Arc::clone(&handler);
    let run_handle_for_closure = run_handle.clone();

    let connection_task = tokio::spawn(connect(
        agent_config(&binary, &args, &envs),
        handler,
        None,
        move |connection| {
            let handler = handler_for_closure;
            let run_handle = run_handle_for_closure;
            async move {
                if let Some((_, _, ref handle)) = run_handle {
                    *handle
                        .connection
                        .lock()
                        .unwrap_or_else(|p| p.into_inner()) = Some(connection.clone());
                }

                let init = with_timeout(
                    "initialize",
                    SESSION_OP_TIMEOUT,
                    connection
                        .send_request(initialize_request(workspace_path.is_some(), source_edit_allowed))
                        .block_task(),
                )
                .await?;

                let load_session_supported = init.agent_capabilities.load_session;

                let mut used_fallback_context = false;
                let (acp_session_id, setup) = if let (Some(existing), true) =
                    (existing_sid.clone(), load_session_supported)
                {
                    let params = session_scope_params(Some(&existing), &cwd_str);
                    let load_msg = UntypedMessage::new("session/load", &params)
                        .map_err(|e| Error::new(ErrorCode::INTERNAL_ERROR, e.to_string()))?;
                    match with_timeout(
                        "session/load",
                        SESSION_OP_TIMEOUT,
                        connection.send_request(load_msg).block_task(),
                    )
                    .await
                    {
                        Ok(value) => {
                            let mut setup = DialectSessionSetup::parse(&value);
                            if setup.session_id.is_none() {
                                setup.session_id = Some(existing.clone());
                            }
                            (setup.session_id.clone().unwrap_or(existing), setup)
                        }
                        Err(err) if err.code == ErrorCode::AUTH_REQUIRED => {
                            return Err(err);
                        }
                        Err(err) => {
                            tracing::warn!(
                                "ACP session/load failed for {existing} ({err}); starting a fresh session"
                            );
                            used_fallback_context = true;
                            let new_params = session_scope_params(None, &cwd_str);
                            let new_msg = UntypedMessage::new("session/new", &new_params)
                                .map_err(|e| Error::new(ErrorCode::INTERNAL_ERROR, e.to_string()))?;
                            let value = with_timeout(
                                "session/new",
                                SESSION_OP_TIMEOUT,
                                connection.send_request(new_msg).block_task(),
                            )
                            .await?;
                            let setup = DialectSessionSetup::parse(&value);
                            let sid = setup.session_id.clone().ok_or_else(|| {
                                Error::new(ErrorCode::INTERNAL_ERROR, "session/new returned no session id")
                            })?;
                            (sid, setup)
                        }
                    }
                } else {
                    used_fallback_context = existing_sid.is_none() && seed_context_owned.is_some();
                    let new_params = session_scope_params(None, &cwd_str);
                    let new_msg = UntypedMessage::new("session/new", &new_params)
                        .map_err(|e| Error::new(ErrorCode::INTERNAL_ERROR, e.to_string()))?;
                    let value = with_timeout(
                        "session/new",
                        SESSION_OP_TIMEOUT,
                        connection.send_request(new_msg).block_task(),
                    )
                    .await?;
                    let setup = DialectSessionSetup::parse(&value);
                    let sid = setup.session_id.clone().ok_or_else(|| {
                        Error::new(ErrorCode::INTERNAL_ERROR, "session/new returned no session id")
                    })?;
                    (sid, setup)
                };

                if let Some((_, _, ref handle)) = run_handle {
                    *handle
                        .acp_session_id
                        .lock()
                        .unwrap_or_else(|p| p.into_inner()) = Some(acp_session_id.clone());
                }

                apply_session_config(
                    &connection,
                    &acp_session_id,
                    &model_owned,
                    effort_owned.as_deref(),
                    &setup,
                )
                .await;

                let full_prompt = compose_acp_prompt(
                    invoked_owned.as_deref(),
                    matched_owned.as_deref(),
                    memory_owned.as_deref(),
                    if used_fallback_context {
                        seed_context_owned.as_deref()
                    } else {
                        None
                    },
                    &user_prompt_owned,
                );

                handler.state.lock().await.accepting_updates = true;

                let prompt_params = json!({
                    "sessionId": acp_session_id,
                    "prompt": [{ "type": "text", "text": full_prompt }],
                });
                let prompt_msg = UntypedMessage::new("session/prompt", &prompt_params)
                    .map_err(|e| Error::new(ErrorCode::INTERNAL_ERROR, e.to_string()))?;
                let prompt_value = connection
                    .send_request(prompt_msg)
                    .block_task()
                    .await?;
                let stop_reason = prompt_value
                    .get("stopReason")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let usage = prompt_value.get("usage").cloned();

                handler.state.lock().await.accepting_updates = false;

                if init.agent_capabilities.session_capabilities.close.is_some() {
                    let close_params = json!({ "sessionId": acp_session_id });
                    if let Ok(msg) = UntypedMessage::new("session/close", &close_params) {
                        let _ = with_timeout(
                            "session/close",
                            Duration::from_secs(3),
                            connection.send_request(msg).block_task(),
                        )
                        .await;
                    }
                }

                let usage_number = |keys: &[&str]| -> u64 {
                    usage
                        .as_ref()
                        .and_then(|u| {
                            keys.iter()
                                .find_map(|k| u.get(*k).and_then(Value::as_u64))
                        })
                        .unwrap_or(0)
                };

                Ok(TurnOutcome {
                    acp_session_id,
                    stop_reason,
                    prompt_tokens: usage_number(&["inputTokens", "input_tokens", "promptTokens", "prompt_tokens"]),
                    response_tokens: usage_number(&["outputTokens", "output_tokens", "responseTokens", "response_tokens"]),
                })
            }
        },
    ));

    if let Some((_, _, ref handle)) = run_handle {
        *handle
            .abort
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(connection_task.abort_handle());
    }

    let mut connection_task = connection_task;
    let outcome = loop {
        tokio::select! {
            res = &mut connection_task => {
                break res;
            }
            maybe_ev = event_rx.recv() => {
                match maybe_ev {
                    Some(ev) => on_event(ev),
                    None => {
                        // Event channel closed; keep waiting on the connection.
                    }
                }
            }
        }
    };

    // Drain any remaining events.
    while let Ok(ev) = event_rx.try_recv() {
        on_event(ev);
    }

    let cancelled = run_handle
        .as_ref()
        .map(|(_, _, handle)| handle.cancelled.load(Ordering::SeqCst))
        .unwrap_or(false);

    let outcome = match outcome {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(err)) => {
            if cancelled {
                return Err(LlmError::ControlledStop(
                    "Factory Droid ACP turn was cancelled.".to_string(),
                ));
            }
            return Err(map_acp_error(&err, "turn"));
        }
        Err(join_err) => {
            if cancelled || join_err.is_cancelled() {
                return Err(LlmError::ControlledStop(
                    "Factory Droid ACP turn was cancelled.".to_string(),
                ));
            }
            return Err(LlmError::ProviderError(format!(
                "Factory Droid ACP task failed: {join_err}"
            )));
        }
    };

    if outcome.stop_reason == "cancelled" {
        return Err(LlmError::ControlledStop(
            "Factory Droid ACP turn was cancelled.".to_string(),
        ));
    }

    let mut state = handler_for_events.state.lock().await;
    {
        let mut emit = |ev: StreamEvent| {
            let _ = handler_for_events.event_tx.send(ev);
        };
        state.close_reasoning(&mut emit);
    }
    while let Ok(ev) = event_rx.try_recv() {
        on_event(ev);
    }

    let full_response = std::mem::take(&mut state.full_response);
    let blocks = std::mem::take(&mut state.blocks);
    let tool_calls = state.tool_calls;
    let source_edit_succeeded = state.source_edit_succeeded;
    drop(state);

    Ok(StreamCompletionResult {
        full_response,
        history: None,
        source_edit_succeeded,
        prompt_tokens: outcome.prompt_tokens as usize,
        response_tokens: outcome.response_tokens as usize,
        tool_calls,
        acp_session_id: Some(outcome.acp_session_id),
        acp_turn_blocks: Some(blocks),
    })
}

pub async fn fork_acp_session(acp_session_id: &str, cwd: Option<&Path>) -> Option<String> {
    let binary = super::droid::resolve_droid_binary()?;
    if cfg!(test) && std::env::var_os("GOSPEL_DROID_PATH").is_none() {
        return None;
    }
    let cwd_path = cwd.map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
    let cwd_str = cwd_path.to_string_lossy().to_string();
    let args = vec![
        "exec".to_string(),
        "--output-format".to_string(),
        "acp".to_string(),
        "--cwd".to_string(),
        cwd_str.clone(),
    ];
    let session_id_owned = acp_session_id.to_string();
    let result = connect(
        agent_config(&binary, &args, &[]),
        Arc::new(super::client::NoOpAcpHandler),
        None,
        move |connection| {
            let cwd_str = cwd_str.clone();
            let session_id_owned = session_id_owned.clone();
            async move {
                let init = with_timeout(
                    "initialize",
                    SESSION_OP_TIMEOUT,
                    connection
                        .send_request(initialize_request(cwd.is_some(), true))
                        .block_task(),
                )
                .await?;

                let fork_supported = init
                    .agent_capabilities
                    .session_capabilities
                    .fork
                    .is_some();
                if !fork_supported {
                    return Ok(None);
                }

                let params = session_scope_params(Some(&session_id_owned), &cwd_str);
                let msg = UntypedMessage::new("session/fork", &params)
                    .map_err(|e| Error::new(ErrorCode::INTERNAL_ERROR, e.to_string()))?;
                let value = with_timeout(
                    "session/fork",
                    SESSION_OP_TIMEOUT,
                    connection.send_request(msg).block_task(),
                )
                .await?;
                let setup = DialectSessionSetup::parse(&value);
                Ok(setup.session_id.filter(|id| !id.is_empty()))
            }
        },
    )
    .await;

    match result {
        Ok(forked) => forked,
        Err(err) => {
            tracing::warn!("ACP session/fork failed for {acp_session_id}: {err}");
            None
        }
    }
}
