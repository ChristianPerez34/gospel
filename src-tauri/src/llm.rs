use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::agent::{HookAction, PromptHook, ToolCallHookAction};
use rig::client::CompletionClient;
use rig::completion::message::Message;
use rig::completion::{CompletionModel, Prompt, ToolDefinition};
use rig::providers::{anthropic, chatgpt, gemini, groq, mistral, openai};
use rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingChat};
use rig::tool::Tool;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::time::{timeout, Duration};

/// Role-specific thresholds for run guards.
const MAIN_AGENT_LOOP_WARN: usize = 3;
const MAIN_AGENT_LOOP_STOP: usize = 5;
const EXPLORATION_AGENT_LOOP_WARN: usize = 3;
const EXPLORATION_AGENT_LOOP_STOP: usize = 5;
const VERIFICATION_AGENT_LOOP_WARN: usize = 2;
const VERIFICATION_AGENT_LOOP_STOP: usize = 3;

/// Agent roles for trace logging and guard behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRole {
    Main,
    Exploration,
    Verification,
}

impl AgentRole {
    fn warn_threshold(self) -> usize {
        match self {
            AgentRole::Main => MAIN_AGENT_LOOP_WARN,
            AgentRole::Exploration => EXPLORATION_AGENT_LOOP_WARN,
            AgentRole::Verification => VERIFICATION_AGENT_LOOP_WARN,
        }
    }

    fn stop_threshold(self) -> usize {
        match self {
            AgentRole::Main => MAIN_AGENT_LOOP_STOP,
            AgentRole::Exploration => EXPLORATION_AGENT_LOOP_STOP,
            AgentRole::Verification => VERIFICATION_AGENT_LOOP_STOP,
        }
    }
}

/// Deterministic failure reasons that should not be retried automatically.
const DETERMINISTIC_FAILURE_REASONS: &[&str] = &[
    "blocked",
    "path_escape",
    "secret",
    "not_found",
    "invalid_query",
    "binary",
    "too_large",
    "invalid_range",
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
    fn new(role: AgentRole) -> Self {
        Self {
            last_call_hash: 0,
            consecutive_count: 0,
            last_failure_reason: String::new(),
            failure_streak: 0,
            warn_threshold: role.warn_threshold(),
            stop_threshold: role.stop_threshold(),
        }
    }

    fn canonicalize_args(args: &serde_json::Value) -> String {
        let sorted = sort_json_keys(args);
        serde_json::to_string(&sorted).unwrap_or_default()
    }

    fn record_call(&mut self, tool_name: &str, args: &serde_json::Value) -> LoopStatus {
        let canonical = format!("{}:{}", tool_name, Self::canonicalize_args(args));
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

fn sort_json_keys(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted: Vec<_> = map.iter().collect();
            sorted.sort_by_key(|(k, _)| k.clone());
            let sorted_map: serde_json::Map<String, serde_json::Value> = sorted
                .into_iter()
                .map(|(k, v)| (k.clone(), sort_json_keys(v)))
                .collect();
            serde_json::Value::Object(sorted_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(sort_json_keys).collect())
        }
        other => other.clone(),
    }
}

use crate::corpus::tools::{
    create_corpus_neighbors_tool, create_corpus_query_tool, create_corpus_summary_tool,
    CORPUS_SYSTEM_PROMPT,
};
use crate::workspace_tools::{
    create_context_search_tool, create_find_files_tool, create_list_directory_tool,
    create_read_file_tool, create_search_code_tool, create_write_harness_file_tool,
    truncate_text_bytes, HARNESS_CONTROL_AREA_SYSTEM_PROMPT, WORKSPACE_TOOLS_SYSTEM_PROMPT,
};

const AGENT_MAX_TURNS: usize = 20;
const EXPLORATION_TIMEOUT: Duration = Duration::from_secs(90);
const EXPLORATION_REPORT_BYTES_CAP: usize = 32 * 1024;
const DELEGATION_SYSTEM_PROMPT: &str = r#"
Use `delegate_exploration` only for broad multi-file, architectural, or investigative tasks that would benefit from a focused report before you answer the user.
Prefer direct file reads and targeted search for small or obvious tasks.
"#;
const EXPLORATION_AGENT_PROMPT: &str = r#"
You are the Gospel Exploration Agent.

Investigate the active workspace and return a concise markdown report with exactly these section headings:

## Summary
## Key Files
## Findings
## Constraints
## Suggested Next Reads
## Tools Used

Use `context_search` for broad workspace discovery to find relevant areas quickly.
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceToolContext {
    pub workspace_path: PathBuf,
    pub corpus_available: bool,
}

pub struct LlmService;

fn validate_api_key(provider: &str, api_key: &str) -> Result<(), LlmError> {
    if provider != "chatgpt" && api_key.trim().is_empty() {
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

        let response = match provider {
            "openai" => {
                let client = openai::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent
                    .prompt(prompt)
                    .await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "chatgpt" => {
                let client = chatgpt::Client::builder()
                    .oauth()
                    .build()
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent
                    .prompt(prompt)
                    .await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "anthropic" => {
                let client = anthropic::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent
                    .prompt(prompt)
                    .await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "gemini" => {
                let client = gemini::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent
                    .prompt(prompt)
                    .await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "groq" => {
                let client = groq::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent
                    .prompt(prompt)
                    .await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "mistral" => {
                let client = mistral::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent
                    .prompt(prompt)
                    .await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            _ => return Err(LlmError::UnsupportedProvider(provider.to_string())),
        };
        Ok(response)
    }
}

#[derive(Debug)]
pub struct StreamCompletionResult {
    pub full_response: String,
    pub history: Option<Vec<Message>>,
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
        let prompt = build_exploration_prompt(&args);
        match run_exploration_agent(
            &self.provider,
            &self.model,
            &self.api_key,
            &self.workspace,
            &prompt,
        )
        .await
        {
            Ok(output) => Ok(output),
            Err(error) => Ok(DelegateExplorationOutput {
                success: false,
                truncated: false,
                report: String::new(),
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

    fn on_tool_result(
        &self,
        _tool_name: &str,
        _tool_call_id: Option<String>,
        _internal_call_id: &str,
        _args: &str,
        _result: &str,
    ) -> impl Future<Output = HookAction> + Send {
        async { HookAction::cont() }
    }
}

fn build_system_preamble(
    workspace: Option<&WorkspaceToolContext>,
    allow_delegate: bool,
    matched_skills_section: Option<String>,
    invoked_skills_section: Option<String>,
    include_harness: bool,
) -> Option<String> {
    let mut sections = Vec::new();

    if let Some(ref invoked) = invoked_skills_section {
        sections.push(invoked.clone());
    }

    if let Some(ref matched) = matched_skills_section {
        sections.push(matched.clone());
    }

    if workspace.is_some() {
        sections.push(WORKSPACE_TOOLS_SYSTEM_PROMPT.trim().to_string());
        if include_harness {
            sections.push(HARNESS_CONTROL_AREA_SYSTEM_PROMPT.trim().to_string());
        }
    }

    if workspace.map(|ctx| ctx.corpus_available).unwrap_or(false) {
        sections.push(CORPUS_SYSTEM_PROMPT.trim().to_string());
    }

    if allow_delegate && workspace.is_some() {
        sections.push(DELEGATION_SYSTEM_PROMPT.trim().to_string());
    }

    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

fn build_exploration_prompt(args: &DelegateExplorationArgs) -> String {
    let mut sections = vec![format!("Task:\n{}", args.task.trim())];
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
    prompt: &str,
) -> Result<DelegateExplorationOutput, LlmError> {
    let hook = ExplorationHook {
        tools: Arc::new(Mutex::new(Vec::new())),
    };
    let tool_names = hook.tools.clone();
    let agent_preamble = format!(
        "{}\n\n{}",
        build_system_preamble(Some(workspace), false, None, None, false).unwrap_or_default(),
        EXPLORATION_AGENT_PROMPT.trim()
    );

    // Exploration agent uses tighter loop thresholds
    let loop_detector = Arc::new(Mutex::new(LoopDetector::new(AgentRole::Exploration)));

    macro_rules! exploration_from_client {
        ($client:expr, $model:expr) => {{
            let mut builder = $client
                .agent($model)
                .preamble(&agent_preamble)
                .default_max_turns(AGENT_MAX_TURNS)
                .tool(create_read_file_tool(workspace.workspace_path.clone()))
                .tool(create_search_code_tool(workspace.workspace_path.clone()))
                .tool(create_find_files_tool(workspace.workspace_path.clone()))
                .tool(create_list_directory_tool(workspace.workspace_path.clone()));

            if workspace.corpus_available {
                builder = builder
                    .tool(create_corpus_summary_tool(workspace.workspace_path.clone()))
                    .tool(create_corpus_query_tool(workspace.workspace_path.clone()))
                    .tool(create_corpus_neighbors_tool(
                        workspace.workspace_path.clone(),
                    ))
                    .tool(create_context_search_tool(workspace.workspace_path.clone()));
            }

            let agent = builder.build();
            let future = agent
                .prompt(prompt)
                .with_hook(hook.clone())
                .extended_details();
            let response = timeout(EXPLORATION_TIMEOUT, future)
                .await
                .map_err(|_| {
                    LlmError::ProviderError(
                        "Exploration Agent timed out after 90 seconds".to_string(),
                    )
                })?
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;

            let tools_used = tool_names.lock().unwrap().clone();
            let (report, truncated) =
                truncate_text_bytes(&response.output, EXPLORATION_REPORT_BYTES_CAP);
            DelegateExplorationOutput {
                success: true,
                truncated,
                report,
                tools_used,
                message: "Exploration completed.".to_string(),
                reason: None,
            }
        }};
    }

    match provider {
        "openai" => {
            let client =
                openai::Client::new(api_key).map_err(|e| LlmError::ProviderError(e.to_string()))?;
            Ok(exploration_from_client!(client, model))
        }
        "chatgpt" => {
            let client = chatgpt::Client::builder()
                .oauth()
                .build()
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            Ok(exploration_from_client!(client, model))
        }
        "anthropic" => {
            let client = anthropic::Client::new(api_key)
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            Ok(exploration_from_client!(client, model))
        }
        "gemini" => {
            let client =
                gemini::Client::new(api_key).map_err(|e| LlmError::ProviderError(e.to_string()))?;
            Ok(exploration_from_client!(client, model))
        }
        "groq" => {
            let client =
                groq::Client::new(api_key).map_err(|e| LlmError::ProviderError(e.to_string()))?;
            Ok(exploration_from_client!(client, model))
        }
        "mistral" => {
            let client = mistral::Client::new(api_key)
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            Ok(exploration_from_client!(client, model))
        }
        _ => Err(LlmError::UnsupportedProvider(provider.to_string())),
    }
}

pub async fn stream_completion<F>(
    provider: &str,
    prompt: &str,
    model: &str,
    api_key: &str,
    workspace: Option<WorkspaceToolContext>,
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

    let mut full_response = String::new();
    let mut captured_history: Option<Vec<Message>> = None;

    macro_rules! stream_from_client {
        ($client:expr, $model:expr) => {{
            let builder = $client.agent($model).default_max_turns(AGENT_MAX_TURNS);
            let builder = if let Some(preamble) = build_system_preamble(workspace.as_ref(), true, matched_skills_section.clone(), invoked_skill_section.clone(), true) {
                builder.preamble(&preamble)
            } else {
                builder
            };
            let agent = if let Some(workspace_context) = workspace.as_ref() {
                let mut b = builder
                    .tool(create_read_file_tool(
                        workspace_context.workspace_path.clone(),
                    ))
                    .tool(create_search_code_tool(
                        workspace_context.workspace_path.clone(),
                    ))
                    .tool(create_find_files_tool(
                        workspace_context.workspace_path.clone(),
                    ))
                    .tool(create_list_directory_tool(
                        workspace_context.workspace_path.clone(),
                    ))
                    .tool(create_write_harness_file_tool(
                        workspace_context.workspace_path.clone(),
                    ));

                if workspace_context.corpus_available {
                    b = b
                        .tool(create_corpus_summary_tool(
                            workspace_context.workspace_path.clone(),
                        ))
                        .tool(create_corpus_query_tool(
                            workspace_context.workspace_path.clone(),
                        ))
                        .tool(create_corpus_neighbors_tool(
                            workspace_context.workspace_path.clone(),
                        ))
                        .tool(create_context_search_tool(
                            workspace_context.workspace_path.clone(),
                        ));
                }

                b = b.tool(DelegateExplorationTool {
                    workspace: workspace_context.clone(),
                    provider: provider.to_string(),
                    model: model.to_string(),
                    api_key: api_key.to_string(),
                });
                if let Some(st) = skill_script_tool {
                    b = b.tool(st);
                }
                b.build()
            } else if let Some(st) = skill_script_tool {
                builder.tool(st).build()
            } else {
                builder.build()
            };
            let request = agent
                .stream_chat(prompt, chat_history)
                .multi_turn(AGENT_MAX_TURNS);
            let mut stream = request.await;

            let mut tool_name_by_id: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            let mut loop_detector = LoopDetector::new(AgentRole::Main);

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

                            // Check for repeated deterministic failures
                            match serde_json::from_str::<serde_json::Value>(&result_summary) {
                                Ok(parsed) => {
                                    if let Some(reason) = parsed.get("reason").and_then(|r| r.as_str()) {
                                        if let Some(status) = loop_detector.record_failure(reason) {
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
                                    } else {
                                        loop_detector.reset_failure_streak();
                                    }
                                }
                                Err(_) => loop_detector.reset_failure_streak(),
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

    match provider {
        "openai" => {
            let client =
                openai::Client::new(api_key).map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "chatgpt" => {
            let client = chatgpt::Client::builder()
                .oauth()
                .build()
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "anthropic" => {
            let client = anthropic::Client::new(api_key)
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "gemini" => {
            let client =
                gemini::Client::new(api_key).map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "groq" => {
            let client =
                groq::Client::new(api_key).map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "mistral" => {
            let client = mistral::Client::new(api_key)
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        _ => return Err(LlmError::UnsupportedProvider(provider.to_string())),
    }

    Ok(StreamCompletionResult {
        full_response,
        history: captured_history,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delegate_tool_for_test() -> DelegateExplorationTool {
        DelegateExplorationTool {
            workspace: WorkspaceToolContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: false,
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
            "",
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
    fn validate_api_key_rejects_blank_key_for_non_chatgpt() {
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
    fn build_system_preamble_uses_live_tools_and_corpus_distinction() {
        let preamble = build_system_preamble(
            Some(&WorkspaceToolContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: true,
            }),
            true,
            None,
            None,
            true,
        )
        .unwrap();

        assert!(preamble.contains("Live Workspace Tools"));
        assert!(preamble.contains("source-of-truth"));
        assert!(preamble.contains("delegate_exploration"));
        assert!(preamble.contains("Harness Control Area"));
        assert!(preamble.contains(".gospel/PLAN.md"));
    }

    #[test]
    fn build_system_preamble_is_empty_without_workspace_tools() {
        assert!(build_system_preamble(None, true, None, None, true).is_none());
    }

    #[test]
    fn build_exploration_prompt_includes_optional_context() {
        let prompt = build_exploration_prompt(&DelegateExplorationArgs {
            task: "Trace startup flow".to_string(),
            context: Some("Focus on Tauri setup".to_string()),
            expected_output: Some("Highlight risky assumptions".to_string()),
        });

        assert!(prompt.contains("Task:"));
        assert!(prompt.contains("Focus on Tauri setup"));
        assert!(prompt.contains("Highlight risky assumptions"));
    }
}
