use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::streaming::{StreamedAssistantContent, StreamingPrompt};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::timeout;

use crate::corpus::tools::{
    create_corpus_neighbors_tool, create_corpus_query_tool, create_corpus_summary_tool,
    CORPUS_SYSTEM_PROMPT,
};
use crate::llm::WorkspaceToolContext;
use crate::models::ModelRegistry;
use crate::provider_client::provider_client;
use crate::workspace_tools::{
    build_base_workspace_tools, create_context_search_tool, WORKSPACE_TOOLS_SYSTEM_PROMPT,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub status: VerificationStatus,
    pub concerns: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum VerificationStatus {
    Pass,
    Concerns,
    Fail,
    Unavailable,
}

impl Default for VerificationResult {
    fn default() -> Self {
        Self {
            status: VerificationStatus::Unavailable,
            concerns: vec![],
            summary: "Verification not available".to_string(),
        }
    }
}

const VERIFICATION_SYSTEM_PROMPT: &str = r#"You are the Gospel Verification Agent.

Review the assistant's response for correctness, completeness, and potential issues. Focus on:
- Factual accuracy of code-related claims
- Whether file paths and references actually exist
- Whether suggested changes are safe and correct
- Potential edge cases or issues missed

Use `context_search` for scoped claim checking only. Do not expand task scope.
Use live workspace tools (read_file, search_code) to verify specific claims.
Remain read-only and focused on verification.

Return your assessment as JSON with exactly these fields:
{
  "status": "pass" | "concerns" | "fail",
  "concerns": ["list of specific concerns"],
  "summary": "brief summary of verification"
}

If you cannot verify something, set status to "unavailable" and explain why in the summary.
Be concise and focused on concrete issues, not stylistic preferences."#;

const VERIFICATION_MAX_TURNS: usize = 6;
const VERIFICATION_TIMEOUT: Duration = Duration::from_secs(90);

pub async fn run_verification(
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: &WorkspaceToolContext,
    response_to_verify: &str,
    user_prompt: &str,
) -> VerificationResult {
    if !ModelRegistry::is_oauth_provider(provider) && api_key.trim().is_empty() {
        return unavailable("Verification unavailable: API key is not configured.");
    }

    let prompt = build_verification_prompt(workspace, response_to_verify, user_prompt);
    let output = match timeout(
        VERIFICATION_TIMEOUT,
        run_verification_agent(provider, model, api_key, workspace, &prompt),
    )
    .await
    {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            return unavailable(&format!("Verification unavailable: {}", error));
        }
        Err(_) => {
            return unavailable("Verification unavailable: verification agent timed out.");
        }
    };

    parse_verification_output(&output).unwrap_or_else(|| {
        unavailable("Verification unavailable: verification agent returned malformed JSON.")
    })
}

async fn run_verification_agent(
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: &WorkspaceToolContext,
    prompt: &str,
) -> Result<String, String> {
    let preamble = build_verification_preamble(workspace);

    macro_rules! verify_from_client {
        ($client:expr, $model:expr) => {{
            let mut builder = $client
                .agent($model)
                .preamble(&preamble)
                .default_max_turns(VERIFICATION_MAX_TURNS)
                .tools(build_base_workspace_tools(workspace.workspace_path.clone()));

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
            // Use stream_prompt instead of prompt: the ChatGPT Codex
            // backend's non-streaming path drops tool calls when
            // response.completed has an empty output array.
            // See https://github.com/0xPlaygrounds/rig/issues/2000.
            let mut stream = agent
                .stream_prompt(prompt)
                .multi_turn(VERIFICATION_MAX_TURNS)
                .await;
            let mut final_output = String::new();
            loop {
                match timeout(VERIFICATION_TIMEOUT, stream.next()).await {
                    Err(_) => break Err("verification agent timed out".to_string()),
                    Ok(None) => break Ok(final_output),
                    Ok(Some(item)) => match item {
                        Ok(MultiTurnStreamItem::FinalResponse(final_response)) => {
                            final_output = final_response.response().to_owned();
                            break Ok(final_output);
                        }
                        Ok(MultiTurnStreamItem::StreamAssistantItem(
                            StreamedAssistantContent::Text(text),
                        )) => {
                            final_output.push_str(&text.text);
                        }
                        Ok(_) => {}
                        Err(error) => break Err(error.to_string()),
                    },
                }
            }
        }};
    }

    provider_client!(
        provider,
        api_key,
        |e: String| e,
        |s: String| format!("unsupported provider: {}", s),
        |client| { verify_from_client!(client, model) }
    )
}

fn build_verification_preamble(workspace: &WorkspaceToolContext) -> String {
    let mut sections = vec![
        WORKSPACE_TOOLS_SYSTEM_PROMPT.trim().to_string(),
        VERIFICATION_SYSTEM_PROMPT.trim().to_string(),
    ];

    if workspace.corpus_available {
        sections.insert(1, CORPUS_SYSTEM_PROMPT.trim().to_string());
    }

    sections.join("\n\n")
}

fn build_verification_prompt(
    workspace: &WorkspaceToolContext,
    response_to_verify: &str,
    user_prompt: &str,
) -> String {
    format!(
        "Workspace root:\n{}\n\nUser prompt:\n{}\n\nAssistant response to verify:\n{}",
        workspace.workspace_path.display(),
        user_prompt,
        response_to_verify
    )
}

fn parse_verification_output(output: &str) -> Option<VerificationResult> {
    let trimmed = output.trim();
    serde_json::from_str::<VerificationResult>(trimmed)
        .ok()
        .or_else(|| {
            let start = trimmed.find('{')?;
            let end = trimmed.rfind('}')?;
            if end <= start {
                return None;
            }
            serde_json::from_str::<VerificationResult>(&trimmed[start..=end]).ok()
        })
}

fn unavailable(summary: &str) -> VerificationResult {
    VerificationResult {
        status: VerificationStatus::Unavailable,
        concerns: vec![],
        summary: summary.to_string(),
    }
}
