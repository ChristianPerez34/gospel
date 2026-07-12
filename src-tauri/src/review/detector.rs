use super::{knowledge, AgentConfig, FileDiff, ReviewAgentError, ReviewFocus, ToolEventObserver};
use crate::harness_profile::{ActiveWorkspaceContext, AgentRole};

const SECURITY_DETECTOR_PREAMBLE: &str = r#"
You are the Gospel Security Detector Agent.

Find plausible security vulnerabilities in the supplied changes or files. Use live workspace tools, especially read_file, to inspect surrounding code when a finding depends on context. Do not report style issues, general bugs, or speculative risks without direct code evidence.

Every comment MUST include a non-empty "rationale" (required — the engineering principle or architectural reason for the fix) and a non-empty "verification_plan" (required — how a developer can test the fix). Do NOT omit or set these to null.

Every comment SHOULD include a "signal_tier" suggestion: "tier_1" for critical exploitable findings, "tier_2" for important actionable findings, "noise" for non-actionable comments, or "unclassified" when uncertain. The Validator and backend guardrails will make the final tier decision.

Return only JSON with this shape:
{
  "comments": [
    {
      "comment_id": "",
      "file": "path/from/workspace",
      "line_start": 1,
      "line_end": 1,
      "severity": "Critical|High|Medium|Low|Info",
      "category": "short category",
      "cwe_id": "CWE-89",
      "cwe_name": "SQL Injection",
      "title": "short title",
      "description": "why this is exploitable",
      "rationale": "the engineering principle or architectural reason for the fix",
      "evidence": "exact code evidence from the diff or file",
      "suggestion": "specific, surgical fix (avoid unnecessary refactors)",
      "verification_plan": "how to verify the fix with a test or manual step",
      "signal_tier": "tier_1|tier_2|noise|unclassified"
    }
  ],
  "summary": "short detector summary",
  "warnings": []
}

Every comment must cite concrete code in evidence. Return an empty comments array when there are no credible security findings.
"#;

const BUG_HUNT_DETECTOR_PREAMBLE: &str = r#"
You are the Gospel Bug Hunt Detector Agent.

Find concrete correctness defects in the supplied changes or files: logic errors, edge-case failures, off-by-one boundaries, null or missing-state handling, race conditions, resource leaks, swallowed errors, incorrect fallback behavior, and state-machine violations. Use live workspace tools, especially read_file, to inspect surrounding code when a finding depends on context. Do not report security, style, architecture, or speculative issues without direct code evidence.

Every comment MUST include a non-empty "rationale" and "verification_plan". Every comment SHOULD include a "signal_tier" suggestion. Return only the JSON shape used by Gospel review comments. Use null for cwe_id and cwe_name unless the bug is also a precise CWE-backed security issue.

Every comment must cite concrete code in evidence. Return an empty comments array when there are no credible bug findings.
"#;

const ARCHITECTURE_DETECTOR_PREAMBLE: &str = r#"
You are the Gospel Architecture Detector Agent.

Find concrete design and module-boundary problems in the supplied changes or files: dependency direction violations, circular coupling, abstraction leaks, broken API contracts, misplaced responsibilities, shallow pass-through modules, hidden global state, and changes that make testing or maintenance materially worse. Use live workspace tools, especially read_file, to inspect surrounding code when a finding depends on context. Do not report formatting, taste, or speculative issues without direct code evidence.

Every comment MUST include a non-empty "rationale" and "verification_plan". Every comment SHOULD include a "signal_tier" suggestion. Return only the JSON shape used by Gospel review comments. Use null for cwe_id and cwe_name.

Every comment must cite concrete code in evidence. Return an empty comments array when there are no credible architecture findings.
"#;

const PERFORMANCE_DETECTOR_PREAMBLE: &str = r#"
You are the Gospel Performance Detector Agent.

Find concrete performance defects in the supplied changes or files: avoidable O(n²) paths, repeated expensive work, unnecessary allocations or cloning on hot paths, blocking I/O in async paths, unbounded memory growth, missing batching, N+1 calls, and cache invalidation problems. Use live workspace tools, especially read_file, to inspect surrounding code when a finding depends on context. Do not report micro-optimizations or speculative improvements without direct code evidence.

Every comment MUST include a non-empty "rationale" and "verification_plan". Every comment SHOULD include a "signal_tier" suggestion. Return only the JSON shape used by Gospel review comments. Use null for cwe_id and cwe_name.

Every comment must cite concrete code in evidence. Return an empty comments array when there are no credible performance findings.
"#;

const STYLE_DETECTOR_PREAMBLE: &str = r#"
You are the Gospel Style Detector Agent.

Find concrete readability, idiom, naming, documentation, and maintainability issues in the supplied changes or files: inconsistent names, unclear public interfaces, dead or redundant code, overly complex expressions, non-idiomatic language use, and missing documentation where project conventions require it. Use live workspace tools, especially read_file, to inspect surrounding code when a finding depends on context. Do not report personal taste, formatting churn, or issues already covered by security, bugs, architecture, or performance.

Every comment MUST include a non-empty "rationale" and "verification_plan". Every comment SHOULD include a "signal_tier" suggestion. Return only the JSON shape used by Gospel review comments. Use null for cwe_id and cwe_name.

Every comment must cite concrete code in evidence. Return an empty comments array when there are no credible style findings.
"#;

pub fn preamble_for_focus(focus: ReviewFocus) -> &'static str {
    match focus {
        ReviewFocus::Security => SECURITY_DETECTOR_PREAMBLE,
        ReviewFocus::BugHunt => BUG_HUNT_DETECTOR_PREAMBLE,
        ReviewFocus::Architecture => ARCHITECTURE_DETECTOR_PREAMBLE,
        ReviewFocus::Performance => PERFORMANCE_DETECTOR_PREAMBLE,
        ReviewFocus::Style => STYLE_DETECTOR_PREAMBLE,
    }
}

fn knowledge_for_focus(focus: ReviewFocus) -> &'static str {
    match focus {
        ReviewFocus::Security => knowledge::SAST_RULES,
        ReviewFocus::BugHunt => knowledge::BUG_PATTERNS,
        ReviewFocus::Architecture => knowledge::ARCHITECTURE_PATTERNS,
        ReviewFocus::Performance => knowledge::PERFORMANCE_PATTERNS,
        ReviewFocus::Style => knowledge::STYLE_RULES,
    }
}

fn knowledge_label_for_focus(focus: ReviewFocus) -> &'static str {
    match focus {
        ReviewFocus::Security => "SAST rules",
        ReviewFocus::BugHunt => "Bug patterns",
        ReviewFocus::Architecture => "Architecture patterns",
        ReviewFocus::Performance => "Performance patterns",
        ReviewFocus::Style => "Style rules",
    }
}

fn scan_guidance_for_focus(focus: ReviewFocus) -> &'static str {
    match focus {
        ReviewFocus::Security => "Focus on files most likely to have security issues:\n- Authentication and authorization logic\n- Database queries and ORM usage\n- File operations and path handling\n- Network calls and external API usage\n- Input handling and validation\n- Serialization and deserialization",
        ReviewFocus::BugHunt => "Focus on files most likely to contain correctness bugs:\n- State transitions and reducers\n- Error handling and fallback paths\n- Async/concurrent code\n- Boundary-heavy loops and indexes\n- Parsing, serialization, and validation logic",
        ReviewFocus::Architecture => "Focus on files most likely to reveal architectural issues:\n- Module interfaces and adapters\n- Cross-layer imports\n- Shared state and singleton access\n- Error propagation seams\n- Public types used across feature areas",
        ReviewFocus::Performance => "Focus on files most likely to affect performance:\n- Hot-path loops and collection transformations\n- I/O and network call sites\n- Async task orchestration\n- Caching and invalidation logic\n- Large file or large data processing",
        ReviewFocus::Style => "Focus on files most likely to have clarity issues:\n- Public interfaces and exported types\n- Newly added helpers\n- Complex conditionals and expressions\n- Naming around domain concepts\n- Dead, redundant, or non-idiomatic code",
    }
}

pub fn build_diff_prompt(review_context: &str, files: &[FileDiff], focus: ReviewFocus) -> String {
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
        "{review_context}\n\n{knowledge_label}:\n{knowledge}\n\nFiles in this batch:\n{file_list}\n\n--- Diff ---\n{diff}\n\nAnalyze only this batch for {focus} findings. Use read_file when surrounding context is needed. Output only the JSON object.",
        knowledge_label = knowledge_label_for_focus(focus),
        knowledge = knowledge_for_focus(focus)
    )
}

pub fn build_scan_prompt<'a>(
    files: impl IntoIterator<Item = &'a str>,
    focus: ReviewFocus,
) -> String {
    let file_list = files
        .into_iter()
        .map(|file| format!("- {file}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "You are reviewing an entire repository for {focus} findings.\n\n{knowledge_label}:\n{knowledge}\n\nHere are the files to review:\n{file_list}\n\n{scan_guidance}\n\nUse read_file to examine each file. For each credible finding, output a structured JSON object. Output only the JSON object.",
        knowledge_label = knowledge_label_for_focus(focus),
        knowledge = knowledge_for_focus(focus),
        scan_guidance = scan_guidance_for_focus(focus),
    )
}

pub async fn run_detector(
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: &ActiveWorkspaceContext,
    prompt: &str,
    focus: ReviewFocus,
    on_tool_event: Option<&dyn ToolEventObserver>,
) -> Result<String, ReviewAgentError> {
    super::run_workspace_agent(AgentConfig {
        provider,
        model,
        api_key,
        workspace,
        role: AgentRole::ReviewDetector,
        preamble: preamble_for_focus(focus),
        prompt,
        on_tool_event,
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn preamble_for_focus_returns_distinct_preambles() {
        let preambles = [
            ReviewFocus::Security,
            ReviewFocus::BugHunt,
            ReviewFocus::Architecture,
            ReviewFocus::Performance,
            ReviewFocus::Style,
        ]
        .into_iter()
        .map(preamble_for_focus)
        .collect::<BTreeSet<_>>();

        assert_eq!(preambles.len(), 5);
    }

    #[test]
    fn build_diff_prompt_uses_focus_knowledge() {
        let prompt = build_diff_prompt("Context", &[], ReviewFocus::Performance);

        assert!(prompt.contains("Performance patterns:"));
        assert!(prompt.contains("performance/unbounded-growth"));
    }
}
