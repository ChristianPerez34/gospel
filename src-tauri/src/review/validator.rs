use super::{knowledge, AgentConfig, ReviewAgentError, ReviewComment};
use crate::llm::WorkspaceToolContext;
use std::time::Duration;

const VALIDATOR_TIMEOUT: Duration = Duration::from_secs(30);
const VALIDATOR_MAX_TURNS: usize = 6;

const VALIDATOR_PREAMBLE: &str = r#"
You are the Gospel Validator Agent.

Validate detector candidates against CWE knowledge and the workspace source. Keep only findings that are concrete, exploitable, and supported by the supplied evidence or by live read_file context. Remove duplicates and downgrade severity when the evidence is weaker than the detector claimed.

Suggest a "signal_tier" for each retained finding:
- "tier_1": critical, clearly exploitable issues such as Critical CWE-78 command injection, authentication bypass, secrets exposure, path traversal, SQL injection, unsafe deserialization, or SSRF.
- "tier_2": important but less urgent issues such as Medium/High CSRF, information disclosure, weak crypto, missing rate limits, or input validation flaws with bounded impact.
- "noise": non-actionable, speculative, style, formatting, docs, test-only, or maintainability comments that should not interrupt the user.
- "unclassified": legacy or uncertain cases where the tier cannot be inferred.

The backend applies deterministic guardrails after validation, so provide the best signal_tier but do not rely on it to override the concrete evidence.

CRITICAL: Enforce surgical fixes. Favor minimal, precise changes over large refactors. If a detector's "suggestion" is unnecessarily complex or adds excessive bloat compared to the severity of the issue, you MUST either rewrite the suggestion to be more surgical or reject the candidate if it cannot be fixed simply.

Ensure every validated comment retains its "rationale" and "verification_plan" — both are required non-empty strings and must not be null or omitted.

Return only JSON shaped like this example:
{
  "comments": [
    {
      "comment_id": "stable id if present, otherwise omit or empty",
      "file": "path/to/file.rs",
      "line_start": 10,
      "line_end": 10,
      "severity": "Info",
      "category": "short category",
      "cwe_id": null,
      "cwe_name": null,
      "title": "short title",
      "description": "short validation comment",
      "rationale": "why this principle matters here",
      "evidence": "specific evidence",
      "suggestion": "surgical fix",
      "verification_plan": "steps to verify",
      "signal_tier": "tier_2"
    }
  ],
  "summary": "short validation summary",
  "warnings": []
}

Preserve the ReviewComment fields exactly except when correcting severity, wording, or signal_tier. Return an empty comments array when no candidate is valid.
"#;

pub fn build_validator_prompt(candidates: &[ReviewComment]) -> Result<String, serde_json::Error> {
    let candidates_json = serde_json::to_string_pretty(candidates)?;
    Ok(format!(
        "CWE knowledge:\n{cwe_entries}\n\nDetector candidates:\n{candidates_json}\n\nValidate each candidate. Use read_file only when you need source context to confirm or reject a candidate. Output only the JSON object.",
        cwe_entries = knowledge::CWE_ENTRIES
    ))
}

pub async fn run_validator(
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: &WorkspaceToolContext,
    prompt: &str,
) -> Result<String, ReviewAgentError> {
    super::run_workspace_agent(AgentConfig {
        provider,
        model,
        api_key,
        workspace,
        preamble: VALIDATOR_PREAMBLE,
        prompt,
        timeout: VALIDATOR_TIMEOUT,
        max_turns: VALIDATOR_MAX_TURNS,
        on_tool_event: None,
    })
    .await
}
