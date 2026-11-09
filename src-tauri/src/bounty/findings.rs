use serde::Serialize;
use std::path::{Path, PathBuf};

use super::signal::{load_signals, SignalRecord};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BountyFindings {
    pub run_id: Option<String>,
    /// Run directory names, most recently modified first.
    pub runs: Vec<String>,
    /// True when the selected run's threat-model checkpoint awaits approval.
    pub checkpoint_pending: bool,
    pub signals: Vec<SignalRecord>,
    pub verdicts: Vec<serde_json::Value>,
    pub report: Option<String>,
}

fn runs_root(workspace_path: &Path) -> PathBuf {
    workspace_path.join(".gospel").join("bounty").join("runs")
}

/// Run directories, most recently modified first.
fn run_dirs(workspace_path: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<(std::time::SystemTime, PathBuf)> =
        match std::fs::read_dir(runs_root(workspace_path)) {
            Ok(read) => read
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    if !entry.file_type().ok()?.is_dir() {
                        return None;
                    }
                    let modified = entry
                        .metadata()
                        .ok()
                        .and_then(|meta| meta.modified().ok())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    Some((modified, entry.path()))
                })
                .collect(),
            Err(_) => Vec::new(),
        };
    entries.sort_by_key(|(modified, _)| *modified);
    entries.reverse();
    entries.into_iter().map(|(_, path)| path).collect()
}

fn load_jsonl(path: &Path) -> Vec<serde_json::Value> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect()
}

pub fn load_findings(workspace_path: &Path, run_id: Option<&str>) -> BountyFindings {
    let dirs = run_dirs(workspace_path);
    let runs: Vec<String> = dirs
        .iter()
        .filter_map(|dir| dir.file_name().map(|name| name.to_string_lossy().into_owned()))
        .collect();
    let dir = match run_id {
        Some(id) => {
            let dir = runs_root(workspace_path)
                .join(super::http_tool::sanitize_run_id(id));
            dir.is_dir().then_some(dir)
        }
        None => dirs.first().cloned(),
    };
    let Some(dir) = dir else {
        return BountyFindings {
            run_id: None,
            runs,
            checkpoint_pending: false,
            signals: Vec::new(),
            verdicts: Vec::new(),
            report: None,
        };
    };
    // Later verdict lines win: model triage first, user overrides appended.
    let mut verdicts: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    for verdict in load_jsonl(&dir.join("triage.jsonl")) {
        if let Some(id) = verdict.get("id").and_then(|id| id.as_str()) {
            verdicts.insert(id.to_string(), verdict);
        }
    }
    BountyFindings {
        run_id: dir.file_name().map(|name| name.to_string_lossy().into_owned()),
        runs,
        checkpoint_pending: super::checkpoint::checkpoint_state(&dir)
            == super::checkpoint::CheckpointState::Pending,
        signals: load_signals(&dir.join("signals.jsonl")),
        verdicts: verdicts.into_values().collect(),
        report: std::fs::read_to_string(dir.join("sweep-report.md")).ok(),
    }
}

const VALID_VERDICT_STATUSES: &[&str] = &["confirmed", "needs_review", "rejected", "candidate"];

/// Appends a user verdict to `triage.jsonl`; latest line wins on load.
pub fn record_user_verdict(
    workspace_path: &Path,
    run_id: &str,
    signal_id: &str,
    status: &str,
    severity: Option<&str>,
    rationale: Option<&str>,
) -> Result<(), String> {
    if !VALID_VERDICT_STATUSES.contains(&status) {
        return Err(format!(
            "invalid verdict status '{status}'; expected one of {VALID_VERDICT_STATUSES:?}"
        ));
    }
    let dir = runs_root(workspace_path)
        .join(super::http_tool::sanitize_run_id(run_id));
    if !dir.is_dir() {
        return Err(format!("bounty run '{run_id}' does not exist"));
    }
    let verdict = serde_json::json!({
        "id": signal_id,
        "status": status,
        "severity": severity,
        "rationale": rationale,
        "source": "user",
        "ts": chrono::Utc::now().to_rfc3339(),
    });
    let line = serde_json::to_string(&verdict).map_err(|e| e.to_string())?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("triage.jsonl"))
        .map_err(|e| e.to_string())?;
    writeln!(file, "{line}").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn empty_when_no_runs() {
        let dir = tempfile::tempdir().unwrap();
        let findings = load_findings(dir.path(), None);
        assert!(findings.run_id.is_none());
        assert!(findings.signals.is_empty());
    }

    #[test]
    fn loads_latest_run_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let run = dir
            .path()
            .join(".gospel/bounty/runs/run-a");
        std::fs::create_dir_all(&run).unwrap();
        let mut signal = std::fs::File::create(run.join("signals.jsonl")).unwrap();
        writeln!(
            signal,
            "{}",
            serde_json::json!({
                "id": "s1",
                "created_at": "t",
                "kind": "enforcement_gap",
                "endpoint": "https://e/a",
                "method": "GET",
                "summary": "sum",
                "confidence": "high",
                "status": "candidate"
            })
        )
        .unwrap();
        let mut triage = std::fs::File::create(run.join("triage.jsonl")).unwrap();
        writeln!(triage, "{}", serde_json::json!({"id":"s1","status":"confirmed"})).unwrap();
        std::fs::write(run.join("sweep-report.md"), "# report").unwrap();

        let findings = load_findings(dir.path(), None);
        assert_eq!(findings.run_id.as_deref(), Some("run-a"));
        assert_eq!(findings.runs, vec!["run-a".to_string()]);
        assert_eq!(findings.signals.len(), 1);
        assert_eq!(findings.signals[0].id, "s1");
        assert_eq!(findings.verdicts.len(), 1);
        assert_eq!(findings.report.as_deref(), Some("# report"));

        let findings = load_findings(dir.path(), Some("run-a"));
        assert_eq!(findings.signals.len(), 1);
        let findings = load_findings(dir.path(), Some("missing"));
        assert!(findings.run_id.is_none());
    }

    #[test]
    fn user_verdict_overrides_model_verdict() {
        let dir = tempfile::tempdir().unwrap();
        let run = dir.path().join(".gospel/bounty/runs/run-a");
        std::fs::create_dir_all(&run).unwrap();
        let mut triage = std::fs::File::create(run.join("triage.jsonl")).unwrap();
        writeln!(
            triage,
            "{}",
            serde_json::json!({"id": "s1", "status": "rejected"})
        )
        .unwrap();
        record_user_verdict(dir.path(), "run-a", "s1", "confirmed", Some("high"), None).unwrap();
        let findings = load_findings(dir.path(), Some("run-a"));
        assert_eq!(findings.verdicts.len(), 1);
        assert_eq!(findings.verdicts[0]["status"], "confirmed");
        assert_eq!(findings.verdicts[0]["source"], "user");

        assert!(record_user_verdict(dir.path(), "run-a", "s1", "bogus", None, None).is_err());
        assert!(record_user_verdict(dir.path(), "no-run", "s1", "confirmed", None, None).is_err());
    }
}
