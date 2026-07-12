use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::agent::{HookAction, PromptHook, ToolCallHookAction};
use rig::client::CompletionClient;
use rig::completion::message::Message;
use rig::completion::{CompletionModel, Prompt, ToolDefinition};
use rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingChat};
use rig::tool::{Tool, ToolDyn};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::future::Future;
use std::hash::{Hash, Hasher};
#[cfg(test)]
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::time::timeout;

#[cfg(test)]
use crate::harness_profile::guards_for_role;
use crate::harness_profile::{
    resolve_harness_profile, AgentRole, HarnessProfileRequest, LoopGuardPolicy,
    MainHarnessMechanisms, WorkspaceToolContext,
};

/// Deterministic failure reasons that should not be retried automatically.
const DETERMINISTIC_FAILURE_REASONS: &[&str] = &[
    "blocked",
    "path_escape",
    "secret",
    "not_found",
    "invalid_query",
    "binary",
    "too_large",
    "oversized",
    "invalid_utf8",
    "invalid_range",
    "invalid_replacement",
    "no_match",
    "ambiguous_match",
    "no_op",
];

/// Detects consecutive identical tool calls and repeated deterministic failures.
#[derive(Debug)]
struct LoopDetector {
    last_call_hash: u64,
    consecutive_count: usize,
    last_failure_reason: String,
    failure_streak: usize,
    warn_threshold: usize,
    stop_threshold: usize,
}

impl LoopDetector {
    fn new(policy: LoopGuardPolicy) -> Self {
        Self {
            last_call_hash: 0,
            consecutive_count: 0,
            last_failure_reason: String::new(),
            failure_streak: 0,
            warn_threshold: policy.warning_threshold,
            stop_threshold: policy.stop_threshold,
        }
    }

    fn record_call(&mut self, tool_name: &str, args: &serde_json::Value) -> LoopStatus {
        let canonical = format!(
            "{}:{}",
            tool_name,
            crate::json_utils::canonical_json_string(args)
        );
        let mut hasher = DefaultHasher::new();
        canonical.hash(&mut hasher);
        let hash = hasher.finish();

        if hash == self.last_call_hash {
            self.consecutive_count += 1;
        } else {
            self.last_call_hash = hash;
            self.consecutive_count = 1;
        }

        if self.consecutive_count >= self.stop_threshold {
            LoopStatus::Stop
        } else if self.consecutive_count >= self.warn_threshold {
            LoopStatus::Warning(self.consecutive_count)
        } else {
            LoopStatus::Ok
        }
    }

    fn record_failure(&mut self, reason: &str) -> Option<LoopStatus> {
        if !DETERMINISTIC_FAILURE_REASONS.contains(&reason) {
            self.reset_failure_streak();
            return None;
        }

        if reason == self.last_failure_reason {
            self.failure_streak += 1;
        } else {
            self.last_failure_reason = reason.to_string();
            self.failure_streak = 1;
        }

        if self.failure_streak >= self.stop_threshold {
            Some(LoopStatus::Stop)
        } else if self.failure_streak >= self.warn_threshold {
            Some(LoopStatus::Warning(self.failure_streak))
        } else {
            Some(LoopStatus::Ok)
        }
    }

    #[allow(dead_code)]
    fn reset(&mut self) {
        self.last_call_hash = 0;
        self.consecutive_count = 0;
        self.reset_failure_streak();
    }

    fn reset_failure_streak(&mut self) {
        self.last_failure_reason.clear();
        self.failure_streak = 0;
    }
}

#[derive(Debug, PartialEq)]
enum LoopStatus {
    Ok,
    Warning(usize),
    Stop,
}

use crate::models::ModelRegistry;
use crate::provider_client::provider_client;
use crate::shell_tools::CommandApproval;
use crate::text_utils::truncate_text_bytes;
use crate::workspace_tools::{workspace_root_inventory, ExternalPathApproval};

const EXPLORATION_REPORT_BYTES_CAP: usize = 32 * 1024;
const EXPLORATION_AGENT_PROMPT: &str = r#"
You are the Gospel Exploration Agent.

Investigate the active workspace and return a concise markdown report with exactly these section headings:

## Summary
## Key Files
## Findings
## Constraints
## Suggested Next Reads
## Tools Used

Use broad workspace retrieval for orientation when it is available.
Verify important hits with live workspace tools (read_file, search_code) before making claims.
Use corpus tools for fast structure when available, and live workspace tools for source-of-truth verification.
Do not answer as the final user-facing assistant. Return findings only.
"#;

#[derive(Error, Debug)]
pub enum LlmError {
    #[error("API key not configured for provider")]
    ApiKeyMissing,
    #[error("provider error: {0}")]
    ProviderError(String),
    #[error("model {0} is not available")]
    #[allow(dead_code)]
    ModelUnavailable(String),
    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),
    #[error("controlled stop: {0}")]
    ControlledStop(String),
}

#[derive(Serialize, Clone)]
pub struct LlmErrorDto {
    pub code: String,
    pub message: String,
}

impl LlmError {
    pub fn to_dto(&self) -> LlmErrorDto {
        match self {
            LlmError::ApiKeyMissing => LlmErrorDto {
                code: "API_KEY_MISSING".to_string(),
                message: "API key not configured. Open Settings to add one.".to_string(),
            },
            LlmError::ProviderError(msg) => LlmErrorDto {
                code: "PROVIDER_ERROR".to_string(),
                message: format!("Completion failed: {}", msg),
            },
            LlmError::ModelUnavailable(model) => LlmErrorDto {
                code: "MODEL_UNAVAILABLE".to_string(),
                message: format!("Model {} is not available", model),
            },
            LlmError::UnsupportedProvider(provider) => LlmErrorDto {
                code: "UNSUPPORTED_PROVIDER".to_string(),
                message: format!("Provider {} is not supported", provider),
            },
            LlmError::ControlledStop(msg) => LlmErrorDto {
                code: "CONTROLLED_STOP".to_string(),
                message: msg.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StreamEvent {
    Text(String),
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

pub struct LlmService;

fn validate_api_key(provider: &str, api_key: &str) -> Result<(), LlmError> {
    if !ModelRegistry::is_oauth_provider(provider) && api_key.trim().is_empty() {
        return Err(LlmError::ApiKeyMissing);
    }

    Ok(())
}

impl LlmService {
    pub async fn completion(
        provider: &str,
        prompt: &str,
        model: &str,
        api_key: &str,
    ) -> Result<String, LlmError> {
        validate_api_key(provider, api_key)?;

        let response = provider_client!(
            provider,
            api_key,
            LlmError::ProviderError,
            LlmError::UnsupportedProvider,
            |client| {
                let agent = client.agent(model).build();
                agent
                    .prompt(prompt)
                    .await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
        );
        Ok(response)
    }
}

#[derive(Debug)]
pub struct StreamCompletionResult {
    pub full_response: String,
    pub history: Option<Vec<Message>>,
    pub source_edit_succeeded: bool,
    pub prompt_tokens: usize,
    pub response_tokens: usize,
    pub tool_calls: usize,
}

#[derive(Debug, Deserialize)]
struct DelegateExplorationArgs {
    task: String,
    context: Option<String>,
    expected_output: Option<String>,
}

#[derive(Debug, Serialize)]
struct DelegateExplorationOutput {
    success: bool,
    truncated: bool,
    report: String,
    summary: Option<String>,
    key_files: Vec<String>,
    findings: Vec<String>,
    constraints: Vec<String>,
    suggested_next_reads: Vec<String>,
    tools_used: Vec<String>,
    message: String,
    reason: Option<String>,
}

#[derive(Clone, Deserialize)]
struct DelegateExplorationTool {
    workspace: WorkspaceToolContext,
    provider: String,
    model: String,
    #[serde(default)]
    api_key: String,
}

impl std::fmt::Debug for DelegateExplorationTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DelegateExplorationTool")
            .field("workspace", &self.workspace)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl Serialize for DelegateExplorationTool {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("DelegateExplorationTool", 3)?;
        state.serialize_field("workspace", &self.workspace)?;
        state.serialize_field("provider", &self.provider)?;
        state.serialize_field("model", &self.model)?;
        state.end()
    }
}

impl Tool for DelegateExplorationTool {
    const NAME: &'static str = "delegate_exploration";

    type Error = LlmError;
    type Args = DelegateExplorationArgs;
    type Output = DelegateExplorationOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Delegate a broad workspace investigation to the Exploration Agent. Use this for multi-file or architectural investigations that need a focused report before answering the user.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "Required investigation task for the Exploration Agent."
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional extra context or constraints for the investigation."
                    },
                    "expected_output": {
                        "type": "string",
                        "description": "Optional guidance for what the report should emphasize."
                    }
                },
                "required": ["task"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        match run_exploration_agent(
            &self.provider,
            &self.model,
            &self.api_key,
            &self.workspace,
            &args,
        )
        .await
        {
            Ok(output) => Ok(output),
            Err(error) => Ok(DelegateExplorationOutput {
                success: false,
                truncated: false,
                report: String::new(),
                summary: None,
                key_files: vec![],
                findings: vec![],
                constraints: vec![],
                suggested_next_reads: vec![],
                tools_used: vec![],
                message: error.to_string(),
                reason: Some(match error {
                    LlmError::ProviderError(_) => "provider_error".to_string(),
                    LlmError::ApiKeyMissing => "api_key_missing".to_string(),
                    LlmError::ModelUnavailable(_) => "model_unavailable".to_string(),
                    LlmError::UnsupportedProvider(_) => "unsupported_provider".to_string(),
                    LlmError::ControlledStop(_) => "controlled_stop".to_string(),
                }),
            }),
        }
    }
}

#[derive(Clone)]
struct ExplorationHook {
    tools: Arc<Mutex<Vec<String>>>,
}

impl<M> PromptHook<M> for ExplorationHook
where
    M: CompletionModel,
{
    fn on_tool_call(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        _internal_call_id: &str,
        _args: &str,
    ) -> impl Future<Output = ToolCallHookAction> + Send {
        let tool_name = tool_name.to_string();
        let tools = self.tools.clone();
        async move {
            let mut guard = tools.lock().unwrap();
            if !guard.iter().any(|name| name == &tool_name) {
                guard.push(tool_name);
            }
            ToolCallHookAction::cont()
        }
    }

    async fn on_tool_result(
        &self,
        _tool_name: &str,
        _tool_call_id: Option<String>,
        _internal_call_id: &str,
        _args: &str,
        _result: &str,
    ) -> HookAction {
        HookAction::cont()
    }
}

#[derive(Debug, Default, Clone)]
struct ParsedDelegateSections {
    summary: Option<String>,
    key_files: Vec<String>,
    findings: Vec<String>,
    constraints: Vec<String>,
    suggested_next_reads: Vec<String>,
    tools_used: Vec<String>,
}

fn parse_delegation_sections(text: &str) -> ParsedDelegateSections {
    let mut section = "other".to_string();
    let mut sections: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    sections.insert("summary".to_string(), Vec::new());
    sections.insert("key_files".to_string(), Vec::new());
    sections.insert("findings".to_string(), Vec::new());
    sections.insert("constraints".to_string(), Vec::new());
    sections.insert("suggested_next_reads".to_string(), Vec::new());
    sections.insert("tools_used".to_string(), Vec::new());

    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("## ") {
            let heading = lower.trim_start_matches("## ").trim();
            match heading {
                "summary" => section = "summary".to_string(),
                "key files" => section = "key_files".to_string(),
                "findings" => section = "findings".to_string(),
                "constraints" => section = "constraints".to_string(),
                "suggested next reads" => section = "suggested_next_reads".to_string(),
                "tools used" => section = "tools_used".to_string(),
                _ => section = "other".to_string(),
            }
            continue;
        }

        if let Some(lines) = sections.get_mut(&section) {
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
        }
    }

    let summary = sections
        .remove("summary")
        .unwrap_or_default()
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let key_files = parse_structured_lines(sections.remove("key_files").unwrap_or_default());
    let findings = parse_structured_lines(sections.remove("findings").unwrap_or_default());
    let constraints = parse_structured_lines(sections.remove("constraints").unwrap_or_default());
    let suggested_next_reads =
        parse_structured_lines(sections.remove("suggested_next_reads").unwrap_or_default());
    let tools_used = parse_structured_lines(sections.remove("tools_used").unwrap_or_default());

    ParsedDelegateSections {
        summary: if summary.is_empty() {
            None
        } else {
            Some(summary.join("\n").trim().to_string())
        },
        key_files,
        findings,
        constraints,
        suggested_next_reads,
        tools_used,
    }
}

fn parse_structured_lines(lines: Vec<String>) -> Vec<String> {
    let mut parsed = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        let value = strip_list_bullet(trimmed);
        if !value.is_empty() {
            parsed.push(value.to_string());
            continue;
        }
        parsed.push(trimmed.to_string());
    }
    parsed
        .into_iter()
        .map(|value| value.trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn strip_list_bullet(line: &str) -> &str {
    if let Some(value) = line.strip_prefix("- ") {
        value
    } else if let Some(value) = line.strip_prefix("* ") {
        value
    } else if let Some(value) = line.strip_prefix("+ ") {
        value
    } else if let Some(dot_position) = line.find(". ") {
        if line[..dot_position]
            .chars()
            .all(|character| character.is_ascii_digit())
        {
            line[dot_position + 2..].trim()
        } else {
            line
        }
    } else if let Some(paren_position) = line.find(") ") {
        if line[..paren_position]
            .chars()
            .all(|character| character.is_ascii_digit())
        {
            line[paren_position + 2..].trim()
        } else {
            line
        }
    } else {
        line
    }
}

fn build_exploration_prompt(args: &DelegateExplorationArgs, workspace_context: &str) -> String {
    let mut sections = vec![format!("Task:\n{}", args.task.trim())];
    if !workspace_context.trim().is_empty() {
        sections.push(format!("Workspace root snapshot:\n{}", workspace_context));
    }
    if let Some(context) = args
        .context
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Context:\n{}", context));
    }
    if let Some(expected_output) = args
        .expected_output
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Expected output emphasis:\n{}", expected_output));
    }
    sections.join("\n\n")
}

async fn run_exploration_agent(
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: &WorkspaceToolContext,
    args: &DelegateExplorationArgs,
) -> Result<DelegateExplorationOutput, LlmError> {
    let hook = ExplorationHook {
        tools: Arc::new(Mutex::new(Vec::new())),
    };
    let tool_names = hook.tools.clone();
    let workspace_snapshot = workspace_root_inventory(&workspace.workspace_path, 40)
        .map(|inventory| format!("{}", inventory))
        .unwrap_or_else(|_| String::new());
    let profile = resolve_harness_profile(HarnessProfileRequest {
        role: AgentRole::Exploration,
        workspace: Some(workspace.clone()),
        role_guidance: Some(EXPLORATION_AGENT_PROMPT.to_string()),
        matched_skills_section: None,
        invoked_skill_section: None,
        main_mechanisms: None,
    })
    .map_err(|error| LlmError::ProviderError(error.to_string()))?;
    tracing::debug!(profile = ?profile.summary(), "resolved Exploration Harness Profile");
    let agent_preamble = profile.preamble.unwrap_or_default();
    let agent_tools = profile.tools;
    let agent_guards = profile.guards;
    let agent_deadline = agent_guards
        .deadline
        .expect("Exploration Harness Profiles always have a deadline");
    let prompt = build_exploration_prompt(&args, &workspace_snapshot);

    // Exploration agent uses tighter loop thresholds
    let _loop_detector = Arc::new(Mutex::new(LoopDetector::new(agent_guards.loop_guard)));

    macro_rules! exploration_from_client {
        ($client:expr, $model:expr) => {{
            let builder = $client
                .agent($model)
                .preamble(&agent_preamble)
                .default_max_turns(agent_guards.max_turns)
                .tools(agent_tools);

            let agent = builder.build();
            let future = agent
                .prompt(prompt)
                .with_hook(hook.clone())
                .extended_details();
            let response = timeout(agent_deadline, future)
                .await
                .map_err(|_| {
                    LlmError::ControlledStop(format!(
                        "Exploration Agent timed out after {} seconds",
                        agent_deadline.as_secs()
                    ))
                })?
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;

            let tools_used = tool_names.lock().unwrap().clone();
            let sections = parse_delegation_sections(&response.output);
            let (report, truncated) =
                truncate_text_bytes(&response.output, EXPLORATION_REPORT_BYTES_CAP);
            DelegateExplorationOutput {
                success: true,
                truncated,
                report,
                summary: sections.summary,
                key_files: sections.key_files,
                findings: sections.findings,
                constraints: sections.constraints,
                suggested_next_reads: sections.suggested_next_reads,
                tools_used: if sections.tools_used.is_empty() {
                    tools_used
                } else {
                    sections.tools_used
                },
                message: "Exploration completed.".to_string(),
                reason: None,
            }
        }};
    }

    provider_client!(
        provider,
        api_key,
        LlmError::ProviderError,
        LlmError::UnsupportedProvider,
        |client| { Ok(exploration_from_client!(client, model)) }
    )
}

pub async fn stream_completion<F>(
    provider: &str,
    prompt: &str,
    model: &str,
    variant: Option<&str>,
    api_key: &str,
    delegate_provider: &str,
    delegate_model: &str,
    delegate_api_key: &str,
    workspace: Option<WorkspaceToolContext>,
    external_path_approval: Option<Arc<dyn ExternalPathApproval>>,
    command_approval: Option<Arc<dyn CommandApproval>>,
    chat_history: Vec<Message>,
    matched_skills_section: Option<String>,
    invoked_skill_section: Option<String>,
    skill_script_tool: Option<crate::skills::RunSkillScriptTool>,
    mut on_event: F,
) -> Result<StreamCompletionResult, LlmError>
where
    F: FnMut(StreamEvent),
{
    validate_api_key(provider, api_key)?;
    let variant_resolution = ModelRegistry::resolve_model_variant(provider, model, variant);
    if let Some(warning) = variant_resolution.warning.clone() {
        on_event(StreamEvent::ModelVariantWarning {
            kind: warning.kind,
            provider: provider.to_string(),
            model: model.to_string(),
            variant: warning.variant,
            message: warning.message,
        });
    }

    let mut full_response = String::new();
    let mut tool_calls = 0usize;
    let mut captured_history: Option<Vec<Message>> = None;
    let mut source_edit_succeeded = false;
    let prompt_tokens = estimate_tokens(prompt)
        + chat_history
            .iter()
            .map(estimate_message_tokens)
            .sum::<usize>();
    let mut additional_tools: Vec<Box<dyn ToolDyn>> = Vec::new();
    if let Some(workspace_context) = workspace.as_ref() {
        additional_tools.push(Box::new(DelegateExplorationTool {
            workspace: workspace_context.clone(),
            provider: delegate_provider.to_string(),
            model: delegate_model.to_string(),
            api_key: delegate_api_key.to_string(),
        }));
    }
    if let Some(tool) = skill_script_tool {
        additional_tools.push(Box::new(tool));
    }
    let profile = resolve_harness_profile(HarnessProfileRequest {
        role: AgentRole::Main,
        workspace,
        role_guidance: None,
        matched_skills_section,
        invoked_skill_section,
        main_mechanisms: Some(MainHarnessMechanisms {
            provider: provider.to_string(),
            model: model.to_string(),
            api_key: api_key.to_string(),
            external_path_approval,
            command_approval,
            additional_tools,
        }),
    })
    .map_err(|error| LlmError::ProviderError(error.to_string()))?;
    tracing::debug!(profile = ?profile.summary(), "resolved Main Harness Profile");
    let profile_preamble = profile.preamble;
    let profile_tools = profile.tools;
    let profile_guards = profile.guards;

    macro_rules! stream_from_client {
        ($client:expr, $model:expr) => {{
            let mut builder = $client
                .agent($model)
                .default_max_turns(profile_guards.max_turns);
            if let Some(additional_params) = variant_resolution.additional_params.clone() {
                builder = builder.additional_params(additional_params);
            }
            let builder = if let Some(preamble) = profile_preamble.as_ref() {
                builder.preamble(preamble)
            } else {
                builder
            };
            let agent = builder.tools(profile_tools).build();
            let request = agent
                .stream_chat(prompt, chat_history)
                .multi_turn(profile_guards.max_turns);
            let mut stream = request.await;

            let mut tool_name_by_id: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            let mut loop_detector = LoopDetector::new(profile_guards.loop_guard);

            while let Some(item) = stream.next().await {
                match item {
                    Ok(item) => match item {
                        MultiTurnStreamItem::StreamAssistantItem(
                            StreamedAssistantContent::Text(text),
                        ) => {
                            full_response.push_str(&text.text);
                            on_event(StreamEvent::Text(text.text.clone()));
                        }
                        MultiTurnStreamItem::StreamAssistantItem(
                            StreamedAssistantContent::ToolCall {
                                tool_call,
                                internal_call_id,
                            },
                        ) => {
                            // Check for identical tool call loops
                            match loop_detector.record_call(
                                &tool_call.function.name,
                                &tool_call.function.arguments,
                            ) {
                                LoopStatus::Ok => {}
                                LoopStatus::Warning(count) => {
                                    on_event(StreamEvent::LoopWarning {
                                        count,
                                        tool_name: tool_call.function.name.clone(),
                                    });
                                }
                                LoopStatus::Stop => {
                                    let msg = format!(
                                        "Agent stopped: repeated identical tool call '{}' detected {} times. This usually indicates the agent is stuck in a loop.",
                                        tool_call.function.name, loop_detector.consecutive_count
                                    );
                                    on_event(StreamEvent::LoopStopped {
                                        count: loop_detector.consecutive_count,
                                        tool_name: tool_call.function.name.clone(),
                                        message: msg.clone(),
                                    });
                                    return Err(LlmError::ControlledStop(msg));
                                }
                            }

                            tool_name_by_id
                                .insert(internal_call_id.clone(), tool_call.function.name.clone());
                            on_event(StreamEvent::ToolCall {
                                id: internal_call_id,
                                name: tool_call.function.name.clone(),
                                arguments: tool_call.function.arguments.clone(),
                            });
                            tool_calls += 1;
                        }
                        MultiTurnStreamItem::StreamUserItem(StreamedUserContent::ToolResult {
                            tool_result,
                            internal_call_id,
                        }) => {
                            let result_summary = tool_result
                                .content
                                .iter()
                                .filter_map(|content| match content {
                                    rig::completion::message::ToolResultContent::Text(text) => {
                                        Some(text.text.clone())
                                    }
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            let tool_name = tool_name_by_id
                                .get(&internal_call_id)
                                .cloned()
                                .unwrap_or_else(|| tool_result.id.clone());

                            if tool_name == "source_edit"
                                && source_edit_result_changed(&result_summary)
                            {
                                source_edit_succeeded = true;
                            }

                            // Check for repeated deterministic failures
                            if let Some((reason, status)) =
                                record_tool_result_failure(&mut loop_detector, &result_summary)
                            {
                                match status {
                                    LoopStatus::Ok => {}
                                    LoopStatus::Warning(count) => {
                                        on_event(StreamEvent::LoopWarning {
                                            count,
                                            tool_name: tool_name.clone(),
                                        });
                                    }
                                    LoopStatus::Stop => {
                                        let msg = format!(
                                            "Agent stopped: repeated deterministic failure '{}' detected {} times. The agent appears stuck trying the same failing approach.",
                                            reason, loop_detector.failure_streak
                                        );
                                        on_event(StreamEvent::LoopStopped {
                                            count: loop_detector.failure_streak,
                                            tool_name: tool_name.clone(),
                                            message: msg.clone(),
                                        });
                                        return Err(LlmError::ControlledStop(msg));
                                    }
                                }
                            }

                            on_event(StreamEvent::ToolResult {
                                id: internal_call_id,
                                name: tool_name,
                                result: result_summary,
                            });
                        }
                        MultiTurnStreamItem::FinalResponse(final_response) => {
                            full_response = final_response.response().to_owned();
                            captured_history =
                                final_response.history().map(|history| history.to_vec());
                            break;
                        }
                        _ => {}
                    },
                    Err(error) => {
                        return Err(LlmError::ProviderError(error.to_string()));
                    }
                }
            }
        }};
    }

    provider_client!(
        provider,
        api_key,
        LlmError::ProviderError,
        LlmError::UnsupportedProvider,
        |client| {
            stream_from_client!(client, model);
        }
    );
    let response_tokens = estimate_tokens(&full_response);

    Ok(StreamCompletionResult {
        full_response,
        history: captured_history,
        source_edit_succeeded,
        prompt_tokens,
        response_tokens,
        tool_calls,
    })
}

fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count()
}

fn estimate_message_tokens(message: &Message) -> usize {
    match message {
        Message::User { content } => estimate_tokens(
            &content
                .iter()
                .filter_map(|item| match item {
                    rig::completion::message::UserContent::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" "),
        ),
        Message::Assistant { content, .. } => estimate_tokens(
            &content
                .iter()
                .filter_map(|item| match item {
                    rig::completion::message::AssistantContent::Text(text) => {
                        Some(text.text.as_str())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" "),
        ),
        Message::System { content } => estimate_tokens(content),
    }
}

fn source_edit_result_changed(result_summary: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(result_summary)
        .ok()
        .and_then(|value| {
            let success = value.get("success").and_then(|item| item.as_bool())?;
            let changed = value.get("changed").and_then(|item| item.as_bool())?;
            Some(success && changed)
        })
        .unwrap_or(false)
}

fn record_tool_result_failure(
    loop_detector: &mut LoopDetector,
    result_summary: &str,
) -> Option<(String, LoopStatus)> {
    match serde_json::from_str::<serde_json::Value>(result_summary) {
        Ok(parsed) => {
            let success = parsed
                .get("success")
                .and_then(|item| item.as_bool())
                .unwrap_or(true);

            if success {
                loop_detector.reset_failure_streak();
                return None;
            }

            let Some(reason) = parsed.get("reason").and_then(|item| item.as_str()) else {
                loop_detector.reset_failure_streak();
                return None;
            };

            loop_detector
                .record_failure(reason)
                .map(|status| (reason.to_string(), status))
        }
        Err(_) => {
            loop_detector.reset_failure_streak();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delegate_tool_for_test() -> DelegateExplorationTool {
        DelegateExplorationTool {
            workspace: WorkspaceToolContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: false,
                session_mode: crate::session_mode::SESSION_MODE_BUILD.to_string(),
            },
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            api_key: "secret-api-key".to_string(),
        }
    }

    #[tokio::test]
    async fn completion_rejects_blank_api_key() {
        let error = LlmService::completion("openai", "hello", "gpt-4o-mini", "  ")
            .await
            .unwrap_err();

        assert!(matches!(error, LlmError::ApiKeyMissing));
    }

    #[tokio::test]
    async fn stream_completion_rejects_blank_api_key() {
        let mut events = vec![];
        let error = stream_completion(
            "openai",
            "hello",
            "gpt-4o-mini",
            None,
            "",
            "openai",
            "gpt-4o-mini",
            "",
            None,
            None,
            None,
            vec![],
            None,
            None,
            None,
            |event| events.push(event),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, LlmError::ApiKeyMissing));
        assert!(events.is_empty());
    }

    #[test]
    fn validate_api_key_allows_blank_key_for_chatgpt() {
        let result = validate_api_key("chatgpt", "   ");

        assert!(result.is_ok());
    }

    #[test]
    fn validate_api_key_allows_blank_key_for_github_copilot() {
        let result = validate_api_key("github_copilot", "   ");

        assert!(result.is_ok());
    }

    #[test]
    fn validate_api_key_rejects_blank_key_for_non_oauth_provider() {
        let result = validate_api_key("openai", "");

        assert!(matches!(result, Err(LlmError::ApiKeyMissing)));
    }

    #[test]
    fn delegate_exploration_tool_debug_redacts_api_key() {
        let debug = format!("{:?}", delegate_tool_for_test());

        assert!(debug.contains("api_key"));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-api-key"));
    }

    #[test]
    fn delegate_exploration_tool_serialization_omits_api_key() {
        let json = serde_json::to_string(&delegate_tool_for_test()).unwrap();

        assert!(json.contains("provider"));
        assert!(!json.contains("api_key"));
        assert!(!json.contains("secret-api-key"));
    }

    #[test]
    fn delegate_exploration_tool_deserializes_redacted_serialization() {
        let value = serde_json::to_value(delegate_tool_for_test()).unwrap();
        let tool: DelegateExplorationTool = serde_json::from_value(value).unwrap();

        assert_eq!(tool.api_key, "");
        assert_eq!(tool.provider, "openai");
    }

    #[test]
    fn build_exploration_prompt_includes_optional_context() {
        let prompt = build_exploration_prompt(
            &DelegateExplorationArgs {
                task: "Trace startup flow".to_string(),
                context: Some("Focus on Tauri setup".to_string()),
                expected_output: Some("Highlight risky assumptions".to_string()),
            },
            "",
        );

        assert!(prompt.contains("Task:"));
        assert!(prompt.contains("Focus on Tauri setup"));
        assert!(prompt.contains("Highlight risky assumptions"));
    }

    #[test]
    fn source_edit_result_changed_requires_success_and_changed() {
        assert!(source_edit_result_changed(
            r#"{"success":true,"changed":true,"path":"src/lib.rs"}"#
        ));
        assert!(!source_edit_result_changed(
            r#"{"success":true,"changed":false,"reason":"no_op"}"#
        ));
        assert!(!source_edit_result_changed(
            r#"{"success":false,"changed":false,"reason":"blocked"}"#
        ));
        assert!(!source_edit_result_changed("not json"));
    }

    #[test]
    fn tool_result_failure_is_only_recorded_when_success_is_false() {
        let mut detector = LoopDetector::new(guards_for_role(AgentRole::Verification).loop_guard);

        let successful =
            record_tool_result_failure(&mut detector, r#"{"success":true,"reason":"blocked"}"#);
        assert_eq!(successful, None);
        assert_eq!(detector.failure_streak, 0);

        let failed =
            record_tool_result_failure(&mut detector, r#"{"success":false,"reason":"blocked"}"#);
        assert_eq!(failed, Some(("blocked".to_string(), LoopStatus::Ok)));
        assert_eq!(detector.failure_streak, 1);
    }

    #[test]
    fn successful_no_match_result_resets_failure_streak() {
        let mut detector = LoopDetector::new(guards_for_role(AgentRole::Verification).loop_guard);

        record_tool_result_failure(&mut detector, r#"{"success":false,"reason":"blocked"}"#);
        assert_eq!(detector.failure_streak, 1);

        let status =
            record_tool_result_failure(&mut detector, r#"{"success":true,"reason":"no_match"}"#);
        assert_eq!(status, None);
        assert_eq!(detector.failure_streak, 0);

        record_tool_result_failure(&mut detector, r#"{"success":true,"reason":"no_match"}"#);
        assert_eq!(detector.failure_streak, 0);
    }

    #[test]
    fn repeated_failed_blocked_results_warn_then_stop() {
        let mut detector = LoopDetector::new(guards_for_role(AgentRole::Verification).loop_guard);
        let result = r#"{"success":false,"reason":"blocked"}"#;

        assert_eq!(
            record_tool_result_failure(&mut detector, result),
            Some(("blocked".to_string(), LoopStatus::Ok))
        );
        assert_eq!(
            record_tool_result_failure(&mut detector, result),
            Some(("blocked".to_string(), LoopStatus::Warning(2)))
        );
        assert_eq!(
            record_tool_result_failure(&mut detector, result),
            Some(("blocked".to_string(), LoopStatus::Stop))
        );
    }

    #[test]
    fn failed_result_without_reason_resets_deterministic_failure_streak() {
        let mut detector = LoopDetector::new(guards_for_role(AgentRole::Verification).loop_guard);

        record_tool_result_failure(&mut detector, r#"{"success":false,"reason":"blocked"}"#);
        assert_eq!(detector.failure_streak, 1);

        assert_eq!(
            record_tool_result_failure(&mut detector, r#"{"success":false,"error":"denied"}"#),
            None
        );
        assert_eq!(detector.failure_streak, 0);

        assert_eq!(
            record_tool_result_failure(&mut detector, r#"{"success":false,"reason":"blocked"}"#),
            Some(("blocked".to_string(), LoopStatus::Ok))
        );
    }
}
