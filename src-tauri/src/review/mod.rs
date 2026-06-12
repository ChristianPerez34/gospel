pub mod anti_pattern;
pub mod detector;
pub mod knowledge;
pub mod validator;

use crate::llm::WorkspaceToolContext;
use crate::workspace_tools::{
    create_find_files_tool, create_list_directory_tool, create_read_file_tool,
    create_search_code_tool,
};
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::providers::{anthropic, chatgpt, gemini, groq, mistral, openai};
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tokio::process::Command;
use tokio::time::timeout;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    pub file: String,
    pub line_start: usize,
    pub line_end: usize,
    pub severity: Severity,
    pub category: String,
    pub cwe_id: Option<String>,
    pub cwe_name: Option<String>,
    pub title: String,
    pub description: String,
    pub rationale: Option<String>,
    pub evidence: String,
    pub suggestion: Option<String>,
    pub verification_plan: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewResult {
    pub comments: Vec<ReviewComment>,
    pub summary: String,
    pub validated: bool,
    pub warnings: Vec<String>,
    pub files_scanned: usize,
    pub mode: ReviewMode,
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
) -> Result<ReviewResult, String> {
    let mode = config.review_mode()?;
    let workspace = WorkspaceToolContext {
        workspace_path,
        corpus_available: false,
    };

    match mode {
        ReviewMode::Local => {
            let diff = get_local_diff(&workspace.workspace_path).await?;
            run_diff_review(
                &config.provider,
                &config.model,
                &api_key,
                &workspace,
                ReviewMode::Local,
                "You are reviewing local staged and unstaged changes for security vulnerabilities."
                    .to_string(),
                diff,
            )
            .await
        }
        ReviewMode::PullRequest { pr_number } => {
            let pr = get_pr_diff(&workspace.workspace_path, pr_number).await?;
            let context = format!(
                "You are reviewing Pull Request #{} for security vulnerabilities.\n\nPR Title: {}\nPR Description: {}\n\nAnalyze the changes for security issues. Use read_file to examine surrounding context in the repository.",
                pr_number, pr.title, pr.body
            );
            run_diff_review(
                &config.provider,
                &config.model,
                &api_key,
                &workspace,
                ReviewMode::PullRequest { pr_number },
                context,
                pr.diff,
            )
            .await
        }
        ReviewMode::FullScan => {
            run_full_scan_review(&config.provider, &config.model, &api_key, &workspace).await
        }
    }
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
            if store.is_rejected(&comment.file, comment.line_start, comment.line_end, &comment.title)
            {
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

async fn run_full_scan_review(
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: &WorkspaceToolContext,
) -> Result<ReviewResult, String> {
    let mut warnings = Vec::new();
    let files = get_full_scan_files(&workspace.workspace_path).await?;
    if files.is_empty() {
        return Ok(ReviewResult {
            comments: Vec::new(),
            summary: "No tracked files found".to_string(),
            validated: true,
            warnings,
            files_scanned: 0,
            mode: ReviewMode::FullScan,
        });
    }

    let total_files = files.len();

    let batches = chunk_scan_files(files, MAX_FILES_PER_INVOCATION, MAX_LINES_PER_INVOCATION);
    let (batches, scan_was_capped) = cap_scan_batches(batches, MAX_SCAN_INVOCATIONS);
    let files_scanned = batches.iter().map(Vec::len).sum();
    if scan_was_capped {
        warnings.push(partial_review_warning(files_scanned, total_files));
    }

    let mut candidates = Vec::new();
    let mut failed_batches = 0usize;

    let anti_pattern_store = anti_pattern::AntiPatternStore::load(&workspace.workspace_path)?;

    for (index, batch) in batches.iter().enumerate() {
        let prompt = detector::build_scan_prompt(batch.iter().map(|file| file.path.as_str()));
        match detector::run_detector(provider, model, api_key, workspace, &prompt).await {
            Ok(output) => {
                let parsed = parse_agent_review_output(&output, "Detector");
                warnings.extend(parsed.warnings);
                candidates.extend(filter_rejected_comments(parsed.comments, &anti_pattern_store));
            }
            Err(ReviewAgentError::Timeout) => {
                failed_batches += 1;
                warnings.push(format!(
                    "Detector agent timed out on scan batch {}",
                    index + 1
                ));
            }
            Err(ReviewAgentError::Provider(error)) => {
                failed_batches += 1;
                warnings.push(format!(
                    "Detector agent failed on scan batch {}: {}",
                    index + 1,
                    error
                ));
            }
        }
    }

    if failed_batches == batches.len() {
        return Err("All detector invocations failed".to_string());
    }

    let (comments, validated, validator_summary, validator_warnings) =
        validate_candidates(provider, model, api_key, workspace, &candidates).await?;
    warnings.extend(validator_warnings);

    Ok(ReviewResult {
        summary: validator_summary.unwrap_or_else(|| summarize_comments(&comments, validated)),
        comments,
        validated,
        warnings,
        files_scanned,
        mode: ReviewMode::FullScan,
    })
}

async fn run_diff_review(
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: &WorkspaceToolContext,
    mode: ReviewMode,
    review_context: String,
    diff: String,
) -> Result<ReviewResult, String> {
    if diff.trim().is_empty() {
        return Ok(ReviewResult {
            comments: Vec::new(),
            summary: "No changes found to review".to_string(),
            validated: true,
            warnings: Vec::new(),
            files_scanned: 0,
            mode,
        });
    }

    let mut warnings = Vec::new();
    let file_diffs: Vec<FileDiff> = parse_diff_by_file(&diff)
        .into_iter()
        .filter(|file| !file.is_binary)
        .collect();

    if file_diffs.is_empty() {
        warnings
            .push("Only binary files were present in the diff; nothing was reviewed".to_string());
        return Ok(ReviewResult {
            comments: Vec::new(),
            summary: "No text changes found to review".to_string(),
            validated: true,
            warnings,
            files_scanned: 0,
            mode,
        });
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

    let mut candidates = Vec::new();
    let mut failed_chunks = 0usize;

    let anti_pattern_store = anti_pattern::AntiPatternStore::load(&workspace.workspace_path)?;

    for (index, chunk) in chunks.iter().enumerate() {
        let prompt = detector::build_diff_prompt(&review_context, chunk);
        match detector::run_detector(provider, model, api_key, workspace, &prompt).await {
            Ok(output) => {
                let parsed = parse_agent_review_output(&output, "Detector");
                warnings.extend(parsed.warnings);
                candidates.extend(filter_rejected_comments(parsed.comments, &anti_pattern_store));
            }
            Err(ReviewAgentError::Timeout) => {
                failed_chunks += 1;
                warnings.push(format!(
                    "Detector agent timed out on diff chunk {}",
                    index + 1
                ));
            }
            Err(ReviewAgentError::Provider(error)) => {
                failed_chunks += 1;
                warnings.push(format!(
                    "Detector agent failed on diff chunk {}: {}",
                    index + 1,
                    error
                ));
            }
        }
    }

    if failed_chunks == chunks.len() {
        return Err("All detector invocations failed".to_string());
    }

    let (comments, validated, validator_summary, validator_warnings) =
        validate_candidates(provider, model, api_key, workspace, &candidates).await?;
    warnings.extend(validator_warnings);

    Ok(ReviewResult {
        summary: validator_summary.unwrap_or_else(|| summarize_comments(&comments, validated)),
        comments,
        validated,
        warnings,
        files_scanned,
        mode,
    })
}

async fn validate_candidates(
    provider: &str,
    model: &str,
    api_key: &str,
    workspace: &WorkspaceToolContext,
    candidates: &[ReviewComment],
) -> Result<(Vec<ReviewComment>, bool, Option<String>, Vec<String>), String> {
    if candidates.is_empty() {
        return Ok((
            Vec::new(),
            true,
            Some("No security findings detected.".to_string()),
            Vec::new(),
        ));
    }

    let prompt = validator::build_validator_prompt(candidates)
        .map_err(|e| format!("Failed to build validator prompt: {}", e))?;
    match validator::run_validator(provider, model, api_key, workspace, &prompt).await {
        Ok(output) => {
            let parsed = parse_agent_review_output(&output, "Validator");
            if is_unparseable_agent_output(&parsed) {
                let mut warnings = parsed.warnings;
                warnings.push(
                    "Validator output was unparseable; returning detector candidates".to_string(),
                );
                Ok((candidates.to_vec(), false, None, warnings))
            } else {
                Ok((parsed.comments, true, parsed.summary, parsed.warnings))
            }
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
            vec![format!("Validator agent failed: {}", error)],
        )),
    }
}

fn summarize_comments(comments: &[ReviewComment], validated: bool) -> String {
    if comments.is_empty() {
        return "No security findings detected.".to_string();
    }

    let validation = if validated { "validated" } else { "candidate" };
    format!(
        "Found {} {} security finding{}.",
        comments.len(),
        validation,
        if comments.len() == 1 { "" } else { "s" }
    )
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
}

pub(crate) async fn run_workspace_agent(
    config: AgentConfig<'_>,
) -> Result<String, ReviewAgentError> {
    if config.provider != "chatgpt" && config.api_key.trim().is_empty() {
        return Err(ReviewAgentError::Provider(format!(
            "API key not configured for {}",
            config.provider
        )));
    }

    macro_rules! run_from_client {
        ($client:expr, $model:expr) => {{
            let agent = $client
                .agent($model)
                .preamble(config.preamble)
                .default_max_turns(config.max_turns)
                .tool(create_read_file_tool(config.workspace.workspace_path.clone()))
                .tool(create_search_code_tool(config.workspace.workspace_path.clone()))
                .tool(create_find_files_tool(config.workspace.workspace_path.clone()))
                .tool(create_list_directory_tool(config.workspace.workspace_path.clone()))
                .build();
            let future = agent.prompt(config.prompt).max_turns(config.max_turns);
            timeout(config.timeout, future)
                .await
                .map_err(|_| ReviewAgentError::Timeout)?
                .map_err(|e| ReviewAgentError::Provider(e.to_string()))
        }};
    }

    match config.provider {
        "openai" => {
            let client = openai::Client::new(config.api_key)
                .map_err(|e| ReviewAgentError::Provider(e.to_string()))?;
            run_from_client!(client, config.model)
        }
        "chatgpt" => {
            let client = chatgpt::Client::builder()
                .oauth()
                .build()
                .map_err(|e| ReviewAgentError::Provider(e.to_string()))?;
            run_from_client!(client, config.model)
        }
        "anthropic" => {
            let client = anthropic::Client::new(config.api_key)
                .map_err(|e| ReviewAgentError::Provider(e.to_string()))?;
            run_from_client!(client, config.model)
        }
        "gemini" => {
            let client = gemini::Client::new(config.api_key)
                .map_err(|e| ReviewAgentError::Provider(e.to_string()))?;
            run_from_client!(client, config.model)
        }
        "groq" => {
            let client = groq::Client::new(config.api_key)
                .map_err(|e| ReviewAgentError::Provider(e.to_string()))?;
            run_from_client!(client, config.model)
        }
        "mistral" => {
            let client = mistral::Client::new(config.api_key)
                .map_err(|e| ReviewAgentError::Provider(e.to_string()))?;
            run_from_client!(client, config.model)
        }
        _ => Err(ReviewAgentError::Provider(format!(
            "unsupported provider: {}",
            config.provider
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parses_single_review_comment_object() {
        let parsed = parse_agent_review_output(&sample_comment().to_string(), "Detector");

        assert_eq!(parsed.comments.len(), 1);
        assert_eq!(parsed.comments[0].file, "src/main.rs");
        assert_eq!(parsed.comments[0].severity, Severity::High);
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
}
