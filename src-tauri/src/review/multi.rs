use super::{
    run_review, MultiFocusStatus, NoopReviewProgressEmitter, ReviewConfig, ReviewFocus,
    ReviewPhase, ReviewProgressEmitter, ReviewProgressEvent, ReviewResult,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
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

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MultiReviewResult {
    pub results: Vec<ReviewResult>,
    pub errors: BTreeMap<String, String>,
    pub summary: String,
    pub files_scanned: usize,
    pub total_findings: usize,
    pub total_suppressed: usize,
}

pub async fn run_multi_focus_review(
    provider: String,
    model: String,
    mode: String,
    pr_number: Option<u64>,
    focuses: &[ReviewFocus],
    workspace_path: PathBuf,
    api_key: String,
    emitter: &dyn ReviewProgressEmitter,
) -> Result<MultiReviewResult, String> {
    if focuses.is_empty() {
        return Err("At least one review focus is required.".to_string());
    }

    let run_id = Uuid::new_v4().to_string();
    let unique_focuses: BTreeSet<ReviewFocus> = focuses.iter().copied().collect();
    let total = unique_focuses.len();

    emitter.emit_progress(ReviewProgressEvent::new(
        &run_id,
        ReviewPhase::MultiFocus {
            focus: String::new(),
            completed: 0,
            total,
            findings: 0,
            suppressed: 0,
            status: MultiFocusStatus::Starting,
        },
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
        let focus_name = focus.to_string();
        let emitter_run_id = run_id.clone();
        emitter.emit_progress(ReviewProgressEvent::new(
            &emitter_run_id,
            ReviewPhase::MultiFocus {
                focus: focus_name.clone(),
                completed: 0,
                total,
                findings: 0,
                suppressed: 0,
                status: MultiFocusStatus::Running,
            },
        ));
        join_set.spawn(async move {
            let result =
                run_review(config, workspace_path, api_key, &NoopReviewProgressEmitter).await;
            (focus, result)
        });
    }

    let mut results = Vec::new();
    let mut errors = BTreeMap::new();
    let deadline = tokio::time::Instant::now() + MULTI_REVIEW_TIMEOUT;
    let mut completed = 0usize;
    let mut total_findings = 0usize;
    let mut total_suppressed = 0usize;

    while let Ok(Some(outcome)) = tokio::time::timeout_at(deadline, join_set.join_next()).await {
        match outcome {
            Ok((focus, Ok(result))) => {
                completed += 1;
                total_findings += result.comments.len();
                total_suppressed += result.suppressed_count;
                emitter.emit_progress(ReviewProgressEvent::new(
                    &run_id,
                    ReviewPhase::MultiFocus {
                        focus: focus.to_string(),
                        completed,
                        total,
                        findings: result.comments.len(),
                        suppressed: result.suppressed_count,
                        status: MultiFocusStatus::Done,
                    },
                ));
                results.push(result);
            }
            Ok((focus, Err(error))) => {
                completed += 1;
                errors.insert(focus.to_string(), error.clone());
                emitter.emit_progress(ReviewProgressEvent::new(
                    &run_id,
                    ReviewPhase::MultiFocus {
                        focus: focus.to_string(),
                        completed,
                        total,
                        findings: 0,
                        suppressed: 0,
                        status: MultiFocusStatus::Failed {
                            detail: error,
                        },
                    },
                ));
            }
            Err(join_error) => {
                completed += 1;
                let key = format!("unknown-{}", errors.len());
                errors.insert(key, join_error.to_string());
            }
        }
    }

    let timed_out = !join_set.is_empty();
    if timed_out {
        join_set.abort_all();
        errors.insert(
            "timeout".to_string(),
            "Multi-focus review timed out.".to_string(),
        );
    }

    results.sort_by_key(|result| focus_order(result.focus));

    if results.is_empty() {
        let detail = format_all_failed(&errors);
        emitter.emit_progress(ReviewProgressEvent::new(&run_id, ReviewPhase::Failed {
            detail: detail.clone(),
        }));
        return Err(detail);
    }

    let files_scanned = results
        .iter()
        .map(|result| result.files_scanned)
        .max()
        .unwrap_or(0);
    let summary = build_multi_summary(&results, &errors);

    emitter.emit_progress(ReviewProgressEvent::new(
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
    use crate::review::{MultiFocusStatus, ReviewMode, ReviewPhase, ReviewProgressEvent, Severity, SignalTier};

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
                focus: "Security".to_string(),
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
    fn multi_focus_starting_event_serializes_with_empty_focus() {
        let event = ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::MultiFocus {
                focus: String::new(),
                completed: 0,
                total: 5,
                findings: 0,
                suppressed: 0,
                status: MultiFocusStatus::Starting,
            },
        );
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["phase"]["type"], "multiFocus");
        assert_eq!(json["phase"]["focus"], "");
        assert_eq!(json["phase"]["completed"], 0);
        assert_eq!(json["phase"]["status"], "starting");
    }

    #[test]
    fn multi_focus_failed_event_serializes_nested_detail() {
        let event = ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::MultiFocus {
                focus: "Performance".to_string(),
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
        assert_eq!(json["phase"]["status"]["failed"]["detail"], "provider error");
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
    fn multi_focus_events_share_one_run_id() {
        let emitter = CapturingEmitter::default();

        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-abc",
            ReviewPhase::MultiFocus {
                focus: String::new(),
                completed: 0,
                total: 2,
                findings: 0,
                suppressed: 0,
                status: MultiFocusStatus::Starting,
            },
        ));
        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-abc",
            ReviewPhase::MultiFocus {
                focus: "Security".to_string(),
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
        assert!(matches!(phases[0], ReviewPhase::MultiFocus { status: MultiFocusStatus::Starting, .. }));
        assert!(matches!(phases[1], ReviewPhase::MultiFocus { status: MultiFocusStatus::Done, .. }));
        assert!(matches!(phases[2], ReviewPhase::Done { .. }));
    }

    #[test]
    fn multi_focus_start_event_emitted_before_done_events() {
        let emitter = CapturingEmitter::default();

        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::MultiFocus {
                focus: String::new(),
                completed: 0,
                total: 3,
                findings: 0,
                suppressed: 0,
                status: MultiFocusStatus::Starting,
            },
        ));
        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::MultiFocus {
                focus: "Security".to_string(),
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
        assert!(matches!(phases[0], ReviewPhase::MultiFocus { status: MultiFocusStatus::Starting, .. }));
    }

    #[test]
    fn partial_child_failures_emit_failed_focus_before_done() {
        let emitter = CapturingEmitter::default();

        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::MultiFocus {
                focus: String::new(),
                completed: 0,
                total: 2,
                findings: 0,
                suppressed: 0,
                status: MultiFocusStatus::Starting,
            },
        ));
        emitter.emit_progress(ReviewProgressEvent::new(
            "multi-run-1",
            ReviewPhase::MultiFocus {
                focus: "Security".to_string(),
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
                focus: "Performance".to_string(),
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
        assert!(matches!(phases[0], ReviewPhase::MultiFocus { status: MultiFocusStatus::Starting, .. }));
        assert!(matches!(phases[3], ReviewPhase::Done { .. }));
    }

    #[test]
    fn noop_emitter_handles_multi_focus_phases() {
        let noop = NoopReviewProgressEmitter;
        noop.emit_progress(ReviewProgressEvent::new(
            "run-1",
            ReviewPhase::MultiFocus {
                focus: "Security".to_string(),
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
                focus: "Performance".to_string(),
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
}
