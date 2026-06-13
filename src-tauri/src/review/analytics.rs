use super::signal::{percentage, SignalTier};
use super::{ReviewComment, ReviewMode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewCategoryMetrics {
    pub total: usize,
    pub tier1: usize,
    pub tier2: usize,
    pub noise: usize,
    pub unclassified: usize,
}

impl ReviewCategoryMetrics {
    fn record(&mut self, tier: SignalTier) {
        self.total += 1;
        match tier {
            SignalTier::Tier1 => self.tier1 += 1,
            SignalTier::Tier2 => self.tier2 += 1,
            SignalTier::Noise => self.noise += 1,
            SignalTier::Unclassified => self.unclassified += 1,
        }
    }

    #[allow(dead_code)]
    fn merge(&mut self, other: &ReviewCategoryMetrics) {
        self.total += other.total;
        self.tier1 += other.tier1;
        self.tier2 += other.tier2;
        self.noise += other.noise;
        self.unclassified += other.unclassified;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewMetricsRecord {
    pub run_id: String,
    pub timestamp: DateTime<Utc>,
    pub mode: ReviewMode,
    pub provider: String,
    pub model: String,
    pub total: usize,
    pub tier1: usize,
    pub tier2: usize,
    pub noise: usize,
    pub unclassified: usize,
    pub snr_percent: f64,
    pub categories: BTreeMap<String, ReviewCategoryMetrics>,
    pub files_scanned: usize,
    pub pr_number: Option<u64>,
    pub user_visible: bool,
}

impl ReviewMetricsRecord {
    pub fn from_comments(
        run_id: String,
        timestamp: DateTime<Utc>,
        mode: ReviewMode,
        provider: String,
        model: String,
        comments: &[ReviewComment],
        files_scanned: usize,
        user_visible: bool,
    ) -> Self {
        let mut record = Self {
            run_id,
            timestamp,
            pr_number: pr_number_for_mode(&mode),
            mode,
            provider,
            model,
            total: 0,
            tier1: 0,
            tier2: 0,
            noise: 0,
            unclassified: 0,
            snr_percent: 100.0,
            categories: BTreeMap::new(),
            files_scanned,
            user_visible,
        };

        for comment in comments {
            record.total += 1;
            match comment.signal_tier {
                SignalTier::Tier1 => record.tier1 += 1,
                SignalTier::Tier2 => record.tier2 += 1,
                SignalTier::Noise => record.noise += 1,
                SignalTier::Unclassified => record.unclassified += 1,
            }

            let category = normalized_category_name(&comment.category);
            record
                .categories
                .entry(category)
                .or_insert_with(|| ReviewCategoryMetrics {
                    total: 0,
                    tier1: 0,
                    tier2: 0,
                    noise: 0,
                    unclassified: 0,
                })
                .record(comment.signal_tier);
        }

        record.snr_percent = percentage(record.tier1 + record.tier2, record.total);
        record
    }
}

pub fn append_review_metrics(
    workspace_path: &Path,
    record: &ReviewMetricsRecord,
) -> Result<(), String> {
    let gospel_dir = workspace_path.join(".gospel");
    fs::create_dir_all(&gospel_dir)
        .map_err(|error| format!("Failed to create .gospel directory: {}", error))?;
    let path = gospel_dir.join("review_metrics.jsonl");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| {
            format!(
                "Failed to open review metrics {}: {}",
                path.display(),
                error
            )
        })?;
    serde_json::to_writer(&mut file, record)
        .map_err(|error| format!("Failed to serialize review metrics: {}", error))?;
    file.write_all(b"\n")
        .map_err(|error| format!("Failed to append review metrics: {}", error))?;
    file.flush()
        .map_err(|error| format!("Failed to flush review metrics: {}", error))
}

#[allow(dead_code)]
pub fn load_review_metrics(workspace_path: &Path) -> Result<Vec<ReviewMetricsRecord>, String> {
    let path = workspace_path.join(".gospel").join("review_metrics.jsonl");
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "Failed to open review metrics {}: {}",
                path.display(),
                error
            ));
        }
    };

    let mut records = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line =
            line.map_err(|error| format!("Failed to read review metrics line: {}", error))?;
        if line.trim().is_empty() {
            continue;
        }
        let record: ReviewMetricsRecord = serde_json::from_str(&line).map_err(|error| {
            format!(
                "Failed to parse review metrics line {} in {}: {}",
                index + 1,
                path.display(),
                error
            )
        })?;
        records.push(record);
    }
    Ok(records)
}

#[allow(dead_code)]
pub fn snr_per_mode(workspace_path: &Path) -> Result<BTreeMap<String, f64>, String> {
    let records = load_review_metrics(workspace_path)?;
    let mut totals: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for record in records {
        let key = mode_key(&record.mode);
        let entry = totals.entry(key).or_insert((0, 0));
        entry.0 += record.tier1 + record.tier2;
        entry.1 += record.total;
    }

    Ok(totals
        .into_iter()
        .map(|(mode, (actionable, total))| (mode, percentage(actionable, total)))
        .collect())
}

#[allow(dead_code)]
pub fn snr_per_category(workspace_path: &Path) -> Result<BTreeMap<String, f64>, String> {
    let records = load_review_metrics(workspace_path)?;
    let mut totals: BTreeMap<String, ReviewCategoryMetrics> = BTreeMap::new();
    for record in records {
        for (category, metrics) in record.categories {
            totals
                .entry(category)
                .or_insert(ReviewCategoryMetrics {
                    total: 0,
                    tier1: 0,
                    tier2: 0,
                    noise: 0,
                    unclassified: 0,
                })
                .merge(&metrics);
        }
    }

    Ok(totals
        .into_iter()
        .map(|(category, metrics)| {
            (
                category,
                percentage(metrics.tier1 + metrics.tier2, metrics.total),
            )
        })
        .collect())
}

#[allow(dead_code)]
pub fn snr_last_n_runs(workspace_path: &Path, n: usize) -> Result<f64, String> {
    let records = load_review_metrics(workspace_path)?;
    let take = n.min(records.len());
    let start = records.len().saturating_sub(take);
    let mut actionable = 0usize;
    let mut total = 0usize;
    for record in records.into_iter().skip(start) {
        actionable += record.tier1 + record.tier2;
        total += record.total;
    }
    Ok(percentage(actionable, total))
}

fn pr_number_for_mode(mode: &ReviewMode) -> Option<u64> {
    match mode {
        ReviewMode::PullRequest { pr_number } => Some(*pr_number),
        _ => None,
    }
}

#[allow(dead_code)]
fn mode_key(mode: &ReviewMode) -> String {
    match mode {
        ReviewMode::Local => "local".to_string(),
        ReviewMode::PullRequest { .. } => "pr".to_string(),
        ReviewMode::FullScan => "scan".to_string(),
    }
}

fn normalized_category_name(category: &str) -> String {
    let value = category.trim();
    if value.is_empty() {
        "uncategorized".to_string()
    } else {
        value.to_ascii_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{Severity, SignalTier};
    use tempfile::tempdir;

    fn comment(category: &str, tier: SignalTier) -> ReviewComment {
        ReviewComment {
            file: "src/main.rs".to_string(),
            line_start: 1,
            line_end: 1,
            severity: Severity::Medium,
            category: category.to_string(),
            cwe_id: None,
            cwe_name: None,
            title: "Finding".to_string(),
            description: "Description".to_string(),
            rationale: Some("Rationale".to_string()),
            evidence: "evidence".to_string(),
            suggestion: Some("Suggestion".to_string()),
            verification_plan: Some("Verify".to_string()),
            signal_tier: tier,
            comment_id: "rc_test".to_string(),
        }
    }

    #[test]
    fn appends_metrics_as_jsonl_without_rewriting_existing_lines() {
        let dir = tempdir().unwrap();
        let record = ReviewMetricsRecord::from_comments(
            "run-1".to_string(),
            Utc::now(),
            ReviewMode::Local,
            "openai".to_string(),
            "model".to_string(),
            &[comment("injection", SignalTier::Tier1)],
            2,
            true,
        );

        append_review_metrics(dir.path(), &record).unwrap();
        append_review_metrics(dir.path(), &record).unwrap();

        let content = fs::read_to_string(dir.path().join(".gospel/review_metrics.jsonl")).unwrap();
        assert_eq!(content.lines().count(), 2);
    }

    #[test]
    fn computes_snr_by_mode_category_and_recent_runs() {
        let dir = tempdir().unwrap();
        let first = ReviewMetricsRecord::from_comments(
            "run-1".to_string(),
            Utc::now(),
            ReviewMode::Local,
            "openai".to_string(),
            "model".to_string(),
            &[
                comment("injection", SignalTier::Tier1),
                comment("style", SignalTier::Noise),
            ],
            2,
            true,
        );
        let second = ReviewMetricsRecord::from_comments(
            "run-2".to_string(),
            Utc::now(),
            ReviewMode::FullScan,
            "openai".to_string(),
            "model".to_string(),
            &[comment("style", SignalTier::Noise)],
            1,
            false,
        );

        append_review_metrics(dir.path(), &first).unwrap();
        append_review_metrics(dir.path(), &second).unwrap();

        assert_eq!(snr_per_mode(dir.path()).unwrap()["local"], 50.0);
        assert_eq!(snr_per_category(dir.path()).unwrap()["style"], 0.0);
        assert_eq!(snr_last_n_runs(dir.path(), 1).unwrap(), 0.0);
    }
}
