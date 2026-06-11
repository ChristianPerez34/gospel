use super::{knowledge, ReviewAgentError, ReviewComment};
use crate::llm::WorkspaceToolContext;
use std::time::Duration;

const VALIDATOR_TIMEOUT: Duration = Duration::from_secs(30);
const VALIDATOR_MAX_TURNS: usize = 6;

const VALIDATOR_PREAMBLE: &str = r#"
You are the Gospel Validator Agent.

Validate detector candidates against CWE knowledge and the workspace source. Keep only findings that are concrete, exploitable, and supported by the supplied evidence or by live read_file context. Remove duplicates and downgrade severity when the evidence is weaker than the detector claimed.

Return only JSON shaped like this example:
{
  "comments": [
    {
      "file": "path/to/file.rs",
      "line_start": 10,
      "line_end": 10,
      "severity": "Info",
      "category": "short category",
      "cwe_id": null,
      "cwe_name": null,
      "title": "short title",
      "description": "short validation comment",
      "evidence": "specific evidence",
      "suggestion": "optional fix"
    }
  ],
  "summary": "short validation summary",
  "warnings": []
}

Preserve the ReviewComment fields exactly. Return an empty comments array when no candidate is valid.
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
    super::run_workspace_agent(
        provider,
        model,
        api_key,
        workspace,
        VALIDATOR_PREAMBLE,
        prompt,
        VALIDATOR_TIMEOUT,
        VALIDATOR_MAX_TURNS,
    )
    .await
}
