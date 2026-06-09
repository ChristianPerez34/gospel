use serde::{Deserialize, Serialize};

use crate::llm::WorkspaceToolContext;
use crate::workspace_tools::WORKSPACE_TOOLS_SYSTEM_PROMPT;

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

pub async fn run_verification(
    _provider: &str,
    _model: &str,
    _api_key: &str,
    _workspace: &WorkspaceToolContext,
    _response_to_verify: &str,
    _user_prompt: &str,
) -> VerificationResult {
    // TODO: Implement actual verification using LLM
    // For now, return unavailable to not block the main response
    VerificationResult::default()
}
