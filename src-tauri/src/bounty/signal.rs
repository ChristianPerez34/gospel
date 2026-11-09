use reqwest::Url;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

use super::http_tool::sanitize_run_id;

#[derive(Error, Debug)]
pub enum BountySignalError {
    #[error("failed to prepare signals file: {0}")]
    Storage(String),
}

/// Hypothesis classes the bounty methodology looks for. `other` is the escape
/// hatch for findings outside the taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    /// Supposed to enforce something, but doesn't.
    EnforcementGap,
    /// Enforced in one place, assumed in another.
    InconsistentEnforcement,
    /// Relied on but never checked.
    UncheckedAssumption,
    /// Depends on undocumented behavior that may change.
    UndocumentedDependency,
    /// Safe alone, dangerous chained.
    CompositionVulnerability,
    /// Same object reachable through differently-permissioned paths.
    InconsistentAccessPath,
    /// Object identity rebound without required checks.
    IdentityRebinding,
    /// Rejected request still causes a side effect.
    FailedRequestSideEffect,
    /// Check holds only under a specific event order.
    OrderingDependency,
    /// Rolled-back transaction leaves side effects behind.
    PartialRollbackSideEffect,
    /// Deleted or revoked object remains usable.
    PostDeleteAccess,
    /// State reversal possible without permission.
    UnauthorizedStateReversal,
    /// Object holds two contradictory states at once.
    ContradictoryState,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalConfidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalStatus {
    Candidate,
    Confirmed,
    Rejected,
    NeedsReview,
}

fn default_confidence() -> SignalConfidence {
    SignalConfidence::Medium
}

fn default_status() -> SignalStatus {
    SignalStatus::Candidate
}

#[derive(Debug, Deserialize)]
pub struct BountySignalArgs {
    pub kind: SignalKind,
    /// URL or path the signal is about.
    pub endpoint: String,
    #[serde(default)]
    pub method: Option<String>,
    /// One-paragraph description of the observed behavior and why it matters.
    pub summary: String,
    /// Evidence ids returned by `http_request` calls that demonstrate the signal.
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default = "default_confidence")]
    pub confidence: SignalConfidence,
    #[serde(default = "default_status")]
    pub status: SignalStatus,
    /// The hypothesis this signal tested, if it came from the mutator or plan.
    #[serde(default)]
    pub hypothesis: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalRecord {
    pub id: String,
    pub created_at: String,
    pub kind: SignalKind,
    pub endpoint: String,
    pub method: String,
    pub summary: String,
    #[serde(default)]
    pub hypothesis: Option<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    pub confidence: SignalConfidence,
    pub status: SignalStatus,
}

#[derive(Debug, Serialize)]
pub struct BountySignalOutput {
    pub signal_id: String,
    pub deduplicated: bool,
    pub summary: String,
}

/// Normalizes an endpoint into a dedup-comparable form: lowercase host plus
/// path, query and fragment dropped, trailing slash removed.
fn normalize_endpoint(raw: &str) -> String {
    let trimmed = raw.trim();
    match Url::parse(trimmed) {
        Ok(url) => {
            let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
            let path = url.path().trim_end_matches('/');
            let path = if path.is_empty() { "/" } else { path };
            format!("{host}{path}")
        }
        Err(_) => trimmed.to_ascii_lowercase().trim_end_matches('/').to_string(),
    }
}

fn kind_key(kind: SignalKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "other".to_string())
}

fn dedup_key(kind: SignalKind, method: &str, endpoint: &str) -> String {
    format!(
        "{}|{}|{}",
        kind_key(kind),
        method.to_ascii_uppercase(),
        normalize_endpoint(endpoint)
    )
}

pub(crate) fn load_signals(path: &Path) -> Vec<SignalRecord> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(|line| serde_json::from_str::<SignalRecord>(line).ok())
        .collect()
}

#[derive(Debug, Clone)]
pub struct BountySignalTool {
    signals_path: PathBuf,
}

impl BountySignalTool {
    pub fn new(workspace_path: &Path, run_id: &str) -> Result<Self, BountySignalError> {
        let run_dir = workspace_path
            .join(".gospel")
            .join("bounty")
            .join("runs")
            .join(sanitize_run_id(run_id));
        std::fs::create_dir_all(&run_dir).map_err(|error| {
            BountySignalError::Storage(error.to_string())
        })?;
        Ok(Self {
            signals_path: run_dir.join("signals.jsonl"),
        })
    }

    fn append(&self, record: &SignalRecord) -> Result<(), String> {
        let line = serde_json::to_string(record).map_err(|error| error.to_string())?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.signals_path)
            .map_err(|error| error.to_string())?;
        writeln!(file, "{line}").map_err(|error| error.to_string())
    }
}

impl Tool for BountySignalTool {
    const NAME: &'static str = "record_bounty_signal";

    type Error = BountySignalError;
    type Args = BountySignalArgs;
    type Output = BountySignalOutput;

    fn description(&self) -> String {
        "Record one evidence-backed bounty signal for this run. Signals are deduplicated on (kind, method, endpoint): a repeat returns the existing signal id instead of writing. Link every claim to evidence_ids from earlier http_request calls so the signal can be triaged.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": [
                        "enforcement_gap",
                        "inconsistent_enforcement",
                        "unchecked_assumption",
                        "undocumented_dependency",
                        "composition_vulnerability",
                        "inconsistent_access_path",
                        "identity_rebinding",
                        "failed_request_side_effect",
                        "ordering_dependency",
                        "partial_rollback_side_effect",
                        "post_delete_access",
                        "unauthorized_state_reversal",
                        "contradictory_state",
                        "other"
                    ]
                },
                "endpoint": {
                    "type": "string",
                    "description": "URL or path the signal is about."
                },
                "method": {
                    "type": "string",
                    "description": "HTTP method the signal was observed with, if applicable."
                },
                "summary": {
                    "type": "string",
                    "description": "What was observed and why it may be a security issue."
                },
                "evidence_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Evidence ids returned by earlier http_request calls."
                },
                "confidence": {
                    "type": "string",
                    "enum": ["low", "medium", "high"]
                },
                "status": {
                    "type": "string",
                    "enum": ["candidate", "confirmed", "rejected", "needs_review"]
                },
                "hypothesis": {
                    "type": "string",
                    "description": "The hypothesis this signal tested, if any."
                }
            },
            "required": ["kind", "endpoint", "summary"]
        })
    }

    fn map_error(&self, error: Self::Error) -> ToolExecutionError {
        ToolExecutionError::other(error.to_string())
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let method = args
            .method
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase();
        let key = dedup_key(args.kind, &method, &args.endpoint);

        for existing in load_signals(&self.signals_path) {
            if dedup_key(existing.kind, &existing.method, &existing.endpoint) == key {
                return Ok(BountySignalOutput {
                    signal_id: existing.id,
                    deduplicated: true,
                    summary: format!(
                        "Duplicate of existing signal; not recorded again: {}",
                        args.summary
                    ),
                });
            }
        }

        let record = SignalRecord {
            id: Uuid::new_v4().to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            kind: args.kind,
            endpoint: args.endpoint.trim().to_string(),
            method,
            summary: args.summary,
            hypothesis: args.hypothesis,
            evidence_ids: args.evidence_ids,
            confidence: args.confidence,
            status: args.status,
        };
        self.append(&record)
            .map_err(BountySignalError::Storage)?;
        Ok(BountySignalOutput {
            signal_id: record.id,
            deduplicated: false,
            summary: "Signal recorded.".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_endpoint_drops_query_fragment_and_case() {
        assert_eq!(
            normalize_endpoint("HTTPS://Example.COM/api/users/5/?q=1#frag"),
            "example.com/api/users/5"
        );
        assert_eq!(
            normalize_endpoint("https://example.com"),
            "example.com/"
        );
        assert_eq!(
            normalize_endpoint("/Admin/Panel/"),
            "/admin/panel"
        );
    }

    #[test]
    fn dedup_key_matches_across_normalization() {
        let a = dedup_key(
            SignalKind::EnforcementGap,
            "get",
            "https://example.com/api/1/?x=1",
        );
        let b = dedup_key(
            SignalKind::EnforcementGap,
            "GET",
            "https://EXAMPLE.com/api/1",
        );
        assert_eq!(a, b);
    }

    #[test]
    fn dedup_key_differs_by_kind() {
        let a = dedup_key(SignalKind::EnforcementGap, "GET", "https://e.com/a");
        let b = dedup_key(SignalKind::PostDeleteAccess, "GET", "https://e.com/a");
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn append_then_duplicate_returns_existing_id() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BountySignalTool::new(dir.path(), "run-1").unwrap();
        let mut context = ToolContext::default();
        let args = BountySignalArgs {
            kind: SignalKind::InconsistentAccessPath,
            endpoint: "https://t.example/api/me".to_string(),
            method: Some("GET".to_string()),
            summary: "first".to_string(),
            evidence_ids: vec!["e1".to_string()],
            confidence: SignalConfidence::High,
            status: SignalStatus::Candidate,
            hypothesis: None,
        };
        let first = tool.call(&mut context, args).await.unwrap();
        assert!(!first.deduplicated);

        let args = BountySignalArgs {
            kind: SignalKind::InconsistentAccessPath,
            endpoint: "https://T.EXAMPLE/api/me?other=1".to_string(),
            method: Some("get".to_string()),
            summary: "second".to_string(),
            evidence_ids: vec![],
            confidence: SignalConfidence::Low,
            status: SignalStatus::Candidate,
            hypothesis: None,
        };
        let second = tool.call(&mut context, args).await.unwrap();
        assert!(second.deduplicated);
        assert_eq!(second.signal_id, first.signal_id);

        let signals = load_signals(&tool.signals_path);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].summary, "first");
        assert_eq!(signals[0].evidence_ids, vec!["e1".to_string()]);
    }

    #[tokio::test]
    async fn distinct_kinds_on_same_endpoint_both_record() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BountySignalTool::new(dir.path(), "run-1").unwrap();
        let mut context = ToolContext::default();
        for kind in [SignalKind::OrderingDependency, SignalKind::ContradictoryState] {
            let args = BountySignalArgs {
                kind,
                endpoint: "https://t.example/cart".to_string(),
                method: None,
                summary: "s".to_string(),
                evidence_ids: vec![],
                confidence: SignalConfidence::Medium,
                status: SignalStatus::Candidate,
                hypothesis: None,
            };
            let out = tool.call(&mut context, args).await.unwrap();
            assert!(!out.deduplicated);
        }
        assert_eq!(load_signals(&tool.signals_path).len(), 2);
    }
}
