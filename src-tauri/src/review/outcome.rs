use super::anti_pattern::AntiPatternStore;
use super::{ReviewComment, ReviewFocus, ReviewMode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewRunRecord {
    pub run_id: String,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub focus: ReviewFocus,
    pub mode: ReviewMode,
    pub comments: Vec<ReviewComment>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewOutcome {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewOutcomeRecord {
    pub run_id: String,
    pub comment_id: String,
    pub outcome: ReviewOutcome,
    pub recorded_at: DateTime<Utc>,
    #[serde(default)]
    pub focus: ReviewFocus,
    pub file: String,
    pub line_start: usize,
    pub line_end: usize,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewOutcomeOutput {
    pub success: bool,
    pub message: String,
    pub run_id: String,
    pub comment_id: String,
    pub outcome: ReviewOutcome,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AcceptedFindingsStore {
    #[serde(default)]
    accepted_findings: Vec<ReviewOutcomeRecord>,
}

pub fn save_review_run(workspace_path: &Path, record: &ReviewRunRecord) -> Result<(), String> {
    validate_id_segment(&record.run_id)?;
    let review_runs_dir = workspace_path.join(".gospel").join("review_runs");
    fs::create_dir_all(&review_runs_dir)
        .map_err(|error| format!("Failed to create review runs directory: {}", error))?;
    let path = review_runs_dir.join(format!("{}.json", record.run_id));
    let content = serde_json::to_string_pretty(record)
        .map_err(|error| format!("Failed to serialize review run: {}", error))?;
    write_file_atomically(&path, &content)
}

pub fn load_review_run(workspace_path: &Path, run_id: &str) -> Result<ReviewRunRecord, String> {
    validate_id_segment(run_id)?;
    let path = workspace_path
        .join(".gospel")
        .join("review_runs")
        .join(format!("{}.json", run_id));
    let content = fs::read_to_string(&path).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            format!("Review run {} does not exist", run_id)
        } else {
            format!("Failed to read review run {}: {}", path.display(), error)
        }
    })?;
    serde_json::from_str(&content)
        .map_err(|error| format!("Failed to parse review run {}: {}", path.display(), error))
}

pub fn record_review_outcome(
    workspace_path: &Path,
    run_id: &str,
    comment_id: &str,
    outcome: ReviewOutcome,
) -> Result<ReviewOutcomeOutput, String> {
    let run = load_review_run(workspace_path, run_id)?;
    let comment = run
        .comments
        .iter()
        .find(|comment| comment.comment_id == comment_id)
        .ok_or_else(|| {
            format!(
                "Comment {} does not exist in review run {}",
                comment_id, run_id
            )
        })?;
    let recorded_at = Utc::now();

    match outcome {
        ReviewOutcome::Accepted => {
            record_acceptance(workspace_path, &run.run_id, comment, recorded_at)?
        }
        ReviewOutcome::Rejected => record_rejection(workspace_path, comment, recorded_at)?,
    }

    Ok(ReviewOutcomeOutput {
        success: true,
        message: format!(
            "Recorded {:?} outcome for review comment {}.",
            outcome, comment_id
        ),
        run_id: run.run_id,
        comment_id: comment_id.to_string(),
        outcome,
        recorded_at,
    })
}

fn record_acceptance(
    workspace_path: &Path,
    run_id: &str,
    comment: &ReviewComment,
    recorded_at: DateTime<Utc>,
) -> Result<(), String> {
    let mut store = load_accepted_findings(workspace_path)?;
    if let Some(existing) = store
        .accepted_findings
        .iter_mut()
        .find(|record| record.run_id == run_id && record.comment_id == comment.comment_id)
    {
        existing.recorded_at = recorded_at;
        existing.focus = comment.focus;
        existing.file = comment.file.clone();
        existing.line_start = comment.line_start;
        existing.line_end = comment.line_end;
        existing.title = comment.title.clone();
    } else {
        store.accepted_findings.push(ReviewOutcomeRecord {
            run_id: run_id.to_string(),
            comment_id: comment.comment_id.clone(),
            outcome: ReviewOutcome::Accepted,
            recorded_at,
            focus: comment.focus,
            file: comment.file.clone(),
            line_start: comment.line_start,
            line_end: comment.line_end,
            title: comment.title.clone(),
        });
    }
    save_accepted_findings(workspace_path, &store)?;

    clear_prior_rejection(workspace_path, comment)
}

fn clear_prior_rejection(workspace_path: &Path, comment: &ReviewComment) -> Result<(), String> {
    let rejected_store_path = workspace_path
        .join(".gospel")
        .join("rejected_findings.json");
    if !rejected_store_path.exists() {
        return Ok(());
    }

    let mut store = AntiPatternStore::load(workspace_path)?;
    if !store.remove_rejection(
        comment.focus,
        &comment.file,
        comment.line_start,
        comment.line_end,
        &comment.title,
    ) {
        return Ok(());
    }
    store.save(workspace_path)
}

fn record_rejection(
    workspace_path: &Path,
    comment: &ReviewComment,
    recorded_at: DateTime<Utc>,
) -> Result<(), String> {
    let mut store = AntiPatternStore::load(workspace_path)?;
    store.add_rejection_at(
        comment.focus,
        &comment.file,
        comment.line_start,
        comment.line_end,
        &comment.title,
        recorded_at,
    );
    store.save(workspace_path)
}

fn load_accepted_findings(workspace_path: &Path) -> Result<AcceptedFindingsStore, String> {
    let path = workspace_path
        .join(".gospel")
        .join("accepted_findings.json");
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).map_err(|error| {
            format!(
                "Failed to parse accepted findings store {}: {}",
                path.display(),
                error
            )
        }),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(AcceptedFindingsStore::default()),
        Err(error) => Err(format!(
            "Failed to read accepted findings store {}: {}",
            path.display(),
            error
        )),
    }
}

fn save_accepted_findings(
    workspace_path: &Path,
    store: &AcceptedFindingsStore,
) -> Result<(), String> {
    let gospel_dir = workspace_path.join(".gospel");
    fs::create_dir_all(&gospel_dir)
        .map_err(|error| format!("Failed to create .gospel directory: {}", error))?;
    let path = gospel_dir.join("accepted_findings.json");
    let content = serde_json::to_string_pretty(store)
        .map_err(|error| format!("Failed to serialize accepted findings: {}", error))?;
    write_file_atomically(&path, &content)
}

fn validate_id_segment(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.contains('/')
        || value.contains('\\')
        || value == "."
        || value == ".."
    {
        return Err("Review run id is invalid".to_string());
    }
    Ok(())
}

fn write_file_atomically(path: &Path, content: &str) -> Result<(), String> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let parent_dir = path
        .parent()
        .ok_or_else(|| "Store path has no parent directory".to_string())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "Store path has no file name".to_string())?;
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(file_name);
    temp_name.push(format!(".tmp.{}.{}", std::process::id(), count));
    let temp_path: PathBuf = parent_dir.join(temp_name);

    let mut file = fs::File::create(&temp_path)
        .map_err(|error| format!("Failed to create temp store file: {}", error))?;
    file.write_all(content.as_bytes()).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        format!("Failed to write temp store file: {}", error)
    })?;
    file.sync_all().map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        format!("Failed to sync temp store file: {}", error)
    })?;
    drop(file);

    fs::rename(&temp_path, path).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        format!("Failed to replace store file: {}", error)
    })?;
    let parent_handle = fs::File::open(parent_dir).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        format!(
            "Failed to open store directory {} for sync: {}",
            parent_dir.display(),
            error
        )
    })?;
    parent_handle.sync_all().map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        format!(
            "Failed to sync store directory {}: {}",
            parent_dir.display(),
            error
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{Severity, SignalTier};
    use tempfile::tempdir;

    fn comment() -> ReviewComment {
        ReviewComment {
            file: "src/main.rs".to_string(),
            line_start: 10,
            line_end: 12,
            severity: Severity::High,
            category: "injection".to_string(),
            focus: ReviewFocus::Security,
            focus_subcategory: None,
            cwe_id: Some("CWE-78".to_string()),
            cwe_name: Some("OS Command Injection".to_string()),
            title: "Unsanitized command".to_string(),
            description: "Description".to_string(),
            rationale: Some("Rationale".to_string()),
            evidence: "Command::new(\"sh\")".to_string(),
            suggestion: Some("Suggestion".to_string()),
            verification_plan: Some("Verify".to_string()),
            signal_tier: SignalTier::Tier1,
            comment_id: "rc_abc".to_string(),
        }
    }

    #[test]
    fn records_acceptance_after_validating_run_and_comment() {
        let dir = tempdir().unwrap();
        let run = ReviewRunRecord {
            run_id: "run-1".to_string(),
            timestamp: Utc::now(),
            focus: ReviewFocus::Security,
            mode: ReviewMode::Local,
            comments: vec![comment()],
        };
        save_review_run(dir.path(), &run).unwrap();

        let output =
            record_review_outcome(dir.path(), "run-1", "rc_abc", ReviewOutcome::Accepted).unwrap();

        assert!(output.success);
        let accepted =
            fs::read_to_string(dir.path().join(".gospel/accepted_findings.json")).unwrap();
        assert!(accepted.contains("rc_abc"));
        assert!(accepted.contains("recorded_at"));
    }

    #[test]
    fn refreshed_acceptance_updates_focus_and_anchor_fields() {
        let dir = tempdir().unwrap();
        let mut updated_comment = comment();
        updated_comment.focus = ReviewFocus::BugHunt;
        updated_comment.file = "src/lib.rs".to_string();
        updated_comment.line_start = 20;
        updated_comment.line_end = 22;
        updated_comment.title = "Incorrect state transition".to_string();
        let run = ReviewRunRecord {
            run_id: "run-1".to_string(),
            timestamp: Utc::now(),
            focus: ReviewFocus::BugHunt,
            mode: ReviewMode::Local,
            comments: vec![updated_comment],
        };
        save_review_run(dir.path(), &run).unwrap();
        save_accepted_findings(
            dir.path(),
            &AcceptedFindingsStore {
                accepted_findings: vec![ReviewOutcomeRecord {
                    run_id: "run-1".to_string(),
                    comment_id: "rc_abc".to_string(),
                    outcome: ReviewOutcome::Accepted,
                    recorded_at: Utc::now(),
                    focus: ReviewFocus::Security,
                    file: "src/main.rs".to_string(),
                    line_start: 10,
                    line_end: 12,
                    title: "Unsanitized command".to_string(),
                }],
            },
        )
        .unwrap();

        record_review_outcome(dir.path(), "run-1", "rc_abc", ReviewOutcome::Accepted).unwrap();

        let accepted = load_accepted_findings(dir.path()).unwrap();
        let record = &accepted.accepted_findings[0];
        assert_eq!(record.focus, ReviewFocus::BugHunt);
        assert_eq!(record.file, "src/lib.rs");
        assert_eq!(record.line_start, 20);
        assert_eq!(record.line_end, 22);
        assert_eq!(record.title, "Incorrect state transition");
    }

    #[test]
    fn rejects_unknown_comment_ids() {
        let dir = tempdir().unwrap();
        let run = ReviewRunRecord {
            run_id: "run-1".to_string(),
            timestamp: Utc::now(),
            focus: ReviewFocus::Security,
            mode: ReviewMode::Local,
            comments: vec![comment()],
        };
        save_review_run(dir.path(), &run).unwrap();

        let error = record_review_outcome(dir.path(), "run-1", "missing", ReviewOutcome::Rejected)
            .unwrap_err();

        assert!(error.contains("does not exist"));
    }

    #[test]
    fn acceptance_clears_prior_rejection_for_same_finding() {
        let dir = tempdir().unwrap();
        let run = ReviewRunRecord {
            run_id: "run-1".to_string(),
            timestamp: Utc::now(),
            focus: ReviewFocus::Security,
            mode: ReviewMode::Local,
            comments: vec![comment()],
        };
        save_review_run(dir.path(), &run).unwrap();

        record_review_outcome(dir.path(), "run-1", "rc_abc", ReviewOutcome::Rejected).unwrap();
        let mut other_focus_rejection = AntiPatternStore::load(dir.path()).unwrap();
        other_focus_rejection.add_rejection(
            ReviewFocus::Architecture,
            "src/main.rs",
            10,
            12,
            "Unsanitized command",
        );
        other_focus_rejection.save(dir.path()).unwrap();
        let rejected_before = AntiPatternStore::load(dir.path()).unwrap();
        assert!(rejected_before.is_rejected(
            ReviewFocus::Security,
            "src/main.rs",
            10,
            12,
            "Unsanitized command"
        ));
        assert!(rejected_before.is_rejected(
            ReviewFocus::Architecture,
            "src/main.rs",
            10,
            12,
            "Unsanitized command"
        ));

        record_review_outcome(dir.path(), "run-1", "rc_abc", ReviewOutcome::Accepted).unwrap();

        let rejected_after = AntiPatternStore::load(dir.path()).unwrap();
        assert!(!rejected_after.is_rejected(
            ReviewFocus::Security,
            "src/main.rs",
            10,
            12,
            "Unsanitized command"
        ));
        assert!(rejected_after.is_rejected(
            ReviewFocus::Architecture,
            "src/main.rs",
            10,
            12,
            "Unsanitized command"
        ));
    }
}
