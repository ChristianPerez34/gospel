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
    /// Whether command paths stay inside the active workspace.
    pub workspace_scoped: bool,
    /// Streaming run that owns this request. The frontend uses it to route
    /// the approval card to the correct session's live turn; `None` falls
    /// back to the focused session.
    pub run_id: Option<String>,
}

impl ApprovalRequest {
    pub fn command(
        tool_name: impl Into<String>,
        command_label: impl Into<String>,
        reason: impl Into<String>,
        destructive: bool,
        run_id: Option<&str>,
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
            workspace_scoped: false,
            tool_name,
            run_id: run_id.map(str::to_string),
        }
    }

    pub fn with_workspace_scope(mut self, workspace_scoped: bool) -> Self {
        self.workspace_scoped = workspace_scoped;
        self
    }

    pub fn external_path(
        tool_name: impl Into<String>,
        path: impl Into<String>,
        reason: impl Into<String>,
        run_id: Option<&str>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            kind: ApprovalKind::ExternalPath,
            title: "Allow external file access?".to_string(),
            summary: path.into(),
            reason: reason.into(),
            risk: ApprovalRisk::ExternalAccess,
            workspace_scoped: false,
            tool_name: tool_name.into(),
            run_id: run_id.map(str::to_string),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyMode {
    #[default]
    Ask,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Deny,
    AlwaysAllow,
    AlwaysDeny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalOutcome {
    Approved,
    Denied,
    TimedOut,
    AutoApproved,
    AutoDenied,
}

impl ApprovalOutcome {
    pub fn from_decision(decision: Option<ApprovalDecision>, timed_out: bool) -> Self {
        if timed_out {
            ApprovalOutcome::TimedOut
        } else {
            match decision {
                Some(ApprovalDecision::Approve) | Some(ApprovalDecision::AlwaysAllow) => {
                    ApprovalOutcome::Approved
                }
                Some(ApprovalDecision::Deny) | Some(ApprovalDecision::AlwaysDeny) => {
                    ApprovalOutcome::Denied
                }
                None => ApprovalOutcome::Denied,
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalPolicyPayload {
    pub mode: AutonomyMode,
    pub allowed_patterns: Vec<String>,
    pub reject_patterns: Vec<String>,
}

pub const DEFAULT_REJECT_PATTERNS: &[&str] = &[
    "rm *",
    "rm",
    "rmdir *",
    "rmdir",
    "git push *--force*",
    "git push *-f*",
    "git reset*",
    "git clean*",
    "git checkout *-f*",
    "git checkout *--force*",
    "git branch *-D*",
    "git branch *--delete*--force*",
    "sudo *",
    "su *",
    "env",
    "env *",
    "chmod *",
    "chown *",
    "mkfs*",
    "dd *",
    "fdisk*",
];

// Keep Auto to routine, workspace-local development operations. Unknown
// commands and remote operations stay interactive.
const AUTO_APPROVE_PATTERNS: &[&str] = &[
    "mkdir",
    "touch",
    "git add",
    "git commit",
    "cargo fmt",
    "cargo check",
    "cargo test",
    "cargo build",
    "cargo clippy",
    "bun run build",
    "bun run check",
    "bun run format",
    "bun run format:check",
    "bun run lint",
    "bun run test",
    "bun run typecheck",
];

fn normalize_command_for_policy(command: &str) -> String {
    let mut parts = command.split_whitespace();
    let Some(program) = parts.next() else {
        return String::new();
    };
    let executable = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let args: Vec<&str> = parts.collect();

    if executable.eq_ignore_ascii_case("git") {
        let args = crate::shell_tools::git_subcommand_args(&args);
        return if !args.is_empty() {
            format!("git {}", args.join(" "))
        } else {
            "git".to_string()
        };
    }

    if args.is_empty() {
        executable.to_string()
    } else {
        format!("{executable} {}", args.join(" "))
    }
}

pub fn canonical_command_pattern(command: &str) -> String {
    let normalized = normalize_command_for_policy(command);
    let parts: Vec<&str> = normalized.split_whitespace().collect();
    if parts.is_empty() {
        return normalized;
    }
    let multi_token_tools = [
        "git", "gh", "cargo", "bun", "npm", "pnpm", "yarn", "docker", "kubectl",
    ];
    if parts.len() >= 2 && multi_token_tools.contains(&parts[0].to_ascii_lowercase().as_str()) {
        format!("{} {}", parts[0], parts[1])
    } else {
        normalized
    }
}

pub fn matches_command_pattern(command: &str, pattern: &str) -> bool {
    let cmd = command.trim();
    let pat = pattern.trim();
    if pat.is_empty() || cmd.is_empty() {
        return false;
    }
    if cmd.eq_ignore_ascii_case(pat) {
        return true;
    }
    if pat.contains('*') {
        let segments: Vec<&str> = pat.split('*').collect();
        let mut remaining = cmd;
        let mut first = true;
        for segment in segments {
            if segment.is_empty() {
                continue;
            }
            if first && !pat.starts_with('*') {
                if !remaining
                    .to_ascii_lowercase()
                    .starts_with(&segment.to_ascii_lowercase())
                {
                    return false;
                }
                remaining = &remaining[segment.len()..];
            } else {
                match remaining
                    .to_ascii_lowercase()
                    .find(&segment.to_ascii_lowercase())
                {
                    Some(pos) => {
                        remaining = &remaining[pos + segment.len()..];
                    }
                    None => return false,
                }
            }
            first = false;
        }
        if !pat.ends_with('*') && !remaining.is_empty() {
            let last_segment = pat.rsplit('*').next().unwrap_or("");
            return cmd
                .to_ascii_lowercase()
                .ends_with(&last_segment.to_ascii_lowercase());
        }
        return true;
    }
    if cmd.len() > pat.len() {
        if let (Some(head), Some(tail)) = (cmd.get(..pat.len()), cmd.get(pat.len()..)) {
            if head.eq_ignore_ascii_case(pat) && tail.starts_with(char::is_whitespace) {
                return true;
            }
        }
    }
    false
}

pub fn matches_sensitive_target(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let sensitive = [
        ".env",
        "id_rsa",
        "id_ed25519",
        "id_ecdsa",
        "credentials",
        ".git/config",
        ".ssh/",
    ];
    for s in sensitive {
        if lower.contains(s) {
            return true;
        }
    }
    false
}

struct PendingEntry {
    sender: oneshot::Sender<ApprovalOutcome>,
    summary: String,
    kind: ApprovalKind,
    workspace_scoped: bool,
    risk: ApprovalRisk,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResolution {
    pub id: String,
    pub outcome: ApprovalOutcome,
    /// Echo of the originating request's run id so the frontend routes the
    /// status transition to the same turn that rendered the request card.
    pub run_id: Option<String>,
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
    pending: Mutex<HashMap<String, PendingEntry>>,
    emitter: Arc<dyn ApprovalEventEmitter>,
    timeout: ApprovalTimeout,
    autonomy_mode: Mutex<AutonomyMode>,
    allowed_patterns: Mutex<Vec<String>>,
    /// User-defined rejects only; built-in guardrails are immutable.
    reject_patterns: Mutex<Vec<String>>,
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
            autonomy_mode: Mutex::new(AutonomyMode::Ask),
            allowed_patterns: Mutex::new(Vec::new()),
            reject_patterns: Mutex::new(Vec::new()),
        }
    }

    pub fn is_command_rejected(&self, command: &str, risk: ApprovalRisk) -> bool {
        if matches!(risk, ApprovalRisk::Destructive) {
            return true;
        }
        let normalized = normalize_command_for_policy(command);
        if matches_sensitive_target(command) || matches_sensitive_target(&normalized) {
            return true;
        }
        for pattern in DEFAULT_REJECT_PATTERNS {
            if matches_command_pattern(command, pattern)
                || matches_command_pattern(&normalized, pattern)
            {
                return true;
            }
        }
        let reject = self
            .reject_patterns
            .lock()
            .expect("approval broker poisoned");
        for pat in reject.iter() {
            if matches_command_pattern(command, pat) || matches_command_pattern(&normalized, pat) {
                return true;
            }
        }
        false
    }

    pub fn matches_allow(&self, command: &str) -> bool {
        let normalized = normalize_command_for_policy(command);
        let allowed = self
            .allowed_patterns
            .lock()
            .expect("approval broker poisoned");
        for pat in allowed.iter() {
            if matches_command_pattern(command, pat) || matches_command_pattern(&normalized, pat) {
                return true;
            }
        }
        false
    }

    pub fn should_auto_approve(&self, request: &ApprovalRequest) -> bool {
        self.should_auto_approve_command(
            request.kind,
            request.workspace_scoped,
            request.risk,
            &request.summary,
        )
    }

    fn should_auto_approve_command(
        &self,
        kind: ApprovalKind,
        workspace_scoped: bool,
        risk: ApprovalRisk,
        command: &str,
    ) -> bool {
        if kind != ApprovalKind::Command
            || !workspace_scoped
            || matches!(risk, ApprovalRisk::Destructive)
        {
            return false;
        }
        if self.is_command_rejected(command, risk) {
            return false;
        }
        if self.matches_allow(command) {
            return true;
        }
        let mode = *self.autonomy_mode.lock().expect("approval broker poisoned");
        matches!(mode, AutonomyMode::Auto)
            && AUTO_APPROVE_PATTERNS.iter().any(|pattern| {
                matches_command_pattern(&normalize_command_for_policy(command), pattern)
            })
    }

    pub fn add_allow_rule_for_command(&self, command: &str) {
        let normalized = normalize_command_for_policy(command);
        let canonical = canonical_command_pattern(&normalized);
        let mut allowed = self
            .allowed_patterns
            .lock()
            .expect("approval broker poisoned");
        if !canonical.is_empty() && !allowed.contains(&canonical) {
            allowed.push(canonical.clone());
        }
        let exact = normalized;
        if !exact.is_empty() && exact != canonical && !allowed.contains(&exact) {
            allowed.push(exact);
        }
    }

    pub fn add_reject_rule_for_command(&self, command: &str) {
        self.add_reject_rule(canonical_command_pattern(command));
    }

    pub fn policy(&self) -> ApprovalPolicyPayload {
        let mode = *self.autonomy_mode.lock().expect("approval broker poisoned");
        let allowed_patterns = self
            .allowed_patterns
            .lock()
            .expect("approval broker poisoned")
            .clone();
        let reject = self
            .reject_patterns
            .lock()
            .expect("approval broker poisoned");
        let reject_patterns = DEFAULT_REJECT_PATTERNS
            .iter()
            .map(|pattern| pattern.to_string())
            .chain(reject.iter().cloned())
            .collect();
        ApprovalPolicyPayload {
            mode,
            allowed_patterns,
            reject_patterns,
        }
    }

    pub fn set_autonomy_mode(&self, mode: AutonomyMode) {
        {
            let mut guard = self.autonomy_mode.lock().expect("approval broker poisoned");
            *guard = mode;
        }

        if mode != AutonomyMode::Auto {
            return;
        }

        let auto_approved = {
            let mut pending = self.pending.lock().expect("approval broker poisoned");
            let ids: Vec<String> = pending
                .iter()
                .filter_map(|(id, entry)| {
                    self.should_auto_approve_command(
                        entry.kind,
                        entry.workspace_scoped,
                        entry.risk,
                        &entry.summary,
                    )
                    .then(|| id.clone())
                })
                .collect();
            ids.into_iter()
                .filter_map(|id| pending.remove(&id))
                .collect::<Vec<_>>()
        };
        for entry in auto_approved {
            let _ = entry.sender.send(ApprovalOutcome::AutoApproved);
        }
    }

    pub fn autonomy_mode(&self) -> AutonomyMode {
        *self.autonomy_mode.lock().expect("approval broker poisoned")
    }

    pub fn add_allow_rule(&self, pattern: impl Into<String>) {
        let pattern = pattern.into().trim().to_string();
        if pattern.is_empty() {
            return;
        }
        let mut allowed = self
            .allowed_patterns
            .lock()
            .expect("approval broker poisoned");
        if !allowed.contains(&pattern) {
            allowed.push(pattern);
        }
    }

    pub fn add_reject_rule(&self, pattern: impl Into<String>) {
        let pattern = pattern.into().trim().to_string();
        if pattern.is_empty() || DEFAULT_REJECT_PATTERNS.contains(&pattern.as_str()) {
            return;
        }
        let mut reject = self
            .reject_patterns
            .lock()
            .expect("approval broker poisoned");
        if !reject.contains(&pattern) {
            reject.push(pattern);
        }
    }

    pub fn remove_rule(&self, pattern: &str) -> bool {
        let mut removed = false;
        {
            let mut allowed = self
                .allowed_patterns
                .lock()
                .expect("approval broker poisoned");
            if let Some(pos) = allowed.iter().position(|p| p == pattern) {
                allowed.remove(pos);
                removed = true;
            }
        }
        {
            let mut reject = self
                .reject_patterns
                .lock()
                .expect("approval broker poisoned");
            if let Some(pos) = reject.iter().position(|p| p == pattern) {
                reject.remove(pos);
                removed = true;
            }
        }
        removed
    }

    pub fn clear_rules(&self) {
        let mut allowed = self
            .allowed_patterns
            .lock()
            .expect("approval broker poisoned");
        allowed.clear();
        let mut reject = self
            .reject_patterns
            .lock()
            .expect("approval broker poisoned");
        reject.clear();
    }

    /// Register a request, emit it to the frontend, and wait for a decision.
    ///
    /// If the request matches an allow rule or autonomy mode (and does not
    /// violate reject guardrails), it auto-approves immediately and emits
    /// an `approval-resolved` event with `AutoApproved`.
    ///
    /// Otherwise, waits for the frontend decision or timeout.
    pub async fn request(&self, request: ApprovalRequest) -> bool {
        let id = request.id.clone();
        let run_id = request.run_id.clone();

        if self.should_auto_approve(&request) {
            let outcome = ApprovalOutcome::AutoApproved;
            self.emitter.emit_requested(&request);
            self.emitter.emit_resolved(&ApprovalResolution {
                id,
                outcome,
                run_id,
            });
            return true;
        }

        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.pending.lock().expect("approval broker poisoned");
            pending.insert(
                id.clone(),
                PendingEntry {
                    sender,
                    summary: request.summary.clone(),
                    kind: request.kind,
                    workspace_scoped: request.workspace_scoped,
                    risk: request.risk,
                },
            );
        }

        self.emitter.emit_requested(&request);

        // Recheck after registering so a concurrent switch to Auto can't miss
        // a request that passed the initial Ask-mode check.
        if self.should_auto_approve(&request) {
            let entry = self
                .pending
                .lock()
                .expect("approval broker poisoned")
                .remove(&id);
            if let Some(entry) = entry {
                let _ = entry.sender.send(ApprovalOutcome::AutoApproved);
            }
        }

        let outcome = match tokio::time::timeout(self.timeout, receiver).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_dropped)) => ApprovalOutcome::Denied,
            Err(_elapsed) => ApprovalOutcome::TimedOut,
        };

        {
            let mut pending = self.pending.lock().expect("approval broker poisoned");
            pending.remove(&id);
        }

        self.emitter.emit_resolved(&ApprovalResolution {
            id,
            outcome,
            run_id,
        });

        matches!(
            outcome,
            ApprovalOutcome::Approved | ApprovalOutcome::AutoApproved
        )
    }

    /// Frontend-driven decision. Returns `true` if a pending request was
    /// found and notified. If the decision is `AlwaysAllow` or `AlwaysDeny`,
    /// automatically adds the rule into broker policy.
    pub fn resolve(&self, id: &str, decision: ApprovalDecision) -> bool {
        let entry = {
            let mut pending = self.pending.lock().expect("approval broker poisoned");
            pending.remove(id)
        };
        match entry {
            Some(entry) => {
                match decision {
                    ApprovalDecision::AlwaysAllow => {
                        if entry.kind == ApprovalKind::Command
                            && entry.workspace_scoped
                            && !matches!(entry.risk, ApprovalRisk::Destructive)
                            && !self.is_command_rejected(&entry.summary, entry.risk)
                        {
                            self.add_allow_rule_for_command(&entry.summary);
                        }
                    }
                    ApprovalDecision::AlwaysDeny if entry.kind == ApprovalKind::Command => {
                        self.add_reject_rule_for_command(&entry.summary);
                    }
                    _ => {}
                }
                entry
                    .sender
                    .send(ApprovalOutcome::from_decision(Some(decision), false))
                    .is_ok()
            }
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

    async fn wait_for_pending(broker: &ApprovalBroker) {
        // Poll until the spawned task has registered and emitted.
        for _ in 0..64 {
            if broker.pending_count() == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn request_emits_event_and_waits_for_decision() {
        let (broker, emitter) = test_broker();
        let request = ApprovalRequest::command(
            "run_shell_command",
            "git push origin main",
            "Pushes to remote.",
            false,
            None,
        );
        let id = request.id.clone();
        let broker_for_task = broker.clone();
        let task = tokio::spawn(async move { broker_for_task.request(request).await });

        wait_for_pending(&broker).await;
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
    async fn switching_to_auto_auto_approves_pending_safe_command() {
        let (broker, emitter) = test_broker_with_timeout(Duration::from_secs(1));
        let request = ApprovalRequest::command(
            "run_shell_command",
            "mkdir -p build",
            "Create build directory.",
            false,
            None,
        )
        .with_workspace_scope(true);
        let broker_for_task = broker.clone();
        let task = tokio::spawn(async move { broker_for_task.request(request).await });

        wait_for_pending(&broker).await;
        assert_eq!(broker.pending_count(), 1);

        broker.set_autonomy_mode(AutonomyMode::Auto);

        assert!(tokio::time::timeout(Duration::from_millis(100), task)
            .await
            .expect("pending safe command should be released when Auto is enabled")
            .unwrap());
        assert_eq!(broker.pending_count(), 0);
        assert_eq!(
            emitter.resolved.lock().unwrap()[0].outcome,
            ApprovalOutcome::AutoApproved
        );
    }

    #[tokio::test]
    async fn switching_to_auto_keeps_unlisted_pending_command_interactive() {
        let (broker, _) = test_broker_with_timeout(Duration::from_secs(1));
        let request = ApprovalRequest::command(
            "run_git_command",
            "git push origin main",
            "Push changes to the remote.",
            false,
            None,
        )
        .with_workspace_scope(true);
        let id = request.id.clone();
        let broker_for_task = broker.clone();
        let mut task = tokio::spawn(async move { broker_for_task.request(request).await });

        wait_for_pending(&broker).await;
        broker.set_autonomy_mode(AutonomyMode::Auto);

        assert!(tokio::time::timeout(Duration::from_millis(50), &mut task)
            .await
            .is_err());
        assert_eq!(broker.pending_count(), 1);
        assert!(broker.resolve(&id, ApprovalDecision::Approve));
        assert!(task.await.unwrap());
    }

    #[tokio::test]
    async fn resolution_echoes_request_run_id() {
        let (broker, emitter) = test_broker();
        let request = ApprovalRequest::command(
            "run_shell_command",
            "cargo publish",
            "Publishes the crate.",
            false,
            Some("r-1"),
        );
        let id = request.id.clone();
        let broker_for_task = broker.clone();
        let task = tokio::spawn(async move { broker_for_task.request(request).await });

        wait_for_pending(&broker).await;
        assert_eq!(broker.pending_count(), 1);
        assert_eq!(
            emitter.requested.lock().unwrap()[0].run_id.as_deref(),
            Some("r-1"),
            "the emitted request must carry the run id"
        );

        assert!(broker.resolve(&id, ApprovalDecision::Approve));
        assert!(task.await.unwrap());
        let resolved = emitter.resolved.lock().unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, id);
        assert_eq!(
            resolved[0].run_id.as_deref(),
            Some("r-1"),
            "the resolution must echo the request's run id"
        );
    }

    #[tokio::test]
    async fn deny_decision_returns_false_and_resolves_as_denied() {
        let (broker, emitter) = test_broker();
        let request =
            ApprovalRequest::external_path("read_file", "/etc/passwd", "Outside workspace.", None);
        let id = request.id.clone();
        let broker_for_task = broker.clone();

        let task = tokio::spawn(async move { broker_for_task.request(request).await });
        wait_for_pending(&broker).await;

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
        let request = ApprovalRequest::command(
            "run_shell_command",
            "rm -rf build",
            "Removes files.",
            true,
            None,
        );
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
        let request =
            ApprovalRequest::external_path("read_file", "/tmp/x", "Outside workspace.", None);
        let id = request.id.clone();
        let broker_for_task = broker.clone();

        let task = tokio::spawn(async move { broker_for_task.request(request).await });
        wait_for_pending(&broker).await;

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
            let cmd = ApprovalRequest::command("run_shell_command", "ls", "x", true, None);
            let ext = ApprovalRequest::external_path("read_file", "/tmp/x", "y", None);
            assert_eq!(cmd.kind, ApprovalKind::Command);
            assert_eq!(cmd.risk, ApprovalRisk::Destructive);
            assert!(!cmd.workspace_scoped);
            assert_eq!(ext.kind, ApprovalKind::ExternalPath);
            assert_eq!(ext.risk, ApprovalRisk::ExternalAccess);
            assert!(!ext.workspace_scoped);
            assert!(ids.insert(cmd.id));
            assert!(ids.insert(ext.id));
        }
        assert_eq!(ids.len(), 64, "every request must have a unique id");
        // Reference the atomic import so the test compiles even if no
        // assertion needs it; signals the intent of "every id is distinct".
        let _ = AtomicUsize::new(0).load(Ordering::SeqCst);
    }

    #[tokio::test]
    async fn always_allow_decision_adds_rule_and_auto_approves_subsequent_commands() {
        let (broker, emitter) = test_broker();
        let request1 = ApprovalRequest::command(
            "run_shell_command",
            "cargo test --lib",
            "Run tests.",
            false,
            None,
        )
        .with_workspace_scope(true);
        let id1 = request1.id.clone();
        let broker_for_task = broker.clone();
        let task1 = tokio::spawn(async move { broker_for_task.request(request1).await });

        wait_for_pending(&broker).await;
        assert_eq!(broker.pending_count(), 1);

        assert!(broker.resolve(&id1, ApprovalDecision::AlwaysAllow));
        assert!(task1.await.unwrap());

        // Policy should now have allowed rules for "cargo test" and "cargo test --lib"
        let policy = broker.policy();
        assert!(policy.allowed_patterns.contains(&"cargo test".to_string()));

        // Subsequent matching command should auto-approve without waiting in pending!
        let request2 = ApprovalRequest::command(
            "run_shell_command",
            "cargo test --bin my_bin",
            "Run bin tests.",
            false,
            None,
        )
        .with_workspace_scope(true);
        let approved2 = broker.request(request2).await;
        assert!(approved2);
        assert_eq!(broker.pending_count(), 0);

        let resolved = emitter.resolved.lock().unwrap();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[1].outcome, ApprovalOutcome::AutoApproved);
    }

    #[tokio::test]
    async fn always_allow_does_not_auto_approve_destructive_commands() {
        let (broker, _emitter) = test_broker();
        broker.add_allow_rule("git push");

        // Even though "git push" is in allowed_patterns, destructive is NOT auto-approved!
        let request = ApprovalRequest::command(
            "run_git_command",
            "git push origin main",
            "Destructive push.",
            true, // destructive!
            None,
        )
        .with_workspace_scope(true);
        let broker_for_task = broker.clone();
        let task = tokio::spawn(async move { broker_for_task.request(request).await });

        wait_for_pending(&broker).await;
        assert_eq!(broker.pending_count(), 1);
        let id = {
            let pending = broker.pending.lock().unwrap();
            pending.keys().next().unwrap().clone()
        };
        assert!(broker.resolve(&id, ApprovalDecision::Approve));
        assert!(task.await.unwrap());
    }

    #[tokio::test]
    async fn always_allow_does_not_create_rules_for_external_paths() {
        let (broker, _) = test_broker();
        let request = ApprovalRequest::external_path(
            "read_file",
            "/tmp/outside/My Notes.txt",
            "Outside workspace.",
            None,
        );
        let id = request.id.clone();
        let broker_for_task = broker.clone();
        let task = tokio::spawn(async move { broker_for_task.request(request).await });

        wait_for_pending(&broker).await;
        assert!(broker.resolve(&id, ApprovalDecision::AlwaysAllow));
        assert!(task.await.unwrap());
        assert!(broker.policy().allowed_patterns.is_empty());

        broker.add_allow_rule("/tmp/outside/My");
        let sibling = ApprovalRequest::external_path(
            "read_file",
            "/tmp/outside/My Secrets.txt",
            "Outside workspace.",
            None,
        );
        assert!(!broker.should_auto_approve(&sibling));
    }

    #[tokio::test]
    async fn workspace_escaping_commands_never_auto_approve_or_save_allow_rules() {
        let (broker, _) = test_broker();
        broker.set_autonomy_mode(AutonomyMode::Auto);
        let request = ApprovalRequest::command(
            "run_shell_command",
            "mkdir /tmp/outside",
            "Create outside workspace.",
            false,
            None,
        )
        .with_workspace_scope(false);

        assert!(!broker.should_auto_approve(&request));
        let id = request.id.clone();
        let broker_for_task = broker.clone();
        let task = tokio::spawn(async move { broker_for_task.request(request).await });

        wait_for_pending(&broker).await;
        assert!(broker.resolve(&id, ApprovalDecision::AlwaysAllow));
        assert!(task.await.unwrap());
        assert!(broker.policy().allowed_patterns.is_empty());

        let repeated = ApprovalRequest::command(
            "run_shell_command",
            "mkdir /tmp/outside",
            "Create outside workspace.",
            false,
            None,
        )
        .with_workspace_scope(false);
        broker.add_allow_rule_for_command(&repeated.summary);
        assert!(!broker.should_auto_approve(&repeated));
    }

    #[tokio::test]
    async fn reject_patterns_guardrail_blocks_auto_approval() {
        let (broker, _emitter) = test_broker();
        broker.set_autonomy_mode(AutonomyMode::Auto);

        // A safe mutating command auto-approves in Auto mode
        let safe_cmd = ApprovalRequest::command(
            "run_shell_command",
            "mkdir -p build",
            "Create dir.",
            false,
            None,
        )
        .with_workspace_scope(true);
        assert!(broker.request(safe_cmd).await);
        assert_eq!(broker.pending_count(), 0);

        // A command matching a reject pattern (rm *) is blocked from auto-approval
        let rm_cmd = ApprovalRequest::command(
            "run_shell_command",
            "rm -rf build",
            "Delete dir.",
            false,
            None,
        )
        .with_workspace_scope(true);
        let broker_for_task = broker.clone();
        let task = tokio::spawn(async move { broker_for_task.request(rm_cmd).await });

        wait_for_pending(&broker).await;
        assert_eq!(broker.pending_count(), 1);
        let id = {
            let pending = broker.pending.lock().unwrap();
            pending.keys().next().unwrap().clone()
        };
        assert!(broker.resolve(&id, ApprovalDecision::Deny));
        assert!(!task.await.unwrap());
    }

    #[test]
    fn auto_mode_approves_routine_workspace_commands_only() {
        let (broker, _) = test_broker();
        broker.set_autonomy_mode(AutonomyMode::Auto);
        let request = |summary: &str| {
            ApprovalRequest::command("run_shell_command", summary, "Test command.", false, None)
                .with_workspace_scope(true)
        };

        assert!(broker.should_auto_approve(&request("mkdir -p build")));
        assert!(broker.should_auto_approve(&request("git add src/main.rs")));
        assert!(broker.should_auto_approve(&request("git commit -m update")));
        assert!(broker.should_auto_approve(&request("cargo test --lib")));
        assert!(broker.should_auto_approve(&request("bun run test")));
        assert!(broker.should_auto_approve(&request("bun run build")));
        assert!(!broker.should_auto_approve(&request("git push origin dev")));
        assert!(!broker.should_auto_approve(&request("bun install")));
        assert!(!broker.should_auto_approve(&request(
            "python3 -c \"__import__('pathlib').Path('important.txt').unlink()\""
        )));
        assert!(!broker.should_auto_approve(&request("env rm -rf build")));
        assert!(!broker.should_auto_approve(&request("git -C . reset --hard")));
        assert!(broker.is_command_rejected("env", ApprovalRisk::Mutating));
        assert!(broker.is_command_rejected("/usr/bin/rm -rf build", ApprovalRisk::Mutating));
    }

    #[test]
    fn always_allow_for_unknown_executables_keeps_the_full_command() {
        let (broker, _) = test_broker();
        let harmless = "python3 -c \"print(1)\"";
        let destructive = "python3 -c \"__import__('pathlib').Path('important.txt').unlink()\"";

        assert_eq!(canonical_command_pattern(harmless), harmless);
        broker.add_allow_rule_for_command(harmless);
        assert!(broker.matches_allow(harmless));
        assert!(!broker.matches_allow(destructive));
    }

    #[tokio::test]
    async fn sensitive_targets_are_rejected_from_auto_approval() {
        let (broker, _emitter) = test_broker();
        broker.set_autonomy_mode(AutonomyMode::Auto);

        let env_cmd = ApprovalRequest::command(
            "run_shell_command",
            "cat .env.local",
            "Read env file.",
            false,
            None,
        )
        .with_workspace_scope(true);
        let broker_for_task = broker.clone();
        let task = tokio::spawn(async move { broker_for_task.request(env_cmd).await });

        wait_for_pending(&broker).await;
        assert_eq!(broker.pending_count(), 1);
        let id = {
            let pending = broker.pending.lock().unwrap();
            pending.keys().next().unwrap().clone()
        };
        assert!(broker.resolve(&id, ApprovalDecision::Deny));
        assert!(!task.await.unwrap());
    }

    #[test]
    fn pattern_matching_and_canonicalization() {
        assert_eq!(canonical_command_pattern("git add src/foo.rs"), "git add");
        assert_eq!(canonical_command_pattern("cargo test --lib"), "cargo test");
        assert_eq!(canonical_command_pattern("bun run test"), "bun run");
        assert_eq!(canonical_command_pattern("mkdir -p test"), "mkdir -p test");

        assert!(matches_command_pattern("cargo test --lib", "cargo test"));
        assert!(matches_command_pattern("cargo test", "cargo test"));
        assert!(!matches_command_pattern("cargo testing", "cargo test"));
        assert!(!matches_command_pattern("😀", "a"));
        assert_eq!(
            normalize_command_for_policy("git -C . reset --hard"),
            "git reset --hard"
        );

        assert!(matches_command_pattern("rm -rf foo", "rm *"));
        assert!(matches_command_pattern("git reset --hard", "git reset*"));
        assert!(matches_command_pattern(
            "git push origin main --force",
            "git push *--force*"
        ));
        assert!(!matches_command_pattern(
            "git push origin main",
            "git push *--force*"
        ));
    }

    #[test]
    fn policy_management_rules() {
        let (broker, _) = test_broker();
        assert_eq!(broker.autonomy_mode(), AutonomyMode::Ask);

        broker.set_autonomy_mode(AutonomyMode::Auto);
        assert_eq!(broker.autonomy_mode(), AutonomyMode::Auto);

        broker.add_allow_rule("bun test");
        let policy = broker.policy();
        assert!(policy.allowed_patterns.contains(&"bun test".to_string()));

        assert!(!broker.remove_rule("rm *"));
        assert!(broker
            .policy()
            .reject_patterns
            .contains(&"rm *".to_string()));

        assert!(broker.remove_rule("bun test"));
        assert!(!broker
            .policy()
            .allowed_patterns
            .contains(&"bun test".to_string()));

        broker.add_allow_rule("cargo check");
        broker.clear_rules();
        assert!(broker.policy().allowed_patterns.is_empty());
        // Default rejects are preserved
        assert!(broker
            .policy()
            .reject_patterns
            .contains(&"rm *".to_string()));
    }
}
