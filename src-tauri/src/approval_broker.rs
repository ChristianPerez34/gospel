use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;
use uuid::Uuid;

/// How long a pending approval request waits for a frontend decision before
/// auto-denying. Matches the prior Tauri dialog behavior.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the broker waits for a frontend decision before auto-denying.
/// Configurable on `ApprovalBroker::with_timeout` so tests can drive the
/// timeout path without wall-clock waits.
pub type ApprovalTimeout = Duration;

/// Tauri event name emitted when a new approval request is created and needs
/// to be surfaced in the live agent/tool timeline.
pub const APPROVAL_REQUESTED_EVENT: &str = "approval-requested";

/// Tauri event name emitted after a decision is recorded (or after the
/// broker auto-denies on timeout). The frontend uses this to flip a card's
/// status without waiting for the next tool result.
pub const APPROVAL_RESOLVED_EVENT: &str = "approval-resolved";

/// What the agent was about to do that requires user approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    /// A shell/git/gh command classified as mutating or destructive.
    Command,
    /// A workspace tool reading from a path outside the active workspace.
    ExternalPath,
}

/// Severity classification used by the UI to color and label the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRisk {
    Mutating,
    Destructive,
    ExternalAccess,
}

/// A request the broker surfaces to the frontend and waits on.
///
/// The broker keeps this struct the only thing the UI needs to render — every
/// redacted detail for display (summary, reason, risk) travels in the payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub kind: ApprovalKind,
    pub tool_name: String,
    pub title: String,
    pub summary: String,
    pub reason: String,
    pub risk: ApprovalRisk,
}

impl ApprovalRequest {
    pub fn command(
        tool_name: impl Into<String>,
        command_label: impl Into<String>,
        reason: impl Into<String>,
        destructive: bool,
    ) -> Self {
        let tool_name = tool_name.into();
        let command_label = command_label.into();
        Self {
            id: Uuid::new_v4().to_string(),
            kind: ApprovalKind::Command,
            title: if destructive {
                "Allow destructive command?"
            } else {
                "Allow mutating command?"
            }
            .to_string(),
            summary: command_label.clone(),
            reason: reason.into(),
            risk: if destructive {
                ApprovalRisk::Destructive
            } else {
                ApprovalRisk::Mutating
            },
            tool_name,
        }
    }

    pub fn external_path(
        tool_name: impl Into<String>,
        path: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            kind: ApprovalKind::ExternalPath,
            title: "Allow external file access?".to_string(),
            summary: path.into(),
            reason: reason.into(),
            risk: ApprovalRisk::ExternalAccess,
            tool_name: tool_name.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalOutcome {
    Approved,
    Denied,
    TimedOut,
}

impl ApprovalOutcome {
    pub fn from_decision(decision: Option<ApprovalDecision>, timed_out: bool) -> Self {
        if timed_out {
            ApprovalOutcome::TimedOut
        } else {
            match decision {
                Some(ApprovalDecision::Approve) => ApprovalOutcome::Approved,
                _ => ApprovalOutcome::Denied,
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResolution {
    pub id: String,
    pub outcome: ApprovalOutcome,
}

/// What the broker needs from the host to publish events. Keeping this as a
/// trait lets tests pass in a recording fake instead of a real Tauri app.
pub trait ApprovalEventEmitter: Send + Sync {
    fn emit_requested(&self, request: &ApprovalRequest);
    fn emit_resolved(&self, resolution: &ApprovalResolution);
}

/// In-memory broker that owns the lifetime of every pending approval request.
///
/// Design notes (deep module):
/// - The interface is a single `request` method + two emitters. Callers say
///   "ask the broker" — they never touch oneshots, timers, or Tauri events.
/// - All complexity (UUID minting, channel wiring, timeout, cleanup,
///   outcome derivation) lives behind `request` so adding "always allow"
///   persistence later is a single-file change.
pub struct ApprovalBroker {
    pending: Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>,
    emitter: Arc<dyn ApprovalEventEmitter>,
    timeout: ApprovalTimeout,
}

impl ApprovalBroker {
    pub fn new(emitter: Arc<dyn ApprovalEventEmitter>) -> Self {
        Self::with_timeout(emitter, APPROVAL_TIMEOUT)
    }

    /// Build a broker with a custom decision timeout. Production callers
    /// should use [`ApprovalBroker::new`]; tests use this to drive the
    /// timeout path without wall-clock waits.
    pub fn with_timeout(emitter: Arc<dyn ApprovalEventEmitter>, timeout: ApprovalTimeout) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            emitter,
            timeout,
        }
    }

    /// Register a request, emit it to the frontend, and wait for a decision.
    ///
    /// Returns `true` only when the frontend explicitly approves before the
    /// timeout elapses. Anything else (deny, timeout, dropped channel)
    /// resolves to `false` so callers default to "do not execute".
    pub async fn request(&self, request: ApprovalRequest) -> bool {
        let id = request.id.clone();

        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.pending.lock().expect("approval broker poisoned");
            pending.insert(id.clone(), sender);
        }

        self.emitter.emit_requested(&request);

        let timed_out;
        let decision = match tokio::time::timeout(self.timeout, receiver).await {
            Ok(Ok(decision)) => {
                timed_out = false;
                Some(decision)
            }
            Ok(Err(_dropped)) => {
                timed_out = false;
                None
            }
            Err(_elapsed) => {
                timed_out = true;
                None
            }
        };

        {
            let mut pending = self.pending.lock().expect("approval broker poisoned");
            pending.remove(&id);
        }

        let outcome = ApprovalOutcome::from_decision(decision, timed_out);
        self.emitter
            .emit_resolved(&ApprovalResolution { id, outcome });

        matches!(outcome, ApprovalOutcome::Approved)
    }

    /// Frontend-driven decision. Returns `true` if a pending request was
    /// found and notified. Idempotent: resolving an already-resolved request
    /// is a no-op (returns `false`).
    pub fn resolve(&self, id: &str, decision: ApprovalDecision) -> bool {
        let sender = {
            let mut pending = self.pending.lock().expect("approval broker poisoned");
            pending.remove(id)
        };
        match sender {
            Some(sender) => sender.send(decision).is_ok(),
            None => false,
        }
    }

    /// Number of currently pending requests. Exposed for diagnostics and tests.
    pub fn pending_count(&self) -> usize {
        self.pending.lock().expect("approval broker poisoned").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct RecordingEmitter {
        requested: Mutex<Vec<ApprovalRequest>>,
        resolved: Mutex<Vec<ApprovalResolution>>,
    }

    impl RecordingEmitter {
        fn new() -> Self {
            Self {
                requested: Mutex::new(Vec::new()),
                resolved: Mutex::new(Vec::new()),
            }
        }
    }

    impl ApprovalEventEmitter for RecordingEmitter {
        fn emit_requested(&self, request: &ApprovalRequest) {
            self.requested.lock().unwrap().push(request.clone());
        }
        fn emit_resolved(&self, resolution: &ApprovalResolution) {
            self.resolved.lock().unwrap().push(resolution.clone());
        }
    }

    fn test_broker() -> (Arc<ApprovalBroker>, Arc<RecordingEmitter>) {
        test_broker_with_timeout(Duration::from_millis(50))
    }

    fn test_broker_with_timeout(timeout: Duration) -> (Arc<ApprovalBroker>, Arc<RecordingEmitter>) {
        let emitter = Arc::new(RecordingEmitter::new());
        let broker = Arc::new(ApprovalBroker::with_timeout(emitter.clone(), timeout));
        (broker, emitter)
    }

    #[tokio::test]
    async fn request_emits_event_and_waits_for_decision() {
        let (broker, emitter) = test_broker();
        let request = ApprovalRequest::command(
            "run_shell_command",
            "git push origin main",
            "Pushes to remote.",
            false,
        );
        let id = request.id.clone();
        let broker_for_task = broker.clone();
        let task = tokio::spawn(async move { broker_for_task.request(request).await });

        // Poll until the spawned task has registered and emitted.
        for _ in 0..64 {
            if broker.pending_count() == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(broker.pending_count(), 1);
        assert_eq!(emitter.requested.lock().unwrap().len(), 1);
        assert_eq!(emitter.requested.lock().unwrap()[0].id, id);

        assert!(broker.resolve(&id, ApprovalDecision::Approve));
        let approved = task.await.unwrap();
        assert!(approved);
        assert_eq!(broker.pending_count(), 0);
        assert_eq!(emitter.resolved.lock().unwrap().len(), 1);
        assert_eq!(
            emitter.resolved.lock().unwrap()[0].outcome,
            ApprovalOutcome::Approved
        );
    }

    #[tokio::test]
    async fn deny_decision_returns_false_and_resolves_as_denied() {
        let (broker, emitter) = test_broker();
        let request =
            ApprovalRequest::external_path("read_file", "/etc/passwd", "Outside workspace.");
        let id = request.id.clone();
        let broker_for_task = broker.clone();

        let task = tokio::spawn(async move { broker_for_task.request(request).await });
        for _ in 0..64 {
            if broker.pending_count() == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }

        assert!(broker.resolve(&id, ApprovalDecision::Deny));
        let approved = task.await.unwrap();
        assert!(!approved);
        assert_eq!(
            emitter.resolved.lock().unwrap()[0].outcome,
            ApprovalOutcome::Denied
        );
    }

    #[tokio::test]
    async fn resolve_unknown_id_is_a_noop() {
        let (broker, _) = test_broker();
        assert!(!broker.resolve("nonexistent", ApprovalDecision::Approve));
    }

    #[tokio::test]
    async fn timeout_auto_denies_request() {
        // 10ms timeout keeps the test fast and avoids the production 60s.
        let (broker, emitter) = test_broker_with_timeout(Duration::from_millis(10));
        let request =
            ApprovalRequest::command("run_shell_command", "rm -rf build", "Removes files.", true);
        let id = request.id.clone();

        let approved = broker.request(request).await;
        assert!(!approved);
        assert_eq!(
            emitter.resolved.lock().unwrap()[0].id,
            id,
            "timeout must emit a resolution with the request id"
        );
        assert_eq!(
            emitter.resolved.lock().unwrap()[0].outcome,
            ApprovalOutcome::TimedOut
        );
        assert_eq!(emitter.resolved.lock().unwrap().len(), 1);
        assert_eq!(broker.pending_count(), 0);
    }

    #[tokio::test]
    async fn second_resolve_after_approval_is_a_noop() {
        let (broker, _emitter) = test_broker();
        let request = ApprovalRequest::external_path("read_file", "/tmp/x", "Outside workspace.");
        let id = request.id.clone();
        let broker_for_task = broker.clone();

        let task = tokio::spawn(async move { broker_for_task.request(request).await });
        for _ in 0..64 {
            if broker.pending_count() == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }

        assert!(broker.resolve(&id, ApprovalDecision::Approve));
        let _ = task.await.unwrap();
        // After the first resolve the pending entry is removed; second call is
        // a no-op (returns false) without panicking.
        assert!(!broker.resolve(&id, ApprovalDecision::Approve));
    }

    #[test]
    fn request_factories_set_kind_risk_and_ids() {
        let mut ids = std::collections::HashSet::new();
        for _ in 0..32 {
            let cmd = ApprovalRequest::command("run_shell_command", "ls", "x", true);
            let ext = ApprovalRequest::external_path("read_file", "/tmp/x", "y");
            assert_eq!(cmd.kind, ApprovalKind::Command);
            assert_eq!(cmd.risk, ApprovalRisk::Destructive);
            assert_eq!(ext.kind, ApprovalKind::ExternalPath);
            assert_eq!(ext.risk, ApprovalRisk::ExternalAccess);
            assert!(ids.insert(cmd.id));
            assert!(ids.insert(ext.id));
        }
        assert_eq!(ids.len(), 64, "every request must have a unique id");
        // Reference the atomic import so the test compiles even if no
        // assertion needs it; signals the intent of "every id is distinct".
        let _ = AtomicUsize::new(0).load(Ordering::SeqCst);
    }
}
