pub mod analytics;
pub mod anti_pattern;
pub mod config;
pub mod detector;
pub mod knowledge;
pub mod multi;
pub mod outcome;
pub mod progress;
pub mod signal;
pub mod tools;
pub mod validator;

pub use progress::{
    ChunkStatus, MultiFocusStatus, NoopReviewProgressEmitter, PhaseStatus, ReviewPhase,
    ReviewProgressEmitter, ReviewProgressEvent, ToolEventKind,
};

use crate::keychain;
use crate::llm::WorkspaceToolContext;
use crate::models::ModelRegistry;
use crate::provider_client::provider_client;
use crate::workspace_tools::build_base_workspace_tools;
use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::streaming::{StreamedAssistantContent, StreamingPrompt};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tokio::process::Command;
use tokio::time::timeout;
use uuid::Uuid;

pub use outcome::{ReviewOutcome, ReviewOutcomeOutput};
pub use signal::SignalTier;

const MAX_FILES_PER_INVOCATION: usize = 5;
const MAX_DIFF_FILES_REVIEWED: usize = 20;
const MAX_SCAN_INVOCATIONS: usize = 20;
const MAX_LINES_PER_INVOCATION: usize = 500;
const SCAN_FILE_BYTES_CAP: u64 = 256 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const UNPARSEABLE_REVIEW_JSON_WARNING: &str = "did not contain parseable review JSON";

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewConfig {
    pub provider: String,
    pub model: String,
    pub mode: String,
    #[serde(default)]
    pub focus: ReviewFocus,
    #[serde(default, alias = "prNumber")]
    pub pr_number: Option<u64>,
}

impl ReviewConfig {
    fn review_mode(&self) -> Result<ReviewMode, String> {
        match self.mode.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(ReviewMode::Local),
            "pr" | "pull_request" | "pull-request" => {
                let pr_number = self
                    .pr_number
                    .ok_or_else(|| "pr_number is required when mode is \"pr\"".to_string())?;
                Ok(ReviewMode::PullRequest { pr_number })
            }
            "scan" | "full_scan" | "full-scan" => Ok(ReviewMode::FullScan),
            other => Err(format!(
                "Unsupported review mode \"{}\". Expected local, pr, or scan.",
                other
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReviewMode {
    Local,
    PullRequest { pr_number: u64 },
    FullScan,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "PascalCase")]
pub enum ReviewFocus {
    #[serde(alias = "security", alias = "SECURITY")]
    Security,
    #[serde(alias = "bug_hunt", alias = "bughunt", alias = "Bug Hunt")]
    BugHunt,
    #[serde(alias = "architecture", alias = "ARCHITECTURE")]
    Architecture,
    #[serde(alias = "performance", alias = "PERFORMANCE")]
    Performance,
    #[serde(alias = "style", alias = "STYLE")]
    Style,
}

impl Default for ReviewFocus {
    fn default() -> Self {
        Self::Security
    }
}

impl fmt::Display for ReviewFocus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Security => "Security",
            Self::BugHunt => "BugHunt",
            Self::Architecture => "Architecture",
            Self::Performance => "Performance",
            Self::Style => "Style",
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum Severity {
    #[serde(alias = "critical", alias = "CRITICAL")]
    Critical,
    #[serde(alias = "high", alias = "HIGH")]
    High,
    #[serde(alias = "medium", alias = "MEDIUM")]
    Medium,
    #[serde(alias = "low", alias = "LOW")]
    Low,
    #[serde(alias = "info", alias = "INFO", alias = "Informational")]
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewComment {
    #[serde(default)]
    pub comment_id: String,
    pub file: String,
    pub line_start: usize,
    pub line_end: usize,
    pub severity: Severity,
    pub category: String,
    #[serde(default)]
    pub focus: ReviewFocus,
    #[serde(default)]
    pub focus_subcategory: Option<String>,
    pub cwe_id: Option<String>,
    pub cwe_name: Option<String>,
    pub title: String,
    pub description: String,
    pub rationale: Option<String>,
    pub evidence: String,
    pub suggestion: Option<String>,
    pub verification_plan: Option<String>,
    #[serde(default)]
    pub signal_tier: SignalTier,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewResult {
    pub run_id: String,
    pub focus: ReviewFocus,
    pub comments: Vec<ReviewComment>,
    pub summary: String,
    pub validated: bool,
    pub warnings: Vec<String>,
    pub files_scanned: usize,
    pub mode: ReviewMode,
    pub suppressed_count: usize,
    pub snr_percent: f64,
    pub user_visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub file: String,
    pub diff: String,
    pub line_count: usize,
    pub is_binary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScanFile {
    path: String,
    line_count: usize,
}

#[derive(Debug)]
pub(crate) struct AgentParseResult {
    comments: Vec<ReviewComment>,
    summary: Option<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub(crate) enum ReviewAgentError {
    #[error("agent timed out")]
    Timeout,
    #[error("{0}")]
    Provider(String),
}

/// One detector invocation that did not produce a parseable result.
///
/// Collected per chunk/batch so the total-failure path can surface the real
/// inner reasons (timeout vs provider error) to the user and to on-disk
/// diagnostics, instead of the opaque "All detector invocations failed".
#[derive(Debug, Clone)]
struct DetectorFailure {
    /// 1-indexed chunk/batch number, matching the warning text.
    index: usize,
    kind: DetectorFailureKind,
    detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetectorFailureKind {
    Timeout,
    Provider,
}

impl DetectorFailureKind {
    fn as_label(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Provider => "provider_error",
        }
    }
}

impl DetectorFailure {
    fn from_error(index: usize, error: &ReviewAgentError) -> Self {
        match error {
            ReviewAgentError::Timeout => Self {
                index,
                kind: DetectorFailureKind::Timeout,
                detail: "agent timed out".to_string(),
            },
            ReviewAgentError::Provider(message) => Self {
                index,
                kind: DetectorFailureKind::Provider,
                detail: sanitize_failure_detail(message),
            },
        }
    }

    fn warning_line(&self, scope: &str) -> String {
        format!(
            "Detector agent {} on {} {}: {}",
            self.kind.as_label(),
            scope,
            self.index,
            self.detail
        )
    }
}

/// Builds the user-visible error string when every detector invocation failed.
///
/// Includes provider/model/mode context plus the per-chunk reasons so the user
/// (and support) can tell timeout storms apart from provider errors without
/// needing logs that the app does not currently surface.
fn all_detector_failures_error(
    provider: &str,
    model: &str,
    mode: &ReviewMode,
    scope: &str,
    focus: ReviewFocus,
    failures: &[DetectorFailure],
) -> String {
    let total = failures.len();
    let timeouts = failures
        .iter()
        .filter(|failure| failure.kind == DetectorFailureKind::Timeout)
        .count();
    let provider_errors = total.saturating_sub(timeouts);

    let mut lines = Vec::with_capacity(failures.len() + 4);
    lines.push(format!(
        "All {total} detector invocations failed for {mode} {focus} review ({provider}/{model}).",
        mode = mode_label(mode),
        focus = focus
    ));
    lines.push(format!(
        "Failures: {timeouts} timeout(s), {provider_errors} provider error(s)."
    ));
    lines.push("Per-chunk reasons:".to_string());
    for failure in failures {
        lines.push(format!(
            "  {scope} {}: [{}] {}",
            failure.index,
            failure.kind.as_label(),
            failure.detail
        ));
    }
    lines.push(
        "If timeouts dominate, try a smaller PR or run a Local review on a subset of the diff. \
         If provider errors dominate, verify the provider session and model name."
            .to_string(),
    );
    lines.join("\n")
}

/// Redacts and truncates provider error text before it is surfaced to the
/// user, logged as a warning, or persisted to `.gospel/review_failures.jsonl`.
///
/// Provider errors can include headers, URLs, tokens, or other sensitive
/// transport details, so we never persist the raw message. We keep the first
/// line and cap the length to bound on-disk and UI size.
fn sanitize_failure_detail(raw: &str) -> String {
    const MAX_LEN: usize = 500;
    let first_line = raw.lines().next().unwrap_or("").trim();
    if first_line.len() <= MAX_LEN {
        first_line.to_string()
    } else {
        // Truncate on a character boundary to avoid panicking on UTF-8.
        let mut end = MAX_LEN;
        while !first_line.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &first_line[..end])
    }
}

/// Formats a whole-run failed progress detail for the UI.
///
/// Raw provider errors still collapse to the first line through
/// [`sanitize_failure_detail`]. The all-detector-failures path is assembled
/// from already-sanitized per-chunk details, so preserve its multi-line
/// summary to keep the progress feed diagnosable.
fn progress_failure_detail(raw: &str) -> String {
    let trimmed = raw.trim();
    if is_detector_failure_summary(trimmed) {
        truncate_failure_detail(trimmed, 2_000)
    } else {
        sanitize_failure_detail(raw)
    }
}

fn is_detector_failure_summary(detail: &str) -> bool {
    detail.starts_with("All ")
        && detail.contains(" detector invocations failed ")
        && detail.contains("\nPer-chunk reasons:\n")
}

fn truncate_failure_detail(raw: &str, max_len: usize) -> String {
    if raw.len() <= max_len {
        return raw.to_string();
    }

    let mut end = max_len;
    while !raw.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &raw[..end])
}

fn mode_label(mode: &ReviewMode) -> &'static str {
    match mode {
        ReviewMode::Local => "local",
        ReviewMode::PullRequest { .. } => "pr",
        ReviewMode::FullScan => "scan",
    }
}

/// Persists a total-failure event to `.gospel/review_failures.jsonl`.
///
/// Best-effort: a write error is logged as a warning but never propagated,
/// because the caller is already on an error path returning to the user.
fn persist_review_failure(
    workspace_path: &Path,
    provider: &str,
    model: &str,
    mode: &ReviewMode,
    focus: ReviewFocus,
    failures: &[DetectorFailure],
    files_scanned: usize,
) {
    let record = analytics::ReviewFailureRecord {
        timestamp: chrono::Utc::now(),
        mode: mode.clone(),
        focus,
        provider: provider.to_string(),
        model: model.to_string(),
        files_scanned,
        failed_chunks: failures.len(),
        chunks: failures
            .iter()
            .map(|failure| analytics::ReviewFailureChunk {
                index: failure.index,
                kind: failure.kind.as_label().to_string(),
                detail: failure.detail.clone(),
            })
            .collect(),
    };
    if let Err(error) = analytics::append_review_failure(workspace_path, &record) {
        tracing::warn!(
            error = %error,
            "failed to persist review failure record"
        );
    }
}

#[derive(Debug, Error)]
enum CommandRunError {
    #[error("Timed out running {command}")]
    Timeout { command: String },
    #[error("Failed to run {program}: {error}")]
    Io {
        program: String,
        #[source]
        error: std::io::Error,
    },
}

#[derive(Debug, Deserialize)]
struct PrMetadata {
    title: String,
    body: Option<String>,
}

struct PrDiff {
    title: String,
    body: String,
    diff: String,
}

pub async fn run_review(
    config: ReviewConfig,
    workspace_path: PathBuf,
    api_key: String,
    emitter: &dyn ReviewProgressEmitter,
) -> Result<ReviewResult, String> {
    let run_id = Uuid::new_v4().to_string();
    let mode = config.review_mode()?;
    ensure_provider_session(&config.provider)?;
    let workspace = WorkspaceToolContext {
        workspace_path,
        corpus_available: false,
        session_mode: crate::session_mode::SESSION_MODE_BUILD.to_string(),
    };

    // Run-start handshake so the frontend can key on run_id and render the
    // pipeline immediately, before chunking is known.
    emitter.emit_progress(ReviewProgressEvent::new(
        &run_id,
        ReviewPhase::Detector {
            chunk: 0,
            total_chunks: 0,
            files: Vec::new(),
            candidate_count: 0,
            status: ChunkStatus::Starting,
        },
    ));

    let outcome = match mode.clone() {
        ReviewMode::Local => {
            let diff = get_local_diff(&workspace.workspace_path).await?;
            run_diff_review(
                &run_id,
                emitter,
                &config.provider,
                &config.model,
                &api_key,
                &workspace,
                config.focus,
                ReviewMode::Local,
                format!(
                    "You are reviewing local staged and unstaged changes for {} findings.",
                    config.focus
                ),
                diff,
            )
            .await
        }
        ReviewMode::PullRequest { pr_number } => {
            let pr = get_pr_diff(&workspace.workspace_path, pr_number).await?;
            let context = format!(
                "You are reviewing Pull Request #{} for {} findings.\n\nPR Title: {}\nPR Description: {}\n\nAnalyze the changes for {} issues. Use read_file to examine surrounding context in the repository.",
                pr_number, config.focus, pr.title, pr.body, config.focus
            );
            run_diff_review(
                &run_id,
                emitter,
                &config.provider,
                &config.model,
                &api_key,
                &workspace,
                config.focus,
                ReviewMode::PullRequest { pr_number },
                context,
                pr.diff,
            )
            .await
        }
        ReviewMode::FullScan => {
            run_full_scan_review(
                &run_id,
                emitter,
                &config.provider,
                &config.model,
                &api_key,
                &workspace,
                config.focus,
            )
            .await
        }
    };

    let result = match outcome {
        Ok(result) => result,
        Err(detail) => {
            emitter.emit_progress(ReviewProgressEvent::new(
                &run_id,
                ReviewPhase::Failed {
                    detail: progress_failure_detail(&detail),
                },
            ));
            return Err(detail);
        }
    };

    match finalize_review_result(
        &run_id,
        emitter,
        &config.provider,
        &config.model,
        &workspace.workspace_path,
        result,
    ) {
        Ok(finalized) => {
            emitter.emit_progress(ReviewProgressEvent::new(
                &run_id,
                ReviewPhase::Done {
                    findings: finalized.comments.len(),
                    suppressed: finalized.suppressed_count,
                },
            ));
            Ok(finalized)
        }
        Err(detail) => {
            emitter.emit_progress(ReviewProgressEvent::new(
                &run_id,
                ReviewPhase::Failed {
                    detail: progress_failure_detail(&detail),
                },
            ));
            Err(detail)
        }
    }
}

/// Verifies that an OAuth provider has a usable session before any review work
/// (gh fetch, detector calls) runs. Mirrors the early guard used by model fetch
/// and the chat path so the user sees a precise error instead of an opaque
/// "All detector invocations failed" later.
fn ensure_provider_session(provider: &str) -> Result<(), String> {
    ensure_provider_session_with(provider, keychain::provider_has_credentials)
}

/// Same as [`ensure_provider_session`] but accepts the credential-checking
/// dependency so tests can simulate authenticated/unauthenticated providers
/// without touching the real keychain.
fn ensure_provider_session_with(
    provider: &str,
    has_credentials: fn(&str) -> bool,
) -> Result<(), String> {
    if !ModelRegistry::is_oauth_provider(provider) {
        return Ok(());
    }
    if has_credentials(provider) {
        Ok(())
    } else {
        Err(format!(
            "{} is not authenticated. Sign in via the provider settings before running a review.",
            provider
        ))
    }
}

fn finalize_review_result(
    run_id: &str,
    emitter: &dyn ReviewProgressEmitter,
    provider: &str,
    model: &str,
    workspace_path: &Path,
    mut result: ReviewResult,
) -> Result<ReviewResult, String> {
    emitter.emit_progress(ReviewProgressEvent::new(
        run_id,
        ReviewPhase::Finalize {
            status: PhaseStatus::Running,
        },
    ));

    let mut warnings = Vec::new();
    let review_config =
        config::load_workspace_review_config_with_warnings(workspace_path, &mut warnings);
    result.warnings.extend(warnings);

    let timestamp = chrono::Utc::now();
    for comment in &mut result.comments {
        comment.focus = result.focus;
        signal::normalize_review_comment(comment, &review_config.signal_rules);
    }

    let original_comments = result.comments.clone();
    let total_count = original_comments.len();
    let snr_percent = signal::snr_percent(&original_comments);
    let below_threshold = total_count > 0 && snr_percent < review_config.noise_threshold_percent;

    if below_threshold {
        result.comments = original_comments
            .iter()
            .filter(|comment| signal::is_actionable(comment.signal_tier))
            .cloned()
            .collect();
        result.suppressed_count = total_count.saturating_sub(result.comments.len());
        if result.suppressed_count > 0 {
            result.summary = suppression_summary(total_count, result.comments.len(), snr_percent);
        }
    } else {
        result.suppressed_count = 0;
    }

    result.run_id = run_id.to_string();
    result.snr_percent = snr_percent;
    result.user_visible = total_count == 0 || !result.comments.is_empty();

    let metrics = analytics::ReviewMetricsRecord::from_comments(
        run_id.to_string(),
        timestamp,
        result.mode.clone(),
        result.focus,
        provider.to_string(),
        model.to_string(),
        &original_comments,
        result.files_scanned,
        result.user_visible,
    );
    if let Err(error) = analytics::append_review_metrics(workspace_path, &metrics) {
        result
            .warnings
            .push(format!("Failed to write review metrics: {}", error));
    }

    let run_record = outcome::ReviewRunRecord {
        run_id: run_id.to_string(),
        timestamp,
        focus: result.focus,
        mode: result.mode.clone(),
        comments: original_comments,
    };
    if let Err(error) = outcome::save_review_run(workspace_path, &run_record) {
        result
            .warnings
            .push(format!("Failed to write review run index: {}", error));
    }

    emitter.emit_progress(ReviewProgressEvent::new(
        run_id,
        ReviewPhase::Finalize {
            status: PhaseStatus::Done,
        },
    ));

    Ok(result)
}

pub async fn get_local_diff(workspace_path: &Path) -> Result<String, String> {
    let unstaged = run_git(workspace_path, &["diff"])
        .await
        .map_err(|e| format!("Failed to fetch local diff: {}", e))?;
    let staged = run_git(workspace_path, &["diff", "--cached"])
        .await
        .map_err(|e| format!("Failed to fetch staged diff: {}", e))?;

    Ok([unstaged.trim(), staged.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n"))
}

async fn get_pr_diff(workspace_path: &Path, pr_number: u64) -> Result<PrDiff, String> {
    ensure_gh_available_and_authenticated(workspace_path).await?;

    let pr_arg = pr_number.to_string();
    let diff_args = ["pr", "diff", &pr_arg];
    let meta_args = ["pr", "view", &pr_arg, "--json", "title,body"];
    let diff_fut = run_program(workspace_path, "gh", &diff_args);
    let meta_fut = run_program(workspace_path, "gh", &meta_args);

    let (diff, metadata) = tokio::try_join!(
        async { diff_fut.await.map_err(|e| pr_fetch_error(pr_number, e)) },
        async { meta_fut.await.map_err(|e| pr_fetch_error(pr_number, e)) }
    )?;

    let metadata: PrMetadata = serde_json::from_str(&metadata)
        .map_err(|e| format!("Failed to parse PR metadata for #{}: {}", pr_number, e))?;

    Ok(PrDiff {
        title: metadata.title,
        body: metadata.body.unwrap_or_default(),
        diff,
    })
}

fn filter_rejected_comments(
    comments: Vec<ReviewComment>,
    store: &anti_pattern::AntiPatternStore,
) -> Vec<ReviewComment> {
    comments
        .into_iter()
        .filter(|comment| {
            if store.is_rejected(
                comment.focus,
                &comment.file,
                comment.line_start,
                comment.line_end,
                &comment.title,
            ) {
                tracing::debug!(
                    "Filtering out previously rejected finding: {} in {}",
                    comment.title,
                    comment.file
                );
                false
            } else {
                true
            }
        })
        .collect()
}

fn load_anti_pattern_store_for_review(
    workspace_path: &Path,
    warnings: &mut Vec<String>,
) -> anti_pattern::AntiPatternStore {
    match anti_pattern::AntiPatternStore::load(workspace_path) {
        Ok(store) => store,
        Err(error) => {
            let warning = format!(
                "Ignored rejected findings store because it could not be loaded: {}",
                error
            );
            tracing::warn!("{}", warning);
            warnings.push(warning);
            anti_pattern::AntiPatternStore::default()
        }
    }
}

async fn run_full_scan_review(
    run_id: &str,
    emitter: &dyn ReviewProgressEmitter,
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: &WorkspaceToolContext,
    focus: ReviewFocus,
) -> Result<ReviewResult, String> {
    let mut warnings = Vec::new();
    let files = get_full_scan_files(&workspace.workspace_path).await?;
    if files.is_empty() {
        return Ok(review_result(
            Vec::new(),
            "No tracked files found".to_string(),
            true,
            warnings,
            0,
            focus,
            ReviewMode::FullScan,
        ));
    }

    let total_files = files.len();

    let batches = chunk_scan_files(files, MAX_FILES_PER_INVOCATION, MAX_LINES_PER_INVOCATION);
    let (batches, scan_was_capped) = cap_scan_batches(batches, MAX_SCAN_INVOCATIONS);
    let files_scanned = batches.iter().map(Vec::len).sum();
    if scan_was_capped {
        warnings.push(partial_review_warning(files_scanned, total_files));
    }

    let total_chunks = batches.len();
    let mut candidates = Vec::new();
    let mut failures: Vec<DetectorFailure> = Vec::new();

    let anti_pattern_store =
        load_anti_pattern_store_for_review(&workspace.workspace_path, &mut warnings);

    for (index, batch) in batches.iter().enumerate() {
        let chunk_number = index + 1;
        let batch_files: Vec<String> = batch.iter().map(|file| file.path.clone()).collect();
        emitter.emit_progress(ReviewProgressEvent::new(
            run_id,
            ReviewPhase::Detector {
                chunk: chunk_number,
                total_chunks,
                files: batch_files.clone(),
                candidate_count: candidates.len(),
                status: ChunkStatus::Running,
            },
        ));
        let prompt =
            detector::build_scan_prompt(batch.iter().map(|file| file.path.as_str()), focus);
        match detector::run_detector(provider, model, api_key, workspace, &prompt, focus, None)
            .await
        {
            Ok(output) => {
                let mut parsed = parse_agent_review_output(&output, "Detector");
                stamp_parsed_comments_focus(&mut parsed, focus);
                warnings.extend(parsed.warnings);
                candidates.extend(filter_rejected_comments(
                    parsed.comments,
                    &anti_pattern_store,
                ));
                emitter.emit_progress(ReviewProgressEvent::new(
                    run_id,
                    ReviewPhase::Detector {
                        chunk: chunk_number,
                        total_chunks,
                        files: batch_files,
                        candidate_count: candidates.len(),
                        status: ChunkStatus::Done,
                    },
                ));
            }
            Err(error) => {
                let failure = DetectorFailure::from_error(chunk_number, &error);
                tracing::warn!(
                    provider = provider,
                    model = model,
                    mode = "scan",
                    chunk = chunk_number,
                    kind = failure.kind.as_label(),
                    detail = %failure.detail,
                    "detector invocation failed"
                );
                warnings.push(failure.warning_line("scan batch"));
                emitter.emit_progress(ReviewProgressEvent::new(
                    run_id,
                    ReviewPhase::Detector {
                        chunk: chunk_number,
                        total_chunks,
                        files: batch_files,
                        candidate_count: candidates.len(),
                        status: ChunkStatus::Failed {
                            kind: failure.kind.as_label().to_string(),
                            detail: failure.detail.clone(),
                        },
                    },
                ));
                failures.push(failure);
            }
        }
    }

    if !failures.is_empty() && failures.len() == batches.len() {
        let mode = ReviewMode::FullScan;
        let message = all_detector_failures_error(provider, model, &mode, "scan batch", focus, &failures);
        tracing::error!(
            provider = provider,
            model = model,
            mode = "scan",
            failed_chunks = failures.len(),
            total_chunks = batches.len(),
            "all detector invocations failed"
        );
        persist_review_failure(
            &workspace.workspace_path,
            provider,
            model,
            &mode,
            focus,
            &failures,
            files_scanned,
        );
        return Err(message);
    }

    let (comments, validated, validator_summary, validator_warnings) = validate_candidates(
        run_id,
        emitter,
        provider,
        model,
        api_key,
        workspace,
        &candidates,
        &anti_pattern_store,
        focus,
    )
    .await?;
    warnings.extend(validator_warnings);
    let summary = validator_summary.unwrap_or_else(|| summarize_comments(&comments, validated));

    Ok(review_result(
        comments,
        summary,
        validated,
        warnings,
        files_scanned,
        focus,
        ReviewMode::FullScan,
    ))
}

/// Forwards detector tool-call/result events to the review progress emitter
/// as `DetectorTool` phases so the frontend can show incremental activity
/// ("reading file X") while a chunk is being analyzed.
struct DetectorToolObserver<'a> {
    run_id: &'a str,
    chunk: usize,
    emitter: &'a dyn ReviewProgressEmitter,
}

impl<'a> ToolEventObserver for DetectorToolObserver<'a> {
    fn on_tool_call(&self, name: &str, arguments: &serde_json::Value) {
        self.emitter.emit_progress(ReviewProgressEvent::new(
            self.run_id,
            ReviewPhase::DetectorTool {
                chunk: self.chunk,
                tool_name: name.to_string(),
                event: ToolEventKind::Call {
                    arguments: arguments.clone(),
                },
            },
        ));
    }

    fn on_tool_result(&self, name: &str, result: &str) {
        let summary = truncate_tool_result(result);
        self.emitter.emit_progress(ReviewProgressEvent::new(
            self.run_id,
            ReviewPhase::DetectorTool {
                chunk: self.chunk,
                tool_name: name.to_string(),
                event: ToolEventKind::Result { summary },
            },
        ));
    }
}

/// Caps tool-result summaries forwarded to the frontend so a large file
/// read does not balloon the progress event payload.
fn truncate_tool_result(result: &str) -> String {
    const MAX_RESULT_SUMMARY: usize = 200;
    if result.len() <= MAX_RESULT_SUMMARY {
        return result.to_string();
    }
    let mut end = MAX_RESULT_SUMMARY;
    while !result.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &result[..end])
}

async fn run_diff_review(
    run_id: &str,
    emitter: &dyn ReviewProgressEmitter,
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: &WorkspaceToolContext,
    focus: ReviewFocus,
    mode: ReviewMode,
    review_context: String,
    diff: String,
) -> Result<ReviewResult, String> {
    if diff.trim().is_empty() {
        return Ok(review_result(
            Vec::new(),
            "No changes found to review".to_string(),
            true,
            Vec::new(),
            0,
            focus,
            mode,
        ));
    }

    let mut warnings = Vec::new();
    let file_diffs: Vec<FileDiff> = parse_diff_by_file(&diff)
        .into_iter()
        .filter(|file| !file.is_binary)
        .collect();

    if file_diffs.is_empty() {
        warnings
            .push("Only binary files were present in the diff; nothing was reviewed".to_string());
        return Ok(review_result(
            Vec::new(),
            "No text changes found to review".to_string(),
            true,
            warnings,
            0,
            focus,
            mode,
        ));
    }

    let total_files = file_diffs.len();
    let reviewed_file_diffs: Vec<FileDiff> = if total_files > MAX_DIFF_FILES_REVIEWED {
        warnings.push(partial_review_warning(MAX_DIFF_FILES_REVIEWED, total_files));
        file_diffs
            .into_iter()
            .take(MAX_DIFF_FILES_REVIEWED)
            .collect()
    } else {
        file_diffs
    };
    let files_scanned = reviewed_file_diffs.len();
    let chunks = chunk_files(
        reviewed_file_diffs,
        MAX_FILES_PER_INVOCATION,
        MAX_LINES_PER_INVOCATION,
    );

    let total_chunks = chunks.len();
    let mut candidates = Vec::new();
    let mut failures: Vec<DetectorFailure> = Vec::new();

    let anti_pattern_store =
        load_anti_pattern_store_for_review(&workspace.workspace_path, &mut warnings);

    for (index, chunk) in chunks.iter().enumerate() {
        let chunk_number = index + 1;
        let chunk_files: Vec<String> = chunk.iter().map(|file| file.file.clone()).collect();
        emitter.emit_progress(ReviewProgressEvent::new(
            run_id,
            ReviewPhase::Detector {
                chunk: chunk_number,
                total_chunks,
                files: chunk_files.clone(),
                candidate_count: candidates.len(),
                status: ChunkStatus::Running,
            },
        ));
        let prompt = detector::build_diff_prompt(&review_context, chunk, focus);
        let observer = DetectorToolObserver {
            run_id,
            chunk: chunk_number,
            emitter,
        };
        match detector::run_detector(
            provider,
            model,
            api_key,
            workspace,
            &prompt,
            focus,
            Some(&observer),
        )
        .await
        {
            Ok(output) => {
                let mut parsed = parse_agent_review_output(&output, "Detector");
                stamp_parsed_comments_focus(&mut parsed, focus);
                warnings.extend(parsed.warnings);
                candidates.extend(filter_rejected_comments(
                    parsed.comments,
                    &anti_pattern_store,
                ));
                emitter.emit_progress(ReviewProgressEvent::new(
                    run_id,
                    ReviewPhase::Detector {
                        chunk: chunk_number,
                        total_chunks,
                        files: chunk_files,
                        candidate_count: candidates.len(),
                        status: ChunkStatus::Done,
                    },
                ));
            }
            Err(error) => {
                let failure = DetectorFailure::from_error(chunk_number, &error);
                tracing::warn!(
                    provider = provider,
                    model = model,
                    mode = %mode_label(&mode),
                    chunk = chunk_number,
                    kind = failure.kind.as_label(),
                    detail = %failure.detail,
                    "detector invocation failed"
                );
                warnings.push(failure.warning_line("diff chunk"));
                emitter.emit_progress(ReviewProgressEvent::new(
                    run_id,
                    ReviewPhase::Detector {
                        chunk: chunk_number,
                        total_chunks,
                        files: chunk_files,
                        candidate_count: candidates.len(),
                        status: ChunkStatus::Failed {
                            kind: failure.kind.as_label().to_string(),
                            detail: failure.detail.clone(),
                        },
                    },
                ));
                failures.push(failure);
            }
        }
    }

    if !failures.is_empty() && failures.len() == chunks.len() {
        let message = all_detector_failures_error(provider, model, &mode, "diff chunk", focus, &failures);
        tracing::error!(
            provider = provider,
            model = model,
            mode = %mode_label(&mode),
            failed_chunks = failures.len(),
            total_chunks = chunks.len(),
            "all detector invocations failed"
        );
        persist_review_failure(
            &workspace.workspace_path,
            provider,
            model,
            &mode,
            focus,
            &failures,
            files_scanned,
        );
        return Err(message);
    }

    let (comments, validated, validator_summary, validator_warnings) = validate_candidates(
        run_id,
        emitter,
        provider,
        model,
        api_key,
        workspace,
        &candidates,
        &anti_pattern_store,
        focus,
    )
    .await?;
    warnings.extend(validator_warnings);
    let summary = validator_summary.unwrap_or_else(|| summarize_comments(&comments, validated));

    Ok(review_result(
        comments,
        summary,
        validated,
        warnings,
        files_scanned,
        focus,
        mode,
    ))
}

async fn validate_candidates(
    run_id: &str,
    emitter: &dyn ReviewProgressEmitter,
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: &WorkspaceToolContext,
    candidates: &[ReviewComment],
    anti_pattern_store: &anti_pattern::AntiPatternStore,
    focus: ReviewFocus,
) -> Result<(Vec<ReviewComment>, bool, Option<String>, Vec<String>), String> {
    if candidates.is_empty() {
        emitter.emit_progress(ReviewProgressEvent::new(
            run_id,
            ReviewPhase::Validator {
                candidate_count: 0,
                status: PhaseStatus::Done,
            },
        ));
        return Ok((
            Vec::new(),
            true,
            Some(format!("No {} findings detected.", focus)),
            Vec::new(),
        ));
    }

    emitter.emit_progress(ReviewProgressEvent::new(
        run_id,
        ReviewPhase::Validator {
            candidate_count: candidates.len(),
            status: PhaseStatus::Running,
        },
    ));
    let prompt = validator::build_validator_prompt(candidates, focus)
        .map_err(|e| format!("Failed to build validator prompt: {}", e))?;
    let result =
        match validator::run_validator(provider, model, api_key, workspace, &prompt, focus).await {
            Ok(output) => {
                let mut parsed = parse_agent_review_output(&output, "Validator");
                stamp_parsed_comments_focus(&mut parsed, focus);
                Ok(review_validator_parse_result(
                    parsed,
                    candidates,
                    anti_pattern_store,
                ))
            }
            Err(ReviewAgentError::Timeout) => Ok((
                candidates.to_vec(),
                false,
                None,
                vec!["Validator agent timed out".to_string()],
            )),
            Err(ReviewAgentError::Provider(error)) => Ok((
                candidates.to_vec(),
                false,
                None,
                vec![format!(
                    "Validator agent failed: {}",
                    sanitize_failure_detail(&error)
                )],
            )),
        };
    emitter.emit_progress(ReviewProgressEvent::new(
        run_id,
        ReviewPhase::Validator {
            candidate_count: candidates.len(),
            status: PhaseStatus::Done,
        },
    ));
    result
}

fn stamp_parsed_comments_focus(parsed: &mut AgentParseResult, focus: ReviewFocus) {
    for comment in &mut parsed.comments {
        comment.focus = focus;
    }
}

fn review_validator_parse_result(
    parsed: AgentParseResult,
    candidates: &[ReviewComment],
    anti_pattern_store: &anti_pattern::AntiPatternStore,
) -> (Vec<ReviewComment>, bool, Option<String>, Vec<String>) {
    if is_unparseable_agent_output(&parsed) {
        let mut warnings = parsed.warnings;
        warnings
            .push("Validator output was unparseable; returning detector candidates".to_string());
        return (candidates.to_vec(), false, None, warnings);
    }

    let original_count = parsed.comments.len();
    let comments = filter_rejected_comments(parsed.comments, anti_pattern_store);
    let summary = if comments.len() == original_count {
        parsed.summary
    } else {
        None
    };

    (comments, true, summary, parsed.warnings)
}

fn summarize_comments(comments: &[ReviewComment], validated: bool) -> String {
    if comments.is_empty() {
        return "No findings detected.".to_string();
    }

    let validation = if validated { "validated" } else { "candidate" };
    format!(
        "Found {} {} finding{}.",
        comments.len(),
        validation,
        if comments.len() == 1 { "" } else { "s" }
    )
}

fn suppression_summary(total: usize, visible: usize, snr_percent: f64) -> String {
    let suppressed = total.saturating_sub(visible);
    format!(
        "Found {} potential issue{}. Showing {} actionable finding{} (SNR: {}%). {} low-signal comment{} suppressed.",
        total,
        if total == 1 { "" } else { "s" },
        visible,
        if visible == 1 { "" } else { "s" },
        format_percent(snr_percent),
        suppressed,
        if suppressed == 1 { "" } else { "s" }
    )
}

fn format_percent(value: f64) -> String {
    if (value.fract()).abs() < f64::EPSILON {
        format!("{}", value as usize)
    } else {
        format!("{:.1}", value)
    }
}

fn review_result(
    comments: Vec<ReviewComment>,
    summary: String,
    validated: bool,
    warnings: Vec<String>,
    files_scanned: usize,
    focus: ReviewFocus,
    mode: ReviewMode,
) -> ReviewResult {
    ReviewResult {
        run_id: String::new(),
        focus,
        comments,
        summary,
        validated,
        warnings,
        files_scanned,
        mode,
        suppressed_count: 0,
        snr_percent: 100.0,
        user_visible: true,
    }
}

fn partial_review_warning(reviewed: usize, total: usize) -> String {
    format!(
        "Reviewed {}/{} files — partial review due to size",
        reviewed, total
    )
}

pub fn parse_diff_by_file(diff: &str) -> Vec<FileDiff> {
    let mut files = Vec::new();
    let mut current = String::new();

    for line in diff.lines() {
        if line.starts_with("diff --git ") && !current.is_empty() {
            files.push(file_diff_from_chunk(&current));
            current.clear();
        }

        if line.starts_with("diff --git ") || !current.is_empty() {
            current.push_str(line);
            current.push('\n');
        }
    }

    if !current.is_empty() {
        files.push(file_diff_from_chunk(&current));
    } else if !diff.trim().is_empty() {
        files.push(FileDiff {
            file: "<unknown>".to_string(),
            diff: diff.to_string(),
            line_count: diff.lines().count(),
            is_binary: is_binary_diff(diff),
        });
    }

    files
}

fn file_diff_from_chunk(chunk: &str) -> FileDiff {
    let file = diff_file_name(chunk).unwrap_or_else(|| "<unknown>".to_string());
    FileDiff {
        file,
        diff: chunk.to_string(),
        line_count: chunk.lines().count(),
        is_binary: is_binary_diff(chunk),
    }
}

fn diff_file_name(chunk: &str) -> Option<String> {
    let header = chunk.lines().next()?;
    if let Some(rest) = header.strip_prefix("diff --git ") {
        let mut parts = rest.split_whitespace();
        let _old = parts.next();
        if let Some(new_path) = parts.next() {
            return Some(strip_diff_prefix(new_path));
        }
    }

    chunk.lines().find_map(|line| {
        line.strip_prefix("+++ ")
            .filter(|path| *path != "/dev/null")
            .map(strip_diff_prefix)
    })
}

fn strip_diff_prefix(path: &str) -> String {
    path.trim_matches('"')
        .strip_prefix("b/")
        .or_else(|| path.trim_matches('"').strip_prefix("a/"))
        .unwrap_or_else(|| path.trim_matches('"'))
        .to_string()
}

fn is_binary_diff(diff: &str) -> bool {
    diff.contains("\nBinary files ") || diff.contains("\nGIT binary patch")
}

pub fn chunk_files(files: Vec<FileDiff>, max_files: usize, max_lines: usize) -> Vec<Vec<FileDiff>> {
    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut current_lines = 0usize;

    for file in files {
        let would_exceed_files = !current.is_empty() && current.len() >= max_files;
        let would_exceed_lines = !current.is_empty() && current_lines + file.line_count > max_lines;

        if would_exceed_files || would_exceed_lines {
            chunks.push(current);
            current = Vec::new();
            current_lines = 0;
        }

        current_lines += file.line_count;
        current.push(file);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

fn chunk_scan_files(
    files: Vec<ScanFile>,
    max_files: usize,
    max_lines: usize,
) -> Vec<Vec<ScanFile>> {
    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut current_lines = 0usize;

    for file in files {
        let would_exceed_files = !current.is_empty() && current.len() >= max_files;
        let would_exceed_lines = !current.is_empty() && current_lines + file.line_count > max_lines;
        if would_exceed_files || would_exceed_lines {
            chunks.push(current);
            current = Vec::new();
            current_lines = 0;
        }

        current_lines += file.line_count;
        current.push(file);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

fn cap_scan_batches(
    mut batches: Vec<Vec<ScanFile>>,
    max_batches: usize,
) -> (Vec<Vec<ScanFile>>, bool) {
    let was_capped = batches.len() > max_batches;
    if was_capped {
        batches.truncate(max_batches);
    }
    (batches, was_capped)
}

pub(crate) fn parse_agent_review_output(raw: &str, source: &str) -> AgentParseResult {
    for candidate in json_candidates(raw) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&candidate) {
            if let Some(parsed) = parse_agent_value(value) {
                return parsed;
            }
        }
    }

    AgentParseResult {
        comments: Vec::new(),
        summary: None,
        warnings: vec![format!(
            "{} output {}",
            source, UNPARSEABLE_REVIEW_JSON_WARNING
        )],
    }
}

fn is_unparseable_agent_output(parsed: &AgentParseResult) -> bool {
    parsed.comments.is_empty()
        && parsed.summary.is_none()
        && parsed
            .warnings
            .iter()
            .any(|warning| warning.contains(UNPARSEABLE_REVIEW_JSON_WARNING))
}

fn parse_agent_value(value: serde_json::Value) -> Option<AgentParseResult> {
    if value.is_array() {
        let comments = serde_json::from_value::<Vec<ReviewComment>>(value).ok()?;
        return Some(AgentParseResult {
            comments,
            summary: None,
            warnings: Vec::new(),
        });
    }

    #[derive(Deserialize)]
    struct Envelope {
        #[serde(default)]
        comments: Vec<ReviewComment>,
        summary: Option<String>,
        #[serde(default)]
        warnings: Vec<String>,
    }

    let object = value.as_object()?;
    if object.contains_key("comments") {
        let envelope = serde_json::from_value::<Envelope>(value).ok()?;
        return Some(AgentParseResult {
            comments: envelope.comments,
            summary: envelope.summary,
            warnings: envelope.warnings,
        });
    }

    let comment = serde_json::from_value::<ReviewComment>(value).ok()?;
    Some(AgentParseResult {
        comments: vec![comment],
        summary: None,
        warnings: Vec::new(),
    })
}

fn json_candidates(raw: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let trimmed = raw.trim();
    if !trimmed.is_empty() {
        candidates.push(trimmed.to_string());
    }

    let mut rest = raw;
    while let Some(start) = rest.find("```") {
        let after_fence = &rest[start + 3..];
        let content_start = after_fence.find('\n').map(|index| index + 1).unwrap_or(0);
        let content = &after_fence[content_start..];
        if let Some(end) = content.find("```") {
            candidates.push(content[..end].trim().to_string());
            rest = &content[end + 3..];
        } else {
            break;
        }
    }

    if let (Some(start), Some(end)) = (raw.find('['), raw.rfind(']')) {
        if start < end {
            candidates.push(raw[start..=end].to_string());
        }
    }

    if let (Some(start), Some(end)) = (raw.find('{'), raw.rfind('}')) {
        if start < end {
            candidates.push(raw[start..=end].to_string());
        }
    }

    candidates
}

async fn get_full_scan_files(workspace_path: &Path) -> Result<Vec<ScanFile>, String> {
    let output = run_git(workspace_path, &["ls-files"])
        .await
        .map_err(|e| format!("Failed to list tracked files: {}", e))?;
    let mut files = Vec::new();

    for path in output
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        if should_skip_scan_file(path) {
            continue;
        }

        let absolute_path = workspace_path.join(path);
        let metadata = match tokio::fs::metadata(&absolute_path).await {
            Ok(metadata) if metadata.is_file() && metadata.len() <= SCAN_FILE_BYTES_CAP => metadata,
            _ => continue,
        };
        if metadata.len() == 0 {
            files.push(ScanFile {
                path: path.to_string(),
                line_count: 1,
            });
            continue;
        }

        let bytes = match tokio::fs::read(&absolute_path).await {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if bytes.contains(&0) {
            continue;
        }
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(_) => continue,
        };
        files.push(ScanFile {
            path: path.to_string(),
            line_count: text.lines().count().max(1),
        });
    }

    Ok(files)
}

fn should_skip_scan_file(path: &str) -> bool {
    let path_obj = Path::new(path);
    if path_obj.components().any(|component| {
        let value = component.as_os_str().to_string_lossy();
        matches!(
            value.as_ref(),
            ".git"
                | ".gospel"
                | "node_modules"
                | "target"
                | "dist"
                | "build"
                | "out"
                | "coverage"
                | ".next"
                | ".nuxt"
                | "vendor"
        )
    }) {
        return true;
    }

    let file_name = path_obj
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(
        file_name.as_str(),
        "cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "skills-lock.json"
    ) || file_name.ends_with(".min.js")
        || file_name.ends_with(".map")
    {
        return true;
    }

    let extension = path_obj
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        extension.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "ico"
            | "icns"
            | "pdf"
            | "zip"
            | "gz"
            | "tar"
            | "tgz"
            | "woff"
            | "woff2"
            | "ttf"
            | "eot"
            | "mp4"
            | "mp3"
            | "wav"
            | "wasm"
            | "sqlite"
            | "sqlite3"
            | "db"
            | "exe"
            | "dll"
            | "dylib"
            | "so"
            | "o"
            | "a"
            | "rlib"
            | "class"
            | "jar"
    )
}

async fn ensure_gh_available_and_authenticated(workspace_path: &Path) -> Result<(), String> {
    match run_command_output(workspace_path, "gh", &["--version"]).await {
        Ok(output) if output.status.success() => {}
        Ok(_) => {
            return Err(
                "PR review requires the GitHub CLI (gh). Install from https://cli.github.com/"
                    .to_string(),
            )
        }
        Err(CommandRunError::Io { error, .. }) if error.kind() == ErrorKind::NotFound => {
            return Err(
                "PR review requires the GitHub CLI (gh). Install from https://cli.github.com/"
                    .to_string(),
            )
        }
        Err(error) => return Err(error.to_string()),
    }

    let auth = run_command_output(workspace_path, "gh", &["auth", "status"])
        .await
        .map_err(|e| e.to_string())?;
    if auth.status.success() {
        Ok(())
    } else {
        Err("GitHub CLI is not authenticated. Run gh auth login first.".to_string())
    }
}

fn pr_fetch_error(pr_number: u64, error: String) -> String {
    let lower = error.to_ascii_lowercase();
    if lower.contains("not found")
        || lower.contains("could not resolve")
        || lower.contains("no pull requests found")
    {
        format!("PR #{} not found in this repository", pr_number)
    } else {
        format!("Failed to fetch PR: {}", error)
    }
}

async fn run_git(workspace_path: &Path, args: &[&str]) -> Result<String, String> {
    run_program(workspace_path, "git", args).await
}

async fn run_program(
    workspace_path: &Path,
    program: &str,
    args: &[&str],
) -> Result<String, String> {
    let output = run_command_output(workspace_path, program, args)
        .await
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        Err(if detail.is_empty() {
            format!("{} exited with status {}", program, output.status)
        } else {
            detail
        })
    }
}

async fn run_command_output(
    workspace_path: &Path,
    program: &str,
    args: &[&str],
) -> Result<std::process::Output, CommandRunError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(workspace_path)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let command_label = command_label(program, args);

    let child = command.spawn().map_err(|error| CommandRunError::Io {
        program: program.to_string(),
        error,
    })?;

    match timeout(COMMAND_TIMEOUT, child.wait_with_output()).await {
        Ok(result) => result.map_err(|error| CommandRunError::Io {
            program: program.to_string(),
            error,
        }),
        Err(_) => Err(CommandRunError::Timeout {
            command: command_label,
        }),
    }
}

fn command_label(program: &str, args: &[&str]) -> String {
    if args.is_empty() {
        program.to_string()
    } else {
        format!("{} {}", program, args.join(" "))
    }
}

pub(crate) struct AgentConfig<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub api_key: &'a str,
    pub workspace: &'a WorkspaceToolContext,
    pub preamble: &'a str,
    pub prompt: &'a str,
    pub timeout: Duration,
    pub max_turns: usize,
    /// Optional observer invoked when the agent makes a tool call or receives
    /// a tool result while streaming. Used by the detector to surface
    /// "reading file X" progress to the review progress emitter.
    pub on_tool_event: Option<&'a dyn ToolEventObserver>,
}

/// Observer for tool-call/tool-result events emitted during a streaming
/// agent run. The review pipeline implements this to forward incremental
/// progress to the frontend without coupling `run_workspace_agent` to the
/// progress event types.
pub trait ToolEventObserver: Send + Sync {
    fn on_tool_call(&self, name: &str, arguments: &serde_json::Value);
    fn on_tool_result(&self, name: &str, result: &str);
}

/// Drives a streaming agent run to completion, accumulating the final
/// assistant text and forwarding tool events to the optional observer.
///
/// Uses `stream_prompt` instead of `prompt` because the ChatGPT Codex
/// backend's non-streaming `completion_from_sse` path drops tool calls
/// when `response.completed` arrives with an empty `output` array (which
/// it does whenever the model emits a tool call). The streaming path
/// processes SSE events incrementally and reconstructs tool calls from
/// the streamed deltas, so it is unaffected. See
/// <https://github.com/0xPlaygrounds/rig/issues/2000>.
pub(crate) async fn run_workspace_agent(
    config: AgentConfig<'_>,
) -> Result<String, ReviewAgentError> {
    if !ModelRegistry::is_oauth_provider(config.provider) && config.api_key.trim().is_empty() {
        return Err(ReviewAgentError::Provider(format!(
            "API key not configured for {}",
            config.provider
        )));
    }

    macro_rules! run_from_client {
        ($client:expr, $model:expr) => {{
            let agent_builder = $client
                .agent($model)
                .preamble(config.preamble)
                .default_max_turns(config.max_turns)
                .tools(build_base_workspace_tools(
                    config.workspace.workspace_path.clone(),
                ));
            let agent = agent_builder.build();
            let stream = agent
                .stream_prompt(config.prompt)
                .multi_turn(config.max_turns)
                .await;
            consume_agent_stream(stream, config.timeout, config.on_tool_event).await
        }};
    }

    provider_client!(
        config.provider,
        config.api_key,
        ReviewAgentError::Provider,
        ReviewAgentError::Provider,
        |client| { run_from_client!(client, config.model) }
    )
}

/// Consumes a multi-turn agent stream to completion, returning the final
/// assistant text. Tool calls/results are forwarded to the observer when
/// present. The whole stream is bounded by `deadline` so a stuck agent
/// surfaces as a timeout rather than hanging forever.
async fn consume_agent_stream<R>(
    mut stream: rig::agent::StreamingResult<R>,
    deadline: Duration,
    on_tool_event: Option<&dyn ToolEventObserver>,
) -> Result<String, ReviewAgentError>
where
    R: Clone + Unpin,
{
    let mut tool_name_by_id: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut final_output = String::new();

    loop {
        let next = timeout(deadline, stream.next()).await;
        match next {
            Err(_) => return Err(ReviewAgentError::Timeout),
            Ok(None) => break,
            Ok(Some(item)) => match item {
                Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => match content {
                    StreamedAssistantContent::ToolCall {
                        tool_call,
                        internal_call_id,
                    } => {
                        tool_name_by_id
                            .insert(internal_call_id.clone(), tool_call.function.name.clone());
                        if let Some(observer) = on_tool_event {
                            observer.on_tool_call(
                                &tool_call.function.name,
                                &tool_call.function.arguments,
                            );
                        }
                    }
                    StreamedAssistantContent::Text(_) => {}
                    _ => {}
                },
                Ok(MultiTurnStreamItem::StreamUserItem(user_content)) => {
                    if let Some(observer) = on_tool_event {
                        if let Some((name, result_text)) =
                            extract_tool_result(&user_content, &tool_name_by_id)
                        {
                            observer.on_tool_result(&name, &result_text);
                        }
                    }
                }
                Ok(MultiTurnStreamItem::FinalResponse(final_response)) => {
                    final_output = final_response.response().to_owned();
                    break;
                }
                Ok(_) => {}
                Err(error) => {
                    return Err(ReviewAgentError::Provider(error.to_string()));
                }
            },
        }
    }

    Ok(final_output)
}

/// Extracts the tool name and a text summary from a streamed user-content
/// tool result, falling back to the provider's tool result id when the
/// internal call id has no recorded name.
fn extract_tool_result(
    user_content: &rig::streaming::StreamedUserContent,
    tool_name_by_id: &std::collections::HashMap<String, String>,
) -> Option<(String, String)> {
    // `StreamedUserContent` currently has a single `ToolResult` variant;
    // match explicitly so new variants added upstream surface as a
    // compile error rather than silently being dropped.
    let rig::streaming::StreamedUserContent::ToolResult {
        tool_result,
        internal_call_id,
    } = user_content;
    let name = tool_name_by_id
        .get(internal_call_id)
        .cloned()
        .unwrap_or_else(|| tool_result.id.clone());
    let result_text = tool_result
        .content
        .iter()
        .filter_map(|content| match content {
            rig::completion::message::ToolResultContent::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some((name, result_text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sample_comment() -> serde_json::Value {
        serde_json::json!({
            "file": "src/main.rs",
            "line_start": 10,
            "line_end": 12,
            "severity": "High",
            "category": "injection",
            "cwe_id": "CWE-78",
            "cwe_name": "OS Command Injection",
            "title": "Unsanitized command",
            "description": "User input reaches a shell command.",
            "rationale": "Direct shell execution is dangerous as it allows command injection if input is not perfectly sanitized. Using specialized APIs is safer.",
            "evidence": "Command::new(\"sh\").arg(user_input)",
            "suggestion": "Avoid shell execution.",
            "verification_plan": "Run the program with a payload like '; touch pwned' and verify the file is not created."
        })
    }

    fn sample_review_comment() -> ReviewComment {
        serde_json::from_value(sample_comment()).unwrap()
    }

    fn tiered_comment(severity: Severity, category: &str, tier: SignalTier) -> ReviewComment {
        ReviewComment {
            severity,
            category: category.to_string(),
            signal_tier: tier,
            ..sample_review_comment()
        }
    }

    #[test]
    fn parses_diff_by_file_and_skips_binary_marker() {
        let diff = r#"diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1 +1 @@
-old
+new
diff --git a/icon.png b/icon.png
Binary files a/icon.png and b/icon.png differ
"#;

        let files = parse_diff_by_file(diff);

        assert_eq!(files.len(), 2);
        assert_eq!(files[0].file, "src/a.rs");
        assert!(!files[0].is_binary);
        assert_eq!(files[1].file, "icon.png");
        assert!(files[1].is_binary);
    }

    #[test]
    fn chunks_files_by_count_and_line_limit() {
        let files = (0..7)
            .map(|index| FileDiff {
                file: format!("file-{}.rs", index),
                diff: "line\n".repeat(10),
                line_count: 10,
                is_binary: false,
            })
            .collect();

        let chunks = chunk_files(files, 5, 50);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 5);
        assert_eq!(chunks[1].len(), 2);
    }

    #[test]
    fn scan_cap_limits_batches_after_line_chunking() {
        let files = (0..25)
            .map(|index| ScanFile {
                path: format!("large-{}.rs", index),
                line_count: 600,
            })
            .collect();
        let chunks = chunk_scan_files(files, 5, 500);

        let (capped, was_capped) = cap_scan_batches(chunks, MAX_SCAN_INVOCATIONS);

        assert!(was_capped);
        assert_eq!(capped.len(), MAX_SCAN_INVOCATIONS);
        assert_eq!(capped.iter().map(Vec::len).sum::<usize>(), 20);
    }

    #[test]
    fn parses_fenced_agent_json_envelope() {
        let raw = format!(
            "Here is the result:\n```json\n{{\"summary\":\"one\",\"comments\":[{}],\"warnings\":[\"partial\"]}}\n```",
            sample_comment()
        );

        let parsed = parse_agent_review_output(&raw, "Detector");

        assert_eq!(parsed.comments.len(), 1);
        assert_eq!(parsed.comments[0].severity, Severity::High);
        assert_eq!(parsed.summary.as_deref(), Some("one"));
        assert_eq!(parsed.warnings, vec!["partial"]);
    }

    #[test]
    fn valid_empty_agent_envelope_is_parseable() {
        let parsed = parse_agent_review_output(
            r#"{"summary":"Rejected all candidates","comments":[],"warnings":[]}"#,
            "Validator",
        );

        assert!(parsed.comments.is_empty());
        assert_eq!(parsed.summary.as_deref(), Some("Rejected all candidates"));
        assert!(!is_unparseable_agent_output(&parsed));
    }

    #[test]
    fn empty_object_is_not_parseable_agent_output() {
        let parsed = parse_agent_review_output("{}", "Validator");

        assert!(parsed.comments.is_empty());
        assert!(is_unparseable_agent_output(&parsed));
    }

    #[test]
    fn review_store_loader_warns_and_uses_empty_store_when_file_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let gospel_dir = dir.path().join(".gospel");
        fs::create_dir_all(&gospel_dir).unwrap();
        fs::write(gospel_dir.join("rejected_findings.json"), "{not json").unwrap();
        let mut warnings = Vec::new();

        let store = load_anti_pattern_store_for_review(dir.path(), &mut warnings);

        assert!(store.rejected_hashes.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Ignored rejected findings store"));
        assert!(warnings[0].contains("Failed to parse rejected findings store"));
    }

    #[test]
    fn validator_parse_result_is_filtered_against_rejected_findings() {
        let comment = sample_review_comment();
        let mut store = anti_pattern::AntiPatternStore::default();
        store.add_rejection(
            ReviewFocus::Security,
            &comment.file,
            comment.line_start,
            comment.line_end,
            &comment.title,
        );
        let parsed = AgentParseResult {
            comments: vec![comment],
            summary: Some("Found 1 validated security finding.".to_string()),
            warnings: Vec::new(),
        };

        let (comments, validated, summary, warnings) =
            review_validator_parse_result(parsed, &[], &store);

        assert!(comments.is_empty());
        assert!(validated);
        assert!(summary.is_none());
        assert!(warnings.is_empty());
    }

    #[test]
    fn parsed_comments_are_stamped_before_rejection_filtering() {
        let raw = sample_comment().to_string();
        let mut parsed = parse_agent_review_output(&raw, "Detector");
        let comment = parsed.comments[0].clone();
        let mut store = anti_pattern::AntiPatternStore::default();
        store.add_rejection(
            ReviewFocus::Security,
            &comment.file,
            comment.line_start,
            comment.line_end,
            &comment.title,
        );

        stamp_parsed_comments_focus(&mut parsed, ReviewFocus::BugHunt);
        let comments = filter_rejected_comments(parsed.comments, &store);

        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].focus, ReviewFocus::BugHunt);
    }

    #[test]
    fn parsed_comments_default_focus_is_stamped_before_rejection_filtering() {
        let raw = format!(
            "{{\"summary\":\"one\",\"comments\":[{}],\"warnings\":[]}}",
            sample_comment()
        );
        let mut store = anti_pattern::AntiPatternStore::default();
        store.add_rejection(
            ReviewFocus::Security,
            "src/main.rs",
            10,
            12,
            "Unsanitized command",
        );

        let parsed = parse_agent_review_output(&raw, "Detector");
        assert_eq!(parsed.comments.len(), 1);
        assert_eq!(parsed.comments[0].focus, ReviewFocus::Security);
        let comments = filter_rejected_comments(parsed.comments, &store);

        assert!(
            comments.is_empty(),
            "Security-stamped comment should be filtered by Security rejection"
        );
    }

    #[test]
    fn parses_single_review_comment_object() {
        let parsed = parse_agent_review_output(&sample_comment().to_string(), "Detector");

        assert_eq!(parsed.comments.len(), 1);
        assert_eq!(parsed.comments[0].file, "src/main.rs");
        assert_eq!(parsed.comments[0].severity, Severity::High);
        assert_eq!(parsed.comments[0].focus, ReviewFocus::Security);
        assert_eq!(parsed.comments[0].focus_subcategory, None);
    }

    #[test]
    fn noise_gate_preserves_results_at_exact_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let comments = vec![
            tiered_comment(Severity::Medium, "business logic", SignalTier::Tier1),
            tiered_comment(Severity::Medium, "business logic", SignalTier::Tier2),
            tiered_comment(Severity::Medium, "business logic", SignalTier::Tier2),
            tiered_comment(Severity::Low, "style", SignalTier::Noise),
            tiered_comment(Severity::Low, "style", SignalTier::Noise),
        ];
        let result = review_result(
            comments,
            "Found 5 validated security findings.".to_string(),
            true,
            Vec::new(),
            3,
            ReviewFocus::Security,
            ReviewMode::Local,
        );

        let finalized = finalize_review_result(
            "test-run-id",
            &NoopReviewProgressEmitter,
            "openai",
            "model",
            dir.path(),
            result,
        )
        .unwrap();

        assert_eq!(finalized.comments.len(), 5);
        assert_eq!(finalized.suppressed_count, 0);
        assert_eq!(finalized.snr_percent, 60.0);
        assert!(finalized.user_visible);
    }

    #[test]
    fn noise_gate_handles_empty_comment_batches() {
        let dir = tempfile::tempdir().unwrap();
        let result = review_result(
            Vec::new(),
            "No security findings detected.".to_string(),
            true,
            Vec::new(),
            0,
            ReviewFocus::Security,
            ReviewMode::FullScan,
        );

        let finalized = finalize_review_result(
            "test-run-id",
            &NoopReviewProgressEmitter,
            "openai",
            "model",
            dir.path(),
            result,
        )
        .unwrap();

        assert!(finalized.comments.is_empty());
        assert_eq!(finalized.suppressed_count, 0);
        assert_eq!(finalized.snr_percent, 100.0);
        assert!(finalized.user_visible);
    }

    #[test]
    fn noise_gate_suppresses_all_noise_batches() {
        let dir = tempfile::tempdir().unwrap();
        let result = review_result(
            vec![
                tiered_comment(Severity::Low, "style", SignalTier::Tier2),
                tiered_comment(Severity::Info, "formatting", SignalTier::Unclassified),
            ],
            "Found 2 validated security findings.".to_string(),
            true,
            Vec::new(),
            2,
            ReviewFocus::Security,
            ReviewMode::Local,
        );

        let finalized = finalize_review_result(
            "test-run-id",
            &NoopReviewProgressEmitter,
            "openai",
            "model",
            dir.path(),
            result,
        )
        .unwrap();

        assert!(finalized.comments.is_empty());
        assert_eq!(finalized.suppressed_count, 2);
        assert_eq!(finalized.snr_percent, 0.0);
        assert!(!finalized.user_visible);
        assert!(finalized.summary.contains("Showing 0 actionable findings"));
    }

    #[test]
    fn filters_generated_and_binary_scan_paths() {
        assert!(should_skip_scan_file("target/debug/app"));
        assert!(should_skip_scan_file("src/assets/icon.png"));
        assert!(should_skip_scan_file("package-lock.json"));
        assert!(!should_skip_scan_file("src/lib.rs"));
        assert!(!should_skip_scan_file("docs/agents/domain.md"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_command_output_drains_large_stdout_while_waiting() {
        let dir = tempfile::tempdir().unwrap();

        let result = timeout(
            Duration::from_secs(2),
            run_command_output(dir.path(), "sh", &["-c", "yes x | head -c 131072"]),
        )
        .await;

        let output = result
            .expect("command should not deadlock on a full stdout pipe")
            .expect("command should run successfully");

        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 131072);
        assert!(output.stderr.is_empty());
    }

    fn detector_failure(index: usize, kind: DetectorFailureKind, detail: &str) -> DetectorFailure {
        DetectorFailure {
            index,
            kind,
            detail: detail.to_string(),
        }
    }

    #[test]
    fn detector_failure_warning_line_includes_kind_index_and_detail() {
        let failure = detector_failure(2, DetectorFailureKind::Timeout, "agent timed out");
        assert_eq!(
            failure.warning_line("diff chunk"),
            "Detector agent timeout on diff chunk 2: agent timed out"
        );

        let failure = detector_failure(3, DetectorFailureKind::Provider, "rate limited");
        assert_eq!(
            failure.warning_line("scan batch"),
            "Detector agent provider_error on scan batch 3: rate limited"
        );
    }

    #[test]
    fn detector_failure_from_error_classifies_timeout_and_provider() {
        let timeout = DetectorFailure::from_error(1, &ReviewAgentError::Timeout);
        assert_eq!(timeout.kind, DetectorFailureKind::Timeout);
        assert_eq!(timeout.index, 1);

        let provider =
            DetectorFailure::from_error(2, &ReviewAgentError::Provider("boom".to_string()));
        assert_eq!(provider.kind, DetectorFailureKind::Provider);
        assert_eq!(provider.detail, "boom");
    }

    #[test]
    fn detector_failure_from_error_sanitizes_provider_detail() {
        // Multi-line provider payloads should collapse to the first line so
        // headers/stack traces never reach the user or disk.
        let raw = "rate limited\nx-request-id: secret-token-abc";
        let provider = DetectorFailure::from_error(3, &ReviewAgentError::Provider(raw.to_string()));
        assert_eq!(provider.kind, DetectorFailureKind::Provider);
        assert_eq!(provider.detail, "rate limited");
    }

    #[test]
    fn sanitize_failure_detail_truncates_long_messages() {
        let long = "x".repeat(600);
        let sanitized = sanitize_failure_detail(&long);
        // Truncates to the 500-char cap plus a trailing ellipsis character.
        assert_eq!(sanitized.chars().count(), 501);
        assert!(sanitized.ends_with('…'));
    }

    #[test]
    fn sanitize_failure_detail_keeps_short_messages_intact() {
        assert_eq!(sanitize_failure_detail("rate limited"), "rate limited");
        assert_eq!(sanitize_failure_detail("  trimmed  "), "trimmed");
    }

    #[test]
    fn progress_failure_detail_preserves_detector_failure_summary() {
        let failures = vec![
            detector_failure(1, DetectorFailureKind::Provider, "bad model"),
            detector_failure(2, DetectorFailureKind::Provider, "tool request rejected"),
        ];
        let message = all_detector_failures_error(
            "chatgpt",
            "gpt-5.5",
            &ReviewMode::PullRequest { pr_number: 34 },
            "diff chunk",
            ReviewFocus::Security,
            &failures,
        );

        let progress_detail = progress_failure_detail(&message);

        assert!(progress_detail.contains("All 2 detector invocations failed"));
        assert!(progress_detail.contains("Failures: 0 timeout(s), 2 provider error(s)."));
        assert!(progress_detail.contains("Per-chunk reasons:"));
        assert!(progress_detail.contains("diff chunk 1: [provider_error] bad model"));
        assert!(progress_detail.contains("diff chunk 2: [provider_error] tool request rejected"));
    }

    #[test]
    fn progress_failure_detail_sanitizes_unknown_multiline_errors() {
        let progress_detail =
            progress_failure_detail("provider failed\nx-request-id: should-not-be-shown");

        assert_eq!(progress_detail, "provider failed");
    }

    #[test]
    fn all_detector_failures_error_lists_per_chunk_reasons_and_counts() {
        let failures = vec![
            detector_failure(1, DetectorFailureKind::Timeout, "agent timed out"),
            detector_failure(2, DetectorFailureKind::Provider, "rate limited"),
            detector_failure(3, DetectorFailureKind::Provider, "bad model"),
        ];
        let message = all_detector_failures_error(
            "chatgpt",
            "gpt-5.5",
            &ReviewMode::PullRequest { pr_number: 42 },
            "diff chunk",
            ReviewFocus::Security,
            &failures,
        );

        assert!(message.contains("All 3 detector invocations failed"));
        assert!(message.contains("pr Security review"));
        assert!(message.contains("chatgpt/gpt-5.5"));
        assert!(message.contains("Failures: 1 timeout(s), 2 provider error(s)."));
        assert!(message.contains("diff chunk 1: [timeout] agent timed out"));
        assert!(message.contains("diff chunk 2: [provider_error] rate limited"));
        assert!(message.contains("diff chunk 3: [provider_error] bad model"));
    }

    #[test]
    fn all_detector_failures_error_handles_all_timeouts() {
        let failures = vec![
            detector_failure(1, DetectorFailureKind::Timeout, "agent timed out"),
            detector_failure(2, DetectorFailureKind::Timeout, "agent timed out"),
        ];
        let message = all_detector_failures_error(
            "openai",
            "gpt-4o",
            &ReviewMode::Local,
            "diff chunk",
            ReviewFocus::Performance,
            &failures,
        );

        assert!(message.contains("Failures: 2 timeout(s), 0 provider error(s)."));
        assert!(message.contains("try a smaller PR"));
    }

    #[test]
    fn ensure_provider_session_passes_for_non_oauth_provider() {
        assert!(ensure_provider_session("openai").is_ok());
    }

    #[test]
    fn ensure_provider_session_rejects_unauthenticated_oauth_provider() {
        // Simulate an unauthenticated provider explicitly so the test does not
        // depend on real keychain state (which could skip assertions on a
        // machine that happens to be signed in).
        fn always_unauthenticated(_provider: &str) -> bool {
            false
        }

        let error = ensure_provider_session_with("chatgpt", always_unauthenticated).unwrap_err();
        assert!(error.contains("chatgpt"));
        assert!(error.contains("not authenticated"));
        assert!(error.contains("Sign in"));

        let error =
            ensure_provider_session_with("github_copilot", always_unauthenticated).unwrap_err();
        assert!(error.contains("github_copilot"));
        assert!(error.contains("not authenticated"));
    }

    #[test]
    fn ensure_provider_session_accepts_authenticated_oauth_provider() {
        fn always_authenticated(_provider: &str) -> bool {
            true
        }

        assert!(ensure_provider_session_with("chatgpt", always_authenticated).is_ok());
        assert!(ensure_provider_session_with("github_copilot", always_authenticated).is_ok());
    }

    #[test]
    fn persist_review_failure_writes_jsonl_record() {
        let dir = tempfile::tempdir().unwrap();
        let failures = vec![
            detector_failure(1, DetectorFailureKind::Timeout, "agent timed out"),
            detector_failure(2, DetectorFailureKind::Provider, "rate limited"),
        ];

        persist_review_failure(
            dir.path(),
            "chatgpt",
            "gpt-5.5",
            &ReviewMode::PullRequest { pr_number: 7 },
            ReviewFocus::Security,
            &failures,
            12,
        );

        let content = fs::read_to_string(dir.path().join(".gospel/review_failures.jsonl")).unwrap();
        assert_eq!(content.lines().count(), 1);
        let record: analytics::ReviewFailureRecord = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(record.provider, "chatgpt");
        assert_eq!(record.model, "gpt-5.5");
        assert_eq!(record.files_scanned, 12);
        assert_eq!(record.failed_chunks, 2);
        assert_eq!(record.chunks.len(), 2);
        assert_eq!(record.chunks[0].kind, "timeout");
        assert_eq!(record.chunks[1].kind, "provider_error");
        assert_eq!(record.chunks[1].detail, "rate limited");
        assert_eq!(record.focus, ReviewFocus::Security);
        match record.mode {
            ReviewMode::PullRequest { pr_number } => assert_eq!(pr_number, 7),
            _ => panic!("expected PullRequest mode"),
        }
    }

    /// Capturing emitter that records every event in order so tests can assert
    /// the real emission sequence without touching Tauri.
    #[derive(Default)]
    struct CapturingEmitter {
        events: std::sync::Mutex<Vec<ReviewProgressEvent>>,
    }

    impl ReviewProgressEmitter for CapturingEmitter {
        fn emit_progress(&self, event: ReviewProgressEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    impl CapturingEmitter {
        fn phases(&self) -> Vec<ReviewPhase> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .map(|event| event.phase.clone())
                .collect()
        }
    }

    #[test]
    fn finalize_emits_running_then_done_with_run_id() {
        let dir = tempfile::tempdir().unwrap();
        let result = review_result(
            vec![tiered_comment(
                Severity::High,
                "injection",
                SignalTier::Tier1,
            )],
            "Found 1 validated security finding.".to_string(),
            true,
            Vec::new(),
            1,
            ReviewFocus::Security,
            ReviewMode::Local,
        );
        let emitter = CapturingEmitter::default();

        let finalized = finalize_review_result(
            "run-id-abc",
            &emitter,
            "openai",
            "model",
            dir.path(),
            result,
        )
        .unwrap();

        assert_eq!(finalized.run_id, "run-id-abc");

        let phases = emitter.phases();
        assert_eq!(phases.len(), 2, "finalize should emit exactly two events");
        assert!(matches!(
            phases[0],
            ReviewPhase::Finalize {
                status: PhaseStatus::Running
            }
        ));
        assert!(matches!(
            phases[1],
            ReviewPhase::Finalize {
                status: PhaseStatus::Done
            }
        ));
        // Every event carries the same run_id.
        for event in emitter.events.lock().unwrap().iter() {
            assert_eq!(event.run_id, "run-id-abc");
        }
    }

    #[test]
    fn noop_emitter_does_not_panic_on_every_phase_variant() {
        let noop = NoopReviewProgressEmitter;
        let run_id = "noop-run";
        for phase in [
            ReviewPhase::Detector {
                chunk: 0,
                total_chunks: 0,
                files: Vec::new(),
                candidate_count: 0,
                status: ChunkStatus::Starting,
            },
            ReviewPhase::Detector {
                chunk: 1,
                total_chunks: 3,
                files: vec!["src/a.rs".to_string()],
                candidate_count: 2,
                status: ChunkStatus::Failed {
                    kind: "timeout".to_string(),
                    detail: "agent timed out".to_string(),
                },
            },
            ReviewPhase::Validator {
                candidate_count: 2,
                status: PhaseStatus::Running,
            },
            ReviewPhase::Finalize {
                status: PhaseStatus::Done,
            },
            ReviewPhase::Done {
                findings: 1,
                suppressed: 1,
            },
            ReviewPhase::Failed {
                detail: "boom".to_string(),
            },
        ] {
            noop.emit_progress(ReviewProgressEvent::new(run_id, phase));
        }
    }

    #[test]
    fn review_progress_event_serializes_phase_type_for_frontend() {
        // The frontend switches on `event.payload.type`; assert the serde tag
        // convention matches the SessionTurnEvent pattern.
        let event = ReviewProgressEvent::new(
            "run-1",
            ReviewPhase::Detector {
                chunk: 2,
                total_chunks: 5,
                files: vec!["src/lib.rs".to_string()],
                candidate_count: 3,
                status: ChunkStatus::Running,
            },
        );
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["run_id"], "run-1");
        assert_eq!(json["phase"]["type"], "detector");
        assert_eq!(json["phase"]["chunk"], 2);
        assert_eq!(json["phase"]["totalChunks"], 5);
        assert_eq!(json["phase"]["candidateCount"], 3);
        assert_eq!(json["phase"]["status"], "running");
    }

    #[test]
    fn review_progress_failed_chunk_serializes_nested_failure_for_frontend() {
        let event = ReviewProgressEvent::new(
            "run-1",
            ReviewPhase::Detector {
                chunk: 1,
                total_chunks: 1,
                files: vec!["src/lib.rs".to_string()],
                candidate_count: 0,
                status: ChunkStatus::Failed {
                    kind: "provider_error".to_string(),
                    detail: "model rejected tool-capable request".to_string(),
                },
            },
        );

        let json = serde_json::to_value(&event).unwrap();

        assert_eq!(json["phase"]["status"]["failed"]["kind"], "provider_error");
        assert_eq!(
            json["phase"]["status"]["failed"]["detail"],
            "model rejected tool-capable request"
        );
        assert!(json["phase"]["status"]["detail"].is_null());
    }

    #[test]
    fn shared_agent_max_turns_is_50() {
        assert_eq!(
            crate::llm::AGENT_MAX_TURNS,
            50,
            "Shared interactive-agent turn budget should be 50"
        );
    }
}
