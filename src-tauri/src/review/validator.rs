use super::{knowledge, AgentConfig, ReviewAgentError, ReviewComment, ReviewFocus};
use crate::llm::{WorkspaceToolContext, AGENT_MAX_TURNS};
use std::time::Duration;

const VALIDATOR_TIMEOUT: Duration = Duration::from_secs(30);

const VALIDATOR_BASE_PREAMBLE: &str = r#"
You are the Gospel Validator Agent.

Validate detector candidates against the appropriate domain knowledge and the workspace source. Keep only findings that are concrete, actionable, and supported by the supplied evidence or by live read_file context. Remove duplicates and downgrade severity when the evidence is weaker than the detector claimed.

Suggest a "signal_tier" for each retained finding:
- "tier_1": critical, high-confidence issues that should interrupt the user immediately.
- "tier_2": important actionable issues with concrete impact.
- "noise": non-actionable, speculative, formatting-only, personal preference, or low-value comments.
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

pub fn preamble_for_focus(focus: ReviewFocus) -> String {
    format!(
        "{base}\n\nCurrent review focus: {focus}. {guidance}",
        base = VALIDATOR_BASE_PREAMBLE,
        guidance = focus_validation_guidance(focus),
    )
}

fn focus_validation_guidance(focus: ReviewFocus) -> &'static str {
    match focus {
        ReviewFocus::Security => "Validate against CWE knowledge. Keep only findings with concrete exploit or exposure evidence.",
        ReviewFocus::BugHunt => "Validate against correctness invariants. Keep only findings where the bug is reproducible with a concrete input, state sequence, or error path.",
        ReviewFocus::Architecture => "Validate against module boundary, dependency direction, and interface-depth principles. Keep only findings where the violation is structural, not stylistic.",
        ReviewFocus::Performance => "Validate against algorithmic, allocation, memory, and I/O complexity principles. Keep only findings with plausible measurable impact, not micro-optimizations.",
        ReviewFocus::Style => "Validate against project conventions and language idiom. Keep only findings that improve clarity or maintainability, not personal preference.",
    }
}

fn knowledge_for_focus(focus: ReviewFocus) -> &'static str {
    match focus {
        ReviewFocus::Security => knowledge::CWE_ENTRIES,
        ReviewFocus::BugHunt => knowledge::BUG_PATTERNS,
        ReviewFocus::Architecture => knowledge::ARCHITECTURE_PATTERNS,
        ReviewFocus::Performance => knowledge::PERFORMANCE_PATTERNS,
        ReviewFocus::Style => knowledge::STYLE_RULES,
    }
}

fn knowledge_label_for_focus(focus: ReviewFocus) -> &'static str {
    match focus {
        ReviewFocus::Security => "CWE knowledge",
        ReviewFocus::BugHunt => "Bug pattern knowledge",
        ReviewFocus::Architecture => "Architecture knowledge",
        ReviewFocus::Performance => "Performance knowledge",
        ReviewFocus::Style => "Style knowledge",
    }
}

pub fn build_validator_prompt(
    candidates: &[ReviewComment],
    focus: ReviewFocus,
) -> Result<String, serde_json::Error> {
    let candidates_json = serde_json::to_string_pretty(candidates)?;
    Ok(format!(
        "{knowledge_label}:\n{knowledge}\n\nDetector candidates:\n{candidates_json}\n\nValidate each candidate for the {focus} focus. Use read_file only when you need source context to confirm or reject a candidate. Output only the JSON object.",
        knowledge_label = knowledge_label_for_focus(focus),
        knowledge = knowledge_for_focus(focus),
    ))
}

pub async fn run_validator(
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: &WorkspaceToolContext,
    prompt: &str,
    focus: ReviewFocus,
) -> Result<String, ReviewAgentError> {
    let preamble = preamble_for_focus(focus);
    super::run_workspace_agent(AgentConfig {
        provider,
        model,
        api_key,
        workspace,
        preamble: preamble.as_str(),
        prompt,
        timeout: VALIDATOR_TIMEOUT,
        max_turns: AGENT_MAX_TURNS,
        on_tool_event: None,
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validator_focus_guidance_covers_all_focuses() {
        for focus in [
            ReviewFocus::Security,
            ReviewFocus::BugHunt,
            ReviewFocus::Architecture,
            ReviewFocus::Performance,
            ReviewFocus::Style,
        ] {
            let preamble = preamble_for_focus(focus);
            assert!(preamble.contains(&format!("Current review focus: {focus}")));
        }
    }

    #[test]
    fn build_validator_prompt_uses_focus_knowledge() {
        let prompt = build_validator_prompt(&[], ReviewFocus::Architecture).unwrap();

        assert!(prompt.contains("Architecture knowledge:"));
        assert!(prompt.contains("architecture/dependency-direction"));
    }
}
