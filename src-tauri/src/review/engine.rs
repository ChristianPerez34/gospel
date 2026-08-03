use std::path::PathBuf;
use std::sync::Arc;
use crate::review::multi::run_multi_focus_review;
use crate::review::{
    run_review, ReviewConfig, ReviewFocus, ReviewMode, ReviewProgressEmitter, ReviewResult,
};

/// The primary entry point for code review execution.
///
/// `ReviewEngine` encapsulates single-focus and multi-focus code reviews behind
/// a single interface (`execute`), hiding diff chunking, prompt assembly,
/// LLM streaming, anti-pattern filtering, and comment validation.
#[derive(Debug, Clone, Default)]
pub struct ReviewEngine;

/// Request payload supplied to `ReviewEngine::execute`.
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

    /// Executes a code review for the given `ReviewRequest`.
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

        let mode_str = match &request.mode {
            ReviewMode::Local => "local".to_string(),
            ReviewMode::PullRequest { .. } => "pr".to_string(),
            ReviewMode::FullScan => "scan".to_string(),
        };

        let pr_number = match request.mode {
            ReviewMode::PullRequest { pr_number } => Some(pr_number),
            _ => None,
        };

        if request.focuses.len() == 1 {
            let config = ReviewConfig {
                provider: request.provider,
                model: request.model,
                mode: mode_str,
                focus: request.focuses[0],
                pr_number,
            };
            run_review(config, request.workspace_path, request.api_key, progress).await
        } else {
            let multi_result = run_multi_focus_review(
                request.provider,
                request.model,
                mode_str,
                pr_number,
                &request.focuses,
                request.workspace_path,
                request.api_key,
                progress,
            )
            .await?;

            // Flatten multi review result into a primary ReviewResult
            if let Some(first_result) = multi_result.results.first() {
                let mut aggregated = first_result.clone();
                let mut all_comments = Vec::new();
                let mut total_scanned = 0;
                let mut total_suppressed = 0;

                for res in &multi_result.results {
                    all_comments.extend(res.comments.clone());
                    total_scanned += res.files_scanned;
                    total_suppressed += res.suppressed_count;
                }

                aggregated.comments = all_comments;
                aggregated.files_scanned = total_scanned;
                aggregated.suppressed_count = total_suppressed;
                aggregated.summary = multi_result.summary;
                Ok(aggregated)
            } else {
                Err("Multi-focus review produced no results".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::NoopReviewProgressEmitter;

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
            .execute(request, Arc::new(NoopReviewProgressEmitter))
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "At least one ReviewFocus must be specified in ReviewRequest"
        );
    }
}

