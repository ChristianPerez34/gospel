use std::path::PathBuf;
use std::sync::Arc;
use crate::review::multi::{run_multi_focus_review, MultiReviewResult};
use crate::review::{
    run_review, ReviewConfig, ReviewFocus, ReviewMode, ReviewProgressEmitter, ReviewResult,
};

/// The primary entry point for code review execution.
///
/// `ReviewEngine` encapsulates single-focus and multi-focus code reviews behind
/// a clean seam (`execute` and `execute_multi`), hiding diff chunking, prompt assembly,
/// LLM streaming, anti-pattern filtering, and comment validation.
#[derive(Debug, Clone, Default)]
pub struct ReviewEngine;

/// Request payload supplied to `ReviewEngine::execute` and `ReviewEngine::execute_multi`.
#[derive(Debug, Clone)]
pub struct ReviewRequest {
    pub workspace_path: PathBuf,
    pub mode: ReviewMode,
    pub focuses: Vec<ReviewFocus>,
    pub provider: String,
    pub model: String,
    pub api_key: String,
}

impl ReviewRequest {
    pub fn new(
        workspace_path: PathBuf,
        mode: ReviewMode,
        focuses: Vec<ReviewFocus>,
        provider: String,
        model: String,
        api_key: String,
    ) -> Self {
        Self {
            workspace_path,
            mode,
            focuses,
            provider,
            model,
            api_key,
        }
    }
}

impl ReviewEngine {
    pub fn new() -> Self {
        Self
    }

    /// Executes a code review for the given `ReviewRequest`, returning a single aggregated `ReviewResult`.
    ///
    /// If `request.focuses` contains a single focus, runs a single-focus review pipeline.
    /// If `request.focuses` contains multiple focuses, runs a multi-focus review sweep in parallel,
    /// aggregating results and multiplexing progress events through `progress`.
    pub async fn execute(
        &self,
        request: ReviewRequest,
        progress: Arc<dyn ReviewProgressEmitter>,
    ) -> Result<ReviewResult, String> {
        if request.focuses.is_empty() {
            return Err("At least one ReviewFocus must be specified in ReviewRequest".to_string());
        }

        if request.focuses.len() == 1 {
            let (mode_str, pr_number) = mode_parts(&request.mode);
            let config = ReviewConfig {
                provider: request.provider,
                model: request.model,
                mode: mode_str,
                focus: request.focuses[0],
                pr_number,
            };
            run_review(config, request.workspace_path, request.api_key, progress).await
        } else {
            let multi_result = self.execute_multi(request, progress).await?;
            flatten_multi_review_result(multi_result)
        }
    }

    /// Executes a multi-focus code review for the given `ReviewRequest`, returning `MultiReviewResult`.
    pub async fn execute_multi(
        &self,
        request: ReviewRequest,
        progress: Arc<dyn ReviewProgressEmitter>,
    ) -> Result<MultiReviewResult, String> {
        if request.focuses.is_empty() {
            return Err("At least one ReviewFocus must be specified in ReviewRequest".to_string());
        }

        let (mode_str, pr_number) = mode_parts(&request.mode);
        run_multi_focus_review(
            request.provider,
            request.model,
            mode_str,
            pr_number,
            &request.focuses,
            request.workspace_path,
            request.api_key,
            progress,
        )
        .await
    }
}

fn mode_parts(mode: &ReviewMode) -> (String, Option<u64>) {
    match mode {
        ReviewMode::Local => ("local".to_string(), None),
        ReviewMode::PullRequest { pr_number } => ("pr".to_string(), Some(*pr_number)),
        ReviewMode::FullScan => ("scan".to_string(), None),
    }
}

fn flatten_multi_review_result(multi_result: MultiReviewResult) -> Result<ReviewResult, String> {
    if let Some(first_result) = multi_result.results.first() {
        let mut aggregated = first_result.clone();
        let mut all_comments = Vec::new();
        let mut all_warnings = Vec::new();

        for res in &multi_result.results {
            all_comments.extend(res.comments.clone());
            all_warnings.extend(res.warnings.clone());
        }

        for (focus, err_msg) in &multi_result.errors {
            all_warnings.push(format!("{focus} review failed: {err_msg}"));
        }

        let total_comments = all_comments.len();
        let total_suppressed = multi_result.total_suppressed;
        let total_items = total_comments + total_suppressed;

        aggregated.comments = all_comments;
        aggregated.warnings = all_warnings;
        aggregated.files_scanned = multi_result.files_scanned;
        aggregated.suppressed_count = total_suppressed;
        aggregated.snr_percent = if total_items > 0 {
            (total_comments as f64 / total_items as f64) * 100.0
        } else {
            100.0
        };
        aggregated.summary = multi_result.summary;
        aggregated.validated = multi_result.results.iter().all(|r| r.validated);
        aggregated.user_visible = multi_result.results.iter().any(|r| r.user_visible);

        Ok(aggregated)
    } else {
        if !multi_result.errors.is_empty() {
            let err_summary = multi_result
                .errors
                .iter()
                .map(|(f, e)| format!("{f}: {e}"))
                .collect::<Vec<_>>()
                .join("; ");
            Err(format!("Multi-focus review produced no results. Errors: {err_summary}"))
        } else {
            Err("Multi-focus review produced no results".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use crate::review::{NoopReviewProgressEmitter, ReviewComment, Severity, SignalTier};

    #[tokio::test]
    async fn engine_rejects_empty_focuses_list() {
        let engine = ReviewEngine::new();
        let request = ReviewRequest::new(
            PathBuf::from("/tmp"),
            ReviewMode::Local,
            vec![],
            "openai".to_string(),
            "gpt-4o".to_string(),
            "key".to_string(),
        );

        let result = engine
            .execute(request.clone(), Arc::new(NoopReviewProgressEmitter))
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "At least one ReviewFocus must be specified in ReviewRequest"
        );

        let multi_result = engine
            .execute_multi(request, Arc::new(NoopReviewProgressEmitter))
            .await;
        assert!(multi_result.is_err());
    }

    #[test]
    fn mode_parts_maps_modes_correctly() {
        assert_eq!(mode_parts(&ReviewMode::Local), ("local".to_string(), None));
        assert_eq!(
            mode_parts(&ReviewMode::PullRequest { pr_number: 42 }),
            ("pr".to_string(), Some(42))
        );
        assert_eq!(mode_parts(&ReviewMode::FullScan), ("scan".to_string(), None));
    }

    fn sample_comment(id: &str, focus: ReviewFocus) -> ReviewComment {
        ReviewComment {
            comment_id: id.to_string(),
            file: "src/lib.rs".to_string(),
            line_start: 1,
            line_end: 5,
            severity: Severity::Medium,
            category: "logic".to_string(),
            focus,
            focus_subcategory: None,
            cwe_id: None,
            cwe_name: None,
            title: "Test finding".to_string(),
            description: "Desc".to_string(),
            rationale: None,
            evidence: "Ev".to_string(),
            suggestion: None,
            verification_plan: None,
            signal_tier: SignalTier::Tier1,
        }
    }

    #[test]
    fn flatten_multi_review_result_aggregates_correctly() {
        let res1 = ReviewResult {
            run_id: "run-1".to_string(),
            focus: ReviewFocus::Security,
            comments: vec![sample_comment("c1", ReviewFocus::Security)],
            summary: "Security summary".to_string(),
            validated: true,
            warnings: vec!["warn1".to_string()],
            files_scanned: 10,
            mode: ReviewMode::Local,
            suppressed_count: 1,
            snr_percent: 50.0,
            user_visible: true,
        };

        let res2 = ReviewResult {
            run_id: "run-2".to_string(),
            focus: ReviewFocus::BugHunt,
            comments: vec![sample_comment("c2", ReviewFocus::BugHunt)],
            summary: "BugHunt summary".to_string(),
            validated: true,
            warnings: vec!["warn2".to_string()],
            files_scanned: 10,
            mode: ReviewMode::Local,
            suppressed_count: 0,
            snr_percent: 100.0,
            user_visible: true,
        };

        let mut errors = BTreeMap::new();
        errors.insert("Performance".to_string(), "timeout".to_string());

        let multi = MultiReviewResult {
            results: vec![res1, res2],
            errors,
            summary: "Security: 1 finding · BugHunt: 1 finding · Performance: failed".to_string(),
            files_scanned: 10,
            total_findings: 2,
            total_suppressed: 1,
        };

        let flattened = flatten_multi_review_result(multi).unwrap();

        assert_eq!(flattened.comments.len(), 2);
        assert_eq!(flattened.files_scanned, 10);
        assert_eq!(flattened.suppressed_count, 1);
        // 2 comments / (2 comments + 1 suppressed) = 66.666...%
        assert!((flattened.snr_percent - 66.6666).abs() < 0.01);
        assert_eq!(
            flattened.warnings,
            vec!["warn1", "warn2", "Performance review failed: timeout"]
        );
        assert!(flattened.validated);
        assert!(flattened.user_visible);
        assert_eq!(
            flattened.summary,
            "Security: 1 finding · BugHunt: 1 finding · Performance: failed"
        );
    }
}

