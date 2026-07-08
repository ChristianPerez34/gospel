use super::{run_review, NoopReviewProgressEmitter, ReviewConfig, ReviewFocus, ReviewResult};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::task::JoinSet;

pub const ALL_FOCUSES: &[ReviewFocus] = &[
    ReviewFocus::Security,
    ReviewFocus::BugHunt,
    ReviewFocus::Architecture,
    ReviewFocus::Performance,
    ReviewFocus::Style,
];

const MULTI_REVIEW_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MultiReviewResult {
    pub results: Vec<ReviewResult>,
    pub errors: BTreeMap<String, String>,
    pub summary: String,
    pub files_scanned: usize,
    pub total_findings: usize,
    pub total_suppressed: usize,
}

pub async fn run_multi_focus_review(
    provider: String,
    model: String,
    mode: String,
    pr_number: Option<u64>,
    focuses: &[ReviewFocus],
    workspace_path: PathBuf,
    api_key: String,
) -> Result<MultiReviewResult, String> {
    if focuses.is_empty() {
        return Err("At least one review focus is required.".to_string());
    }

    let mut join_set = JoinSet::new();

    for &focus in focuses {
        let config = ReviewConfig {
            provider: provider.clone(),
            model: model.clone(),
            mode: mode.clone(),
            focus,
            pr_number,
        };
        let workspace_path = workspace_path.clone();
        let api_key = api_key.clone();
        join_set.spawn(async move {
            let result =
                run_review(config, workspace_path, api_key, &NoopReviewProgressEmitter).await;
            (focus, result)
        });
    }

    let mut results = Vec::new();
    let mut errors = BTreeMap::new();
    let deadline = tokio::time::Instant::now() + MULTI_REVIEW_TIMEOUT;

    while let Ok(Some(outcome)) = tokio::time::timeout_at(deadline, join_set.join_next()).await {
        match outcome {
            Ok((_, Ok(result))) => results.push(result),
            Ok((focus, Err(error))) => {
                errors.insert(focus.to_string(), error);
            }
            Err(join_error) => {
                errors.insert("unknown".to_string(), join_error.to_string());
            }
        }
    }

    let timed_out = !join_set.is_empty();
    if timed_out {
        join_set.abort_all();
        errors.insert(
            "timeout".to_string(),
            "Multi-focus review timed out.".to_string(),
        );
    }

    results.sort_by_key(|result| focus_order(result.focus));

    if results.is_empty() {
        return Err(format_all_failed(&errors));
    }

    let total_findings = results.iter().map(|result| result.comments.len()).sum();
    let total_suppressed = results.iter().map(|result| result.suppressed_count).sum();
    let files_scanned = results
        .iter()
        .map(|result| result.files_scanned)
        .max()
        .unwrap_or(0);
    let summary = build_multi_summary(&results, &errors);

    Ok(MultiReviewResult {
        results,
        errors,
        summary,
        files_scanned,
        total_findings,
        total_suppressed,
    })
}

fn focus_order(focus: ReviewFocus) -> usize {
    ALL_FOCUSES
        .iter()
        .position(|candidate| *candidate == focus)
        .unwrap_or(ALL_FOCUSES.len())
}

fn build_multi_summary(results: &[ReviewResult], errors: &BTreeMap<String, String>) -> String {
    let mut parts = results
        .iter()
        .map(|result| {
            format!(
                "{}: {} finding{}",
                result.focus,
                result.comments.len(),
                if result.comments.len() == 1 { "" } else { "s" }
            )
        })
        .collect::<Vec<_>>();

    parts.extend(errors.keys().map(|focus| format!("{focus}: failed")));
    parts.join(" · ")
}

fn format_all_failed(errors: &BTreeMap<String, String>) -> String {
    if errors.is_empty() {
        return "All review focuses failed.".to_string();
    }

    format!(
        "All review focuses failed:\n{}",
        errors
            .iter()
            .map(|(focus, error)| format!("  {focus}: {error}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{ReviewMode, Severity, SignalTier};

    fn result(focus: ReviewFocus, comments: usize, suppressed_count: usize) -> ReviewResult {
        ReviewResult {
            run_id: format!("run-{focus}"),
            focus,
            comments: (0..comments)
                .map(|index| crate::review::ReviewComment {
                    comment_id: format!("{focus}-{index}"),
                    file: "src/main.rs".to_string(),
                    line_start: 1,
                    line_end: 1,
                    severity: Severity::Info,
                    category: "test".to_string(),
                    focus,
                    focus_subcategory: None,
                    cwe_id: None,
                    cwe_name: None,
                    title: "Test finding".to_string(),
                    description: "Test description".to_string(),
                    rationale: Some("Test rationale".to_string()),
                    evidence: "Test evidence".to_string(),
                    suggestion: Some("Test suggestion".to_string()),
                    verification_plan: Some("Test verification".to_string()),
                    signal_tier: SignalTier::Tier2,
                })
                .collect(),
            summary: "summary".to_string(),
            validated: true,
            warnings: Vec::new(),
            files_scanned: 2,
            mode: ReviewMode::Local,
            suppressed_count,
            snr_percent: 100.0,
            user_visible: true,
        }
    }

    #[test]
    fn multi_review_summary_includes_successes_and_failures() {
        let mut errors = BTreeMap::new();
        errors.insert("Performance".to_string(), "provider failed".to_string());

        let summary = build_multi_summary(
            &[
                result(ReviewFocus::Security, 1, 0),
                result(ReviewFocus::BugHunt, 2, 1),
            ],
            &errors,
        );

        assert_eq!(
            summary,
            "Security: 1 finding · BugHunt: 2 findings · Performance: failed"
        );
    }

    #[test]
    fn all_failed_error_lists_each_focus() {
        let mut errors = BTreeMap::new();
        errors.insert("Security".to_string(), "bad key".to_string());
        errors.insert("Style".to_string(), "timeout".to_string());

        let message = format_all_failed(&errors);

        assert!(message.contains("All review focuses failed"));
        assert!(message.contains("Security: bad key"));
        assert!(message.contains("Style: timeout"));
    }
}
