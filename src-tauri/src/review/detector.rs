use super::{knowledge, FileDiff, ReviewAgentError};
use crate::llm::WorkspaceToolContext;
use std::time::Duration;

const DETECTOR_TIMEOUT: Duration = Duration::from_secs(60);
const DETECTOR_MAX_TURNS: usize = 10;

const DETECTOR_PREAMBLE: &str = r#"
You are the Gospel Detector Agent.

Find plausible security vulnerabilities in the supplied changes or files. Use live workspace tools, especially read_file, to inspect surrounding code when a finding depends on context. Do not report style issues, general bugs, or speculative risks without direct code evidence.

Return only JSON with this shape:
{
  "comments": [
    {
      "file": "path/from/workspace",
      "line_start": 1,
      "line_end": 1,
      "severity": "Critical|High|Medium|Low|Info",
      "category": "short category",
      "cwe_id": "CWE-89",
      "cwe_name": "SQL Injection",
      "title": "short title",
      "description": "why this is exploitable",
      "evidence": "exact code evidence from the diff or file",
      "suggestion": "specific fix"
    }
  ],
  "summary": "short detector summary",
  "warnings": []
}

Every comment must cite concrete code in evidence. Return an empty comments array when there are no credible security findings.
"#;

pub fn build_diff_prompt(review_context: &str, files: &[FileDiff]) -> String {
    let file_list = files
        .iter()
        .map(|file| format!("- {} ({} diff lines)", file.file, file.line_count))
        .collect::<Vec<_>>()
        .join("\n");
    let diff = files
        .iter()
        .map(|file| file.diff.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "{review_context}\n\nSAST rules:\n{rules}\n\nFiles in this batch:\n{file_list}\n\n--- Diff ---\n{diff}\n\nAnalyze only this batch. Use read_file when surrounding context is needed. Output only the JSON object.",
        rules = knowledge::SAST_RULES
    )
}

pub fn build_scan_prompt<'a>(files: impl IntoIterator<Item = &'a str>) -> String {
    let file_list = files
        .into_iter()
        .map(|file| format!("- {file}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "You are reviewing an entire repository for security vulnerabilities.\n\nSAST rules:\n{rules}\n\nHere are the files to review:\n{file_list}\n\nFocus on files most likely to have security issues:\n- Authentication and authorization logic\n- Database queries and ORM usage\n- File operations and path handling\n- Network calls and external API usage\n- Input handling and validation\n- Serialization and deserialization\n\nUse read_file to examine each file. For each potential vulnerability, output a structured JSON object. Output only the JSON object.",
        rules = knowledge::SAST_RULES
    )
}

pub async fn run_detector(
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
        DETECTOR_PREAMBLE,
        prompt,
        DETECTOR_TIMEOUT,
        DETECTOR_MAX_TURNS,
    )
    .await
}
