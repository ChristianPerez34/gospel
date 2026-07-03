//! Real-time review progress events.
//!
//! The review pipeline (`run_review`) can run for minutes on a full scan.
//! These types let the backend stream stage/chunk-level progress to the
//! frontend via the Tauri event bus (`review-progress` event), mirroring the
//! `SessionTurnEvent` pattern used by chat streaming.
//!
//! The [`ReviewProgressEmitter`] trait abstracts emission so unit tests in
//! `review/mod.rs` can use a no-op emitter and stay unchanged, while the real
//! `AppHandle`-backed emitter lives in `lib.rs` (kept out of this module so
//! `review/` stays decoupled from Tauri, exactly like `SessionTurnWorkspace`).

use serde::Serialize;

/// One progress event on the `review-progress` stream, discriminated by
/// [`ReviewPhase`]. The `run_id` is generated once at the start of `run_review`
/// so the frontend can key every event for a single run together from the
/// first event onward.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewProgressEvent {
    pub run_id: String,
    pub phase: ReviewPhase,
    pub timestamp: i64,
}

impl ReviewProgressEvent {
    pub fn new(run_id: &str, phase: ReviewPhase) -> Self {
        Self {
            run_id: run_id.to_string(),
            phase,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// Discriminated payload mirroring `SessionTurnEvent`'s `tag = "type"`
/// convention so the frontend can switch on `event.payload.type`.
///
/// `rename_all` renames the variant names used as the `type` tag
/// (`Detector` -> `detector`); `rename_all_fields` renames the fields inside
/// each struct variant (`total_chunks` -> `totalChunks`).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ReviewPhase {
    /// Detector stage. Emitted once with `chunk: 0` and `Starting` when the
    /// run begins, then per chunk with `Running` before `run_detector` and
    /// `Done`/`Failed` after the chunk's detector call returns.
    Detector {
        /// 1-indexed chunk number; `0` for the run-start handshake.
        chunk: usize,
        total_chunks: usize,
        files: Vec<String>,
        candidate_count: usize,
        status: ChunkStatus,
    },
    /// Validator stage. `Running` before `run_validator`, `Done` after.
    Validator {
        candidate_count: usize,
        status: PhaseStatus,
    },
    /// Finalize stage. `Running` at entry to `finalize_review_result`,
    /// `Done` when the result is sealed.
    Finalize { status: PhaseStatus },
    /// Whole-run success. Emitted after `finalize_review_result` succeeds.
    Done { findings: usize, suppressed: usize },
    /// Whole-run failure. Emitted when `run_review` returns an error
    /// (e.g. all detector invocations failed).
    Failed { detail: String },
}

/// Per-chunk detector status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChunkStatus {
    /// Run has started; chunks not yet known.
    Starting,
    /// Detector is running on this chunk.
    Running,
    /// Chunk completed (may have produced zero candidates).
    Done,
    /// This chunk's detector invocation failed.
    Failed { kind: String, detail: String },
}

/// Stage-level status for Validator / Finalize.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PhaseStatus {
    Running,
    Done,
    /// Reserved for future hard-failure surfacing at the validator/finalize
    /// stage. Whole-run failures currently emit [`ReviewPhase::Failed`].
    #[allow(dead_code)]
    Failed { detail: String },
}

/// Abstracts emission of review progress events so the review pipeline can
/// stream progress without depending on Tauri directly. The real
/// `AppHandle`-backed implementation lives in `lib.rs`; tests use
/// [`NoopReviewProgressEmitter`].
pub trait ReviewProgressEmitter: Send + Sync {
    fn emit_progress(&self, event: ReviewProgressEvent);
}

/// Emitter that discards every event. Used by the `run_review` *tool* path
/// (agent-initiated reviews render text-only in the chat transcript for now)
/// and by unit tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopReviewProgressEmitter;

impl ReviewProgressEmitter for NoopReviewProgressEmitter {
    fn emit_progress(&self, _event: ReviewProgressEvent) {}
}
