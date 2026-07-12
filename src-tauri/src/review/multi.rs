use super::{
    run_review, MultiFocusStatus, ReviewConfig, ReviewFocus, ReviewPhase, ReviewProgressEmitter,
    ReviewProgressEvent, ReviewResult,
};
use futures::FutureExt;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use uuid::Uuid;

pub const ALL_FOCUSES: &[ReviewFocus] = &[
    ReviewFocus::Security,
    ReviewFocus::BugHunt,
    ReviewFocus::Architecture,
    ReviewFocus::Performance,
    ReviewFocus::Style,
];

const MULTI_REVIEW_TIMEOUT: Duration = Duration::from_secs(300);

/// Wraps a child `run_review` emitter and forwards its non-terminal progress
/// events to the parent multi-focus emitter. The aggregate `run_id` and the
/// focus are stamped on every event so the frontend can key them to the right
/// per-focus pipeline. Child `Done`/`Failed` events are suppressed because
/// `run_multi_focus_review` emits the terminal `MultiFocus` phases.
struct FocusEmitter {
    parent: Arc<dyn ReviewProgressEmitter>,
    run_id: String,
    focus: ReviewFocus,
}

impl ReviewProgressEmitter for FocusEmitter {
    fn emit_progress(&self, event: ReviewProgressEvent) {
        if matches!(event.phase, ReviewPhase::Done { .. } | ReviewPhase::Failed { .. }) {
            return;
        }
        let mut event = event;
        event.run_id = self.run_id.clone();
        event.focus = Some(self.focus);
        self.parent.emit_progress(event);
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MultiReviewResult {
    pub results: Vec<ReviewResult>,
    pub errors: BTreeMap<String, String>,
    pub summary: String,
    pub files_scanned: usize,
    pub total_findings: usize,
    pub total_suppressed: usize,
}

/// Per-focus child review future, injectable so tests can drive the runner
/// through success, error, timeout, and panic paths without invoking
/// `run_review` (which would make real LLM calls).
///
/// The public `run_multi_focus_review` always passes `run_review` here. Tests
/// pass a stub closure that returns synthetic results.
type ChildReviewFn = Arc<
    dyn Fn(
            ReviewConfig,
            PathBuf,
            String,
            Arc<dyn ReviewProgressEmitter>,
        ) -> Pin<Box<dyn Future<Output = Result<ReviewResult, String>> + Send>>
        + Send
        + Sync,
>;

pub async fn run_multi_focus_review(
    provider: String,
    model: String,
    mode: String,
    pr_number: Option<u64>,
    focuses: &[ReviewFocus],
    workspace_path: PathBuf,
    api_key: String,
    emitter: Arc<dyn ReviewProgressEmitter>,
) -> Result<MultiReviewResult, String> {
    let child: ChildReviewFn = Arc::new(|config, path, key, emitter| {
        Box::pin(run_review(config, path, key, emitter))
    });
    run_multi_focus_review_with_child(
        provider,
        model,
        mode,
        pr_number,
        focuses,
        workspace_path,
        api_key,
        emitter,
        MULTI_REVIEW_TIMEOUT,
        child,
    )
    .await
}

pub(crate) async fn run_multi_focus_review_with_child(
    provider: String,
    model: String,
    mode: String,
    pr_number: Option<u64>,
    focuses: &[ReviewFocus],
    workspace_path: PathBuf,
    api_key: String,
    emitter: Arc<dyn ReviewProgressEmitter>,
    timeout_duration: Duration,
    child: ChildReviewFn,
) -> Result<MultiReviewResult, String> {
    if focuses.is_empty() {
        return Err("At least one review focus is required.".to_string());
    }

    let run_id = Uuid::new_v4().to_string();
    let unique_focuses: BTreeSet<ReviewFocus> = focuses.iter().copied().collect();
    let total = unique_focuses.len();
    let mut pending: BTreeSet<ReviewFocus> = unique_focuses.clone();
    let emitter_ref: &dyn ReviewProgressEmitter = &*emitter;

    emitter_ref.emit_progress(ReviewProgressEvent::new(
        &run_id,
        ReviewPhase::MultiFocusStart { total },
    ));

    let mut join_set = JoinSet::new();

    for &focus in &unique_focuses {
        let config = ReviewConfig {
            provider: provider.clone(),
            model: model.clone(),
            mode: mode.clone(),
            focus,
            pr_number,
        };
        let workspace_path = workspace_path.clone();
        let api_key = api_key.clone();
        let child = Arc::clone(&child);
        let focus_emitter: Arc<dyn ReviewProgressEmitter> = Arc::new(FocusEmitter {
            parent: Arc::clone(&emitter),
            run_id: run_id.clone(),
            focus,
        });
        emitter_ref.emit_progress(ReviewProgressEvent::new_with_focus(
            &run_id,
            focus,
            ReviewPhase::MultiFocus {
                focus,
                completed: 0,
                total,
                findings: 0,
                suppressed: 0,
                status: MultiFocusStatus::Running,
            },
        ));
        join_set.spawn(async move {
            // Catch panics so the focus identity survives a child crash. Without
            // this, a panicking child surfaces as a join error with no focus
            // and the real focus is left showing as "running" forever.
            let outcome = std::panic::AssertUnwindSafe(child(
                config,
                workspace_path,
                api_key,
                Arc::clone(&focus_emitter),
            ))
            .catch_unwind()
            .await;
            let result = match outcome {
                Ok(Ok(review)) => Ok(review),
                Ok(Err(error)) => Err(error),
                Err(payload) => Err(format_panic(&payload)),
            };
            (focus, result)
        });
    }

    let mut results = Vec::new();
    let mut errors = BTreeMap::new();
    let deadline = tokio::time::Instant::now() + timeout_duration;
    let mut completed = 0usize;
    let mut total_findings = 0usize;
    let mut total_suppressed = 0usize;

    loop {
        match tokio::time::timeout_at(deadline, join_set.join_next()).await {
            Ok(Some(Ok((focus, Ok(result))))) => {
                pending.remove(&focus);
                completed += 1;
                total_findings += result.comments.len();
                total_suppressed += result.suppressed_count;
                emitter_ref.emit_progress(ReviewProgressEvent::new_with_focus(
                    &run_id,
                    focus,
                    ReviewPhase::MultiFocus {
                        focus,
                        completed,
                        total,
                        findings: result.comments.len(),
                        suppressed: result.suppressed_count,
                        status: MultiFocusStatus::Done,
                    },
                ));
                results.push(result);
            }
            Ok(Some(Ok((focus, Err(error))))) => {
                pending.remove(&focus);
                completed += 1;
                errors.insert(focus.to_string(), error.clone());
                emitter_ref.emit_progress(ReviewProgressEvent::new_with_focus(
                    &run_id,
                    focus,
                    ReviewPhase::MultiFocus {
                        focus,
                        completed,
                        total,
                        findings: 0,
                        suppressed: 0,
                        status: MultiFocusStatus::Failed { detail: error },
                    },
                ));
            }
            Ok(Some(Err(join_error))) => {
                // A task was aborted or panicked in a way `catch_unwind` could
                // not catch. The focus is not recoverable here; break out and
                // let the timeout/cleanup path report any still-pending focus
                // explicitly. Surface this unexpected error to the caller.
                if !join_error.is_cancelled() {
                    errors.insert(
                        format!("join-error-{}", errors.len()),
                        join_error.to_string(),
                    );
                }
                break;
            }
            Ok(None) => break,
            Err(_elapsed) => break,
        }
    }

    // Any focus still in flight is timed out (or about to be aborted). Emit a
    // per-focus failed event for each so the UI leaves "running" state and the
    // real focus name is recorded in the errors map.
    if !join_set.is_empty() {
        let mut still_pending: Vec<ReviewFocus> = pending.iter().copied().collect();
        still_pending.sort_by_key(|focus| focus_order(*focus));
        for focus in still_pending {
            let detail = "Multi-focus review timed out.".to_string();
            errors.insert(focus.to_string(), detail.clone());
            emitter_ref.emit_progress(ReviewProgressEvent::new_with_focus(
                &run_id,
                focus,
                ReviewPhase::MultiFocus {
                    focus,
                    completed,
                    total,
                    findings: 0,
                    suppressed: 0,
                    status: MultiFocusStatus::Failed { detail },
                },
            ));
        }
        pending.clear();
        join_set.abort_all();
        // Drain aborted tasks so the runtime can clean them up. Don't emit
        // additional events for these — the pending loop above already
        // surfaced them under their real focus name.
        while join_set.join_next().await.is_some() {}
    }

    results.sort_by_key(|result| focus_order(result.focus));

    if results.is_empty() {
        let detail = format_all_failed(&errors);
        emitter_ref.emit_progress(ReviewProgressEvent::new(
            &run_id,
            ReviewPhase::Failed {
                detail: detail.clone(),
            },
        ));
        return Err(detail);
    }

    let files_scanned = results
        .iter()
        .map(|result| result.files_scanned)
        .max()
        .unwrap_or(0);
    let summary = build_multi_summary(&results, &errors);

    emitter_ref.emit_progress(ReviewProgressEvent::new(
        &run_id,
        ReviewPhase::Done {
            findings: total_findings,
            suppressed: total_suppressed,
        },
    ));

    Ok(MultiReviewResult {
        results,
        errors,
        summary,
        files_scanned,
        total_findings,
        total_suppressed,
    })
}

fn format_panic(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        format!("child review panicked: {message}")
    } else if let Some(message) = payload.downcast_ref::<String>() {
        format!("child review panicked: {message}")
    } else {
        "child review panicked".to_string()
    }
}

fn focus_order(focus: ReviewFocus) -> usize {
    ALL_FOCUSES
        .iter()
        .position(|candidate| *candidate == focus)
        .unwrap_or(ALL_FOCUSES.len())
}

fn build_multi_summary(results: &[ReviewResult], errors: &BTreeMap<String, String>) -> String {
    let mut parts = results
        .iter()
        .map(|result| {
            format!(
                "{}: {} finding{}",
                result.focus,
                result.comments.len(),
                if result.comments.len() == 1 { "" } else { "s" }
            )
        })
        .collect::<Vec<_>>();

    parts.extend(errors.keys().map(|focus| format!("{focus}: failed")));
    parts.join(" · ")
}

fn format_all_failed(errors: &BTreeMap<String, String>) -> String {
    if errors.is_empty() {
        return "All review focuses failed.".to_string();
    }

    format!(
        "All review focuses failed:\n{}",
        errors
            .iter()
            .map(|(focus, error)| format!("  {focus}: {error}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{
        MultiFocusStatus, NoopReviewProgressEmitter, PhaseStatus, ReviewMode, ReviewPhase,
        ReviewProgressEvent, Severity, SignalTier,
    };

    fn result(focus: ReviewFocus, comments: usize, suppressed_count: usize) -> ReviewResult {
        ReviewResult {
            run_id: format!("run-{focus}"),
            focus,
            comments: (0..comments)
                .map(|index| crate::review::ReviewComment {
                    comment_id: format!("{focus}-{index}"),
                    file: "src/main.rs".to_string(),
                    line_start: 1,
                    line_end: 1,
                    severity: Severity::Info,
                    category: "test".to_string(),
                    focus,
                    focus_subcategory: None,
                    cwe_id: None,
                    cwe_name: None,
                    title: "Test finding".to_string(),
                    description: "Test description".to_string(),
                    rationale: Some("Test rationale".to_string()),
                    evidence: "Test evidence".to_string(),
                    suggestion: Some("Test suggestion".to_string()),
                    verification_plan: Some("Test verification".to_string()),
                    signal_tier: SignalTier::Tier2,
                })
                .collect(),
            summary: "summary".to_string(),
            validated: true,
            warnings: Vec::new(),
            files_scanned: 2,
            mode: ReviewMode::Local,
            suppressed_count,
            snr_percent: 100.0,
            user_visible: true,
        }
    }

    /// What a child stub should do for a particular focus.
    #[derive(Clone)]
    enum StubAction {
        Ok(ReviewResult),
        Err(String),
        Sleep(Duration),
        Panic(&'static str),
    }

    fn make_stub(plan: BTreeMap<ReviewFocus, StubAction>) -> ChildReviewFn {
        Arc::new(move |config, _path, _key, _emitter| {
            let focus = config.focus;
            let action = plan
                .get(&focus)
                .cloned()
                .unwrap_or_else(|| StubAction::Err("no plan for focus".to_string()));
            Box::pin(async move {
                match action {
                    StubAction::Ok(review) => Ok(review),
                    StubAction::Err(message) => Err(message),
                    StubAction::Sleep(duration) => {
                        tokio::time::sleep(duration).await;
                        Err("cancelled by test".to_string())
                    }
                    StubAction::Panic(message) => panic!("{message}"),
                }
            })
        })
    }

    /// Capturing emitter that records every event in order so tests can assert
    /// the real emission sequence without touching Tauri. The `events` buffer is
    /// `Arc` so a concrete and a trait-object handle can share the same buffer.
    #[derive(Default)]
    struct CapturingEmitter {
        events: Arc<std::sync::Mutex<Vec<ReviewProgressEvent>>>,
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

        fn run_ids(&self) -> Vec<String> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .map(|event| event.run_id.clone())
                .collect()
        }
    }

    fn capturing_emitter_pair() -> (Arc<CapturingEmitter>, Arc<dyn ReviewProgressEmitter>) {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let concrete = Arc::new(CapturingEmitter {
            events: Arc::clone(&events),
        });
        let dynamic: Arc<dyn ReviewProgressEmitter> = Arc::new(CapturingEmitter { events });
        (concrete, dynamic)
    }

    #[test]
    fn multi_review_summary_includes_successes_and_failures() {
        let mut errors = BTreeMap::new();
        errors.insert("Performance".to_string(), "provider failed".to_string());

        let summary = build_multi_summary(
            &[
                result(ReviewFocus::Security, 1, 0),
                result(ReviewFocus::BugHunt, 2, 1),
            ],
            &errors,
        );

        assert_eq!(
            summary,
            "Security: 1 finding · BugHunt: 2 findings · Performance: failed"
        );
    }

    #[test]
    fn all_failed_error_lists_each_focus() {
        let mut errors = BTreeMap::new();
        errors.insert("Security".to_string(), "bad key".to_string());
        errors.insert("Style".to_string(), "timeout".to_string());

        let message = format_all_failed(&errors);

        assert!(message.contains("All review focuses failed"));
        assert!(message.contains("Security: bad key"));
        assert!(message.contains("Style: timeout"));
    }

    #[test]
    fn multi_focus_progress_event_serializes_for_frontend() {
        let event = ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::MultiFocus {
                focus: ReviewFocus::Security,
                completed: 1,
                total: 3,
                findings: 4,
                suppressed: 1,
                status: MultiFocusStatus::Done,
            },
        );
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["run_id"], "multi-run-1");
        assert_eq!(json["phase"]["type"], "multiFocus");
        assert_eq!(json["phase"]["focus"], "Security");
        assert_eq!(json["phase"]["completed"], 1);
        assert_eq!(json["phase"]["total"], 3);
        assert_eq!(json["phase"]["findings"], 4);
        assert_eq!(json["phase"]["suppressed"], 1);
        assert_eq!(json["phase"]["status"], "done");
    }

    #[test]
    fn multi_focus_start_event_serializes_with_total_only() {
        let event =
            ReviewProgressEvent::new("multi-run-1", ReviewPhase::MultiFocusStart { total: 5 });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["phase"]["type"], "multiFocusStart");
        assert_eq!(json["phase"]["total"], 5);
        assert!(json["phase"].get("focus").is_none());
        assert!(json["phase"].get("status").is_none());
    }

    #[test]
    fn multi_focus_failed_event_serializes_nested_detail() {
        let event = ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::MultiFocus {
                focus: ReviewFocus::Performance,
                completed: 2,
                total: 3,
                findings: 0,
                suppressed: 0,
                status: MultiFocusStatus::Failed {
                    detail: "provider error".to_string(),
                },
            },
        );
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(
            json["phase"]["status"]["failed"]["detail"],
            "provider error"
        );
    }

    #[test]
    fn multi_focus_events_share_one_run_id() {
        let emitter = CapturingEmitter::default();

        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-abc",
            ReviewPhase::MultiFocusStart { total: 2 },
        ));
        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-abc",
            ReviewPhase::MultiFocus {
                focus: ReviewFocus::Security,
                completed: 1,
                total: 2,
                findings: 3,
                suppressed: 0,
                status: MultiFocusStatus::Done,
            },
        ));
        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-abc",
            ReviewPhase::Done {
                findings: 3,
                suppressed: 0,
            },
        ));

        let phases = emitter.phases();
        assert_eq!(phases.len(), 3);
        for event in emitter.events.lock().unwrap().iter() {
            assert_eq!(event.run_id, "multi-run-abc");
        }
        assert!(matches!(
            phases[0],
            ReviewPhase::MultiFocusStart { total: 2 }
        ));
        assert!(matches!(
            phases[1],
            ReviewPhase::MultiFocus {
                status: MultiFocusStatus::Done,
                ..
            }
        ));
        assert!(matches!(phases[2], ReviewPhase::Done { .. }));
    }

    #[test]
    fn multi_focus_start_event_emitted_before_done_events() {
        let emitter = CapturingEmitter::default();

        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::MultiFocusStart { total: 3 },
        ));
        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::MultiFocus {
                focus: ReviewFocus::Security,
                completed: 1,
                total: 3,
                findings: 0,
                suppressed: 0,
                status: MultiFocusStatus::Running,
            },
        ));
        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::Done {
                findings: 5,
                suppressed: 1,
            },
        ));

        let phases = emitter.phases();
        assert_eq!(phases.len(), 3);
        assert!(matches!(
            phases[0],
            ReviewPhase::MultiFocusStart { total: 3 }
        ));
    }

    #[test]
    fn partial_child_failures_emit_failed_focus_before_done() {
        let emitter = CapturingEmitter::default();

        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::MultiFocusStart { total: 2 },
        ));
        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::MultiFocus {
                focus: ReviewFocus::Security,
                completed: 1,
                total: 2,
                findings: 3,
                suppressed: 0,
                status: MultiFocusStatus::Done,
            },
        ));
        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::MultiFocus {
                focus: ReviewFocus::Performance,
                completed: 2,
                total: 2,
                findings: 0,
                suppressed: 0,
                status: MultiFocusStatus::Failed {
                    detail: "API key missing".to_string(),
                },
            },
        ));
        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::Done {
                findings: 3,
                suppressed: 0,
            },
        ));

        let phases = emitter.phases();
        assert_eq!(phases.len(), 4);
        assert!(matches!(
            phases[0],
            ReviewPhase::MultiFocusStart { total: 2 }
        ));
        assert!(matches!(phases[3], ReviewPhase::Done { .. }));
    }

    #[test]
    fn noop_emitter_handles_multi_focus_phases() {
        let noop = NoopReviewProgressEmitter;
        noop.emit_progress(ReviewProgressEvent::new(
            "run-1",
            ReviewPhase::MultiFocusStart { total: 5 },
        ));
        noop.emit_progress(ReviewProgressEvent::new(
            "run-1",
            ReviewPhase::MultiFocus {
                focus: ReviewFocus::Security,
                completed: 1,
                total: 5,
                findings: 0,
                suppressed: 0,
                status: MultiFocusStatus::Running,
            },
        ));
        noop.emit_progress(ReviewProgressEvent::new(
            "run-1",
            ReviewPhase::MultiFocus {
                focus: ReviewFocus::Performance,
                completed: 2,
                total: 5,
                findings: 0,
                suppressed: 0,
                status: MultiFocusStatus::Failed {
                    detail: "provider error".to_string(),
                },
            },
        ));
    }

    // ── Runner-level tests driving the real `run_multi_focus_review` loop ──

    fn plan_from_list(focuses: &[(ReviewFocus, StubAction)]) -> BTreeMap<ReviewFocus, StubAction> {
        focuses.iter().cloned().collect()
    }

    fn focuses_vec(values: &[ReviewFocus]) -> Vec<ReviewFocus> {
        values.to_vec()
    }

    #[tokio::test]
    async fn runner_emits_done_for_every_successful_focus_and_terminates() {
        let plan = plan_from_list(&[
            (
                ReviewFocus::Security,
                StubAction::Ok(result(ReviewFocus::Security, 2, 0)),
            ),
            (
                ReviewFocus::BugHunt,
                StubAction::Ok(result(ReviewFocus::BugHunt, 1, 1)),
            ),
        ]);
        let (emitter, emitter_dyn) = capturing_emitter_pair();

        let outcome = run_multi_focus_review_with_child(
            "openai".to_string(),
            "gpt-test".to_string(),
            "local".to_string(),
            None,
            &focuses_vec(&[ReviewFocus::Security, ReviewFocus::BugHunt]),
            PathBuf::from("/tmp"),
            "key".to_string(),
            emitter_dyn,
            Duration::from_secs(5),
            make_stub(plan),
        )
        .await
        .expect("run should succeed");

        assert_eq!(outcome.results.len(), 2);
        assert!(outcome.errors.is_empty());
        assert_eq!(outcome.total_findings, 3);
        assert_eq!(outcome.total_suppressed, 1);

        let phases = emitter.phases();
        // 1 MultiFocusStart + 2 running + 2 done + 1 done = 6 events.
        assert_eq!(phases.len(), 6);
        assert!(
            matches!(phases[0], ReviewPhase::MultiFocusStart { total: 2 }),
            "first event is the aggregate multi-focus start handshake"
        );
        assert!(matches!(phases.last(), Some(ReviewPhase::Done { .. })));
        for run_id in emitter.run_ids() {
            assert!(
                !run_id.is_empty(),
                "every event must carry the shared run id"
            );
        }
    }

    #[tokio::test]
    async fn runner_emits_per_focus_failed_event_when_child_errors() {
        let plan = plan_from_list(&[
            (
                ReviewFocus::Security,
                StubAction::Ok(result(ReviewFocus::Security, 1, 0)),
            ),
            (
                ReviewFocus::Performance,
                StubAction::Err("provider key missing".to_string()),
            ),
        ]);
        let (emitter, emitter_dyn) = capturing_emitter_pair();

        let outcome = run_multi_focus_review_with_child(
            "openai".to_string(),
            "gpt-test".to_string(),
            "local".to_string(),
            None,
            &focuses_vec(&[ReviewFocus::Security, ReviewFocus::Performance]),
            PathBuf::from("/tmp"),
            "key".to_string(),
            emitter_dyn,
            Duration::from_secs(5),
            make_stub(plan),
        )
        .await
        .expect("at least one focus succeeded");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.errors.len(), 1);
        assert_eq!(
            outcome.errors.get("Performance"),
            Some(&"provider key missing".to_string())
        );

        let phases = emitter.phases();
        // MultiFocusStart + 2 running + 1 done + 1 failed + 1 done = 6.
        assert_eq!(phases.len(), 6);
        let failed = phases
            .iter()
            .find_map(|phase| match phase {
                ReviewPhase::MultiFocus {
                    focus,
                    status: MultiFocusStatus::Failed { detail },
                    ..
                } => Some((*focus, detail.clone())),
                _ => None,
            })
            .expect("a failed multi-focus event was emitted");
        assert_eq!(failed.0, ReviewFocus::Performance);
        assert_eq!(failed.1, "provider key missing");
    }

    #[tokio::test]
    async fn runner_emits_per_focus_failed_event_when_child_times_out() {
        // Security completes quickly; Performance sleeps longer than the
        // runner's deadline. The timeout path must emit a per-focus failed
        // event under the real focus name before the terminal phase, so the
        // UI leaves "running" state for Performance.
        let plan = plan_from_list(&[
            (
                ReviewFocus::Security,
                StubAction::Ok(result(ReviewFocus::Security, 1, 0)),
            ),
            (
                ReviewFocus::Performance,
                StubAction::Sleep(Duration::from_millis(500)),
            ),
        ]);
        let (emitter, emitter_dyn) = capturing_emitter_pair();

        let outcome = run_multi_focus_review_with_child(
            "openai".to_string(),
            "gpt-test".to_string(),
            "local".to_string(),
            None,
            &focuses_vec(&[ReviewFocus::Security, ReviewFocus::Performance]),
            PathBuf::from("/tmp"),
            "key".to_string(),
            emitter_dyn,
            Duration::from_millis(50),
            make_stub(plan),
        )
        .await
        .expect("Security succeeded so the run is not an error");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].focus, ReviewFocus::Security);
        assert_eq!(
            outcome.errors.get("Performance"),
            Some(&"Multi-focus review timed out.".to_string())
        );

        let phases = emitter.phases();
        // Find the Performance failed event.
        let timed_out_event = phases.iter().find_map(|phase| match phase {
            ReviewPhase::MultiFocus {
                focus,
                status: MultiFocusStatus::Failed { detail },
                ..
            } if *focus == ReviewFocus::Performance => Some(detail.clone()),
            _ => None,
        });
        assert_eq!(
            timed_out_event,
            Some("Multi-focus review timed out.".to_string()),
            "Performance must be reported as failed under its real name"
        );

        // The failed event must precede the terminal Done event.
        let failed_index = phases
            .iter()
            .position(|phase| {
                matches!(phase, ReviewPhase::MultiFocus {
                    focus,
                    status: MultiFocusStatus::Failed { .. },
                    ..
                } if *focus == ReviewFocus::Performance)
            })
            .expect("failed event was emitted");
        let done_index = phases
            .iter()
            .position(|phase| matches!(phase, ReviewPhase::Done { .. }))
            .expect("Done event was emitted");
        assert!(
            failed_index < done_index,
            "per-focus failed event must precede the terminal Done event"
        );
    }

    #[tokio::test]
    async fn runner_emits_failed_per_focus_when_all_focuses_time_out() {
        let plan = plan_from_list(&[
            (
                ReviewFocus::Security,
                StubAction::Sleep(Duration::from_millis(500)),
            ),
            (
                ReviewFocus::BugHunt,
                StubAction::Sleep(Duration::from_millis(500)),
            ),
            (
                ReviewFocus::Performance,
                StubAction::Sleep(Duration::from_millis(500)),
            ),
        ]);
        let (emitter, emitter_dyn) = capturing_emitter_pair();

        let error = run_multi_focus_review_with_child(
            "openai".to_string(),
            "gpt-test".to_string(),
            "local".to_string(),
            None,
            &ALL_FOCUSES.to_vec(),
            PathBuf::from("/tmp"),
            "key".to_string(),
            emitter_dyn,
            Duration::from_millis(20),
            make_stub(plan),
        )
        .await
        .expect_err("all focuses timed out so the run is an error");

        assert!(error.contains("All review focuses failed"));
        // Every focus must be reported under its real name in the errors map.
        for focus in ALL_FOCUSES {
            assert_eq!(
                emitter
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|event| {
                        matches!(&event.phase, ReviewPhase::MultiFocus {
                            focus: emitted_focus,
                            status: MultiFocusStatus::Failed { .. },
                            ..
                        } if *emitted_focus == *focus)
                    })
                    .count(),
                1,
                "{focus} must surface as a failed multi-focus event under its real name"
            );
        }

        let phases = emitter.phases();
        assert!(
            matches!(phases.last(), Some(ReviewPhase::Failed { .. })),
            "all-failed terminal phase must be a Failed event, not a Done event"
        );
    }

    #[tokio::test]
    async fn runner_reports_panicking_child_under_its_real_focus_name() {
        // A panicking child must be caught and reported under the real focus
        // name, not as "unknown-N". This is the P3 join-error identity fix.
        let plan = plan_from_list(&[
            (ReviewFocus::Security, StubAction::Panic("boom")),
            (
                ReviewFocus::BugHunt,
                StubAction::Ok(result(ReviewFocus::BugHunt, 1, 0)),
            ),
        ]);
        let (_emitter, emitter_dyn) = capturing_emitter_pair();

        let outcome = run_multi_focus_review_with_child(
            "openai".to_string(),
            "gpt-test".to_string(),
            "local".to_string(),
            None,
            &focuses_vec(&[ReviewFocus::Security, ReviewFocus::BugHunt]),
            PathBuf::from("/tmp"),
            "key".to_string(),
            emitter_dyn,
            Duration::from_secs(5),
            make_stub(plan),
        )
        .await
        .expect("BugHunt succeeded so the run is not an error");

        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].focus, ReviewFocus::BugHunt);
        let security_error = outcome
            .errors
            .get("Security")
            .expect("Security panic is reported under its real focus name");
        assert!(
            security_error.contains("panicked"),
            "panic detail surfaces: {security_error}"
        );
        assert!(security_error.contains("boom"));

        // No "unknown-" placeholder should have leaked into the error map.
        for key in outcome.errors.keys() {
            assert!(
                !key.starts_with("unknown-"),
                "focus identity must not be lost: {key}"
            );
        }
    }

    #[tokio::test]
    async fn runner_rejects_empty_focus_list_without_emitting_events() {
        let (emitter, emitter_dyn) = capturing_emitter_pair();
        let error = run_multi_focus_review_with_child(
            "openai".to_string(),
            "gpt-test".to_string(),
            "local".to_string(),
            None,
            &[],
            PathBuf::from("/tmp"),
            "key".to_string(),
            emitter_dyn,
            Duration::from_secs(1),
            make_stub(BTreeMap::new()),
        )
        .await
        .expect_err("empty focus list is rejected");
        assert!(error.contains("At least one review focus"));
        assert!(emitter.phases().is_empty());
    }

    #[test]
    fn focus_emitter_suppresses_terminal_events_and_stamps_run_id_and_focus() {
        let (emitter, emitter_dyn) = capturing_emitter_pair();
        let focus_emitter = FocusEmitter {
            parent: emitter_dyn,
            run_id: "agg-run".to_string(),
            focus: ReviewFocus::Security,
        };

        focus_emitter.emit_progress(ReviewProgressEvent::new(
            "child-run",
            ReviewPhase::Finalize {
                status: PhaseStatus::Running,
            },
        ));
        focus_emitter.emit_progress(ReviewProgressEvent::new(
            "child-run",
            ReviewPhase::Done {
                findings: 1,
                suppressed: 0,
            },
        ));
        focus_emitter.emit_progress(ReviewProgressEvent::new(
            "child-run",
            ReviewPhase::Failed {
                detail: "boom".to_string(),
            },
        ));

        let events = emitter.events.lock().unwrap();
        assert_eq!(events.len(), 1, "Done and Failed must be suppressed");
        assert_eq!(events[0].run_id, "agg-run", "aggregate run_id is stamped");
        assert_eq!(
            events[0].focus,
            Some(ReviewFocus::Security),
            "focus is stamped"
        );
        assert!(matches!(
            events[0].phase,
            ReviewPhase::Finalize {
                status: PhaseStatus::Running
            }
        ));
    }
}
