use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::streaming::{StreamedAssistantContent, StreamingPrompt};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

use crate::harness_profile::{
    resolve_harness_profile, ActiveWorkspaceContext, AgentRole, HarnessProfile,
    HarnessProfileRequest, LoopDetector, LoopStatus,
};
use crate::models::ModelRegistry;
use crate::provider_client::provider_client;

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

Use scoped workspace retrieval for claim checking when it is available. Do not expand task scope.
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

pub async fn run_verification(
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: &ActiveWorkspaceContext,
    response_to_verify: &str,
    user_prompt: &str,
) -> VerificationResult {
    if !ModelRegistry::is_oauth_provider(provider) && api_key.trim().is_empty() {
        return unavailable("Verification unavailable: API key is not configured.");
    }

    let prompt = build_verification_prompt(workspace, response_to_verify, user_prompt);
    let profile = match resolve_harness_profile(HarnessProfileRequest {
        role: AgentRole::Verification,
        workspace: Some(workspace.clone()),
        role_guidance: Some(VERIFICATION_SYSTEM_PROMPT.to_string()),
        matched_skills_section: None,
        invoked_skill_section: None,
        main_tool_inputs: None,
    }) {
        Ok(profile) => profile,
        Err(error) => return unavailable(&format!("Verification unavailable: {error}")),
    };
    tracing::debug!(profile = ?profile.summary(), "resolved Verification Harness Profile");
    let deadline = profile
        .guards
        .deadline
        .expect("Verification Harness Profiles always have a deadline");
    let output = match timeout(
        deadline,
        run_verification_agent(provider, model, api_key, profile, &prompt),
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
    profile: HarnessProfile,
    prompt: &str,
) -> Result<String, String> {
    let preamble = profile.preamble.unwrap_or_default();
    let tools = profile.tools;
    let guards = profile.guards;
    let deadline = guards
        .deadline
        .expect("Verification Harness Profiles always have a deadline");

    macro_rules! verify_from_client {
        ($client:expr, $model:expr) => {{
            let builder = $client
                .agent($model)
                .preamble(&preamble)
                .default_max_turns(guards.max_turns)
                .tools(tools);

            let agent = builder.build();
            // Use stream_prompt instead of prompt: the ChatGPT Codex
            // backend's non-streaming path drops tool calls when
            // response.completed has an empty output array.
            // See https://github.com/0xPlaygrounds/rig/issues/2000.
            let mut stream = agent
                .stream_prompt(prompt)
                .multi_turn(guards.max_turns)
                .await;
            let mut final_output = String::new();
            let mut loop_detector = LoopDetector::new(guards.loop_guard);
            loop {
                match timeout(deadline, stream.next()).await {
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
                        Ok(MultiTurnStreamItem::StreamAssistantItem(
                            StreamedAssistantContent::ToolCall { tool_call, .. },
                        )) => {
                            match loop_detector.record_call(
                                &tool_call.function.name,
                                &tool_call.function.arguments,
                            ) {
                                LoopStatus::Stop => break Err(format!(
                                    "verification agent stopped after a repeated '{}' tool-call loop",
                                    tool_call.function.name
                                )),
                                LoopStatus::Warning(count) => tracing::warn!(
                                    role = "verification",
                                    count,
                                    tool_name = tool_call.function.name,
                                    "repeated tool-call warning"
                                ),
                                LoopStatus::Ok => {}
                            }
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

fn build_verification_prompt(
    workspace: &ActiveWorkspaceContext,
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
