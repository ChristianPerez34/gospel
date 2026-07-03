import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  ReviewActivityEntry,
  ReviewPhase,
  ReviewPipelineState,
  ReviewProgressEvent,
} from "../types";

const INITIAL_PIPELINE: ReviewPipelineState = {
  detector: { chunk: 0, totalChunks: 0, candidateCount: 0, status: "idle" },
  validator: "idle",
  finalize: "idle",
  done: false,
  failed: false,
  failureDetail: null,
  findings: 0,
  suppressed: 0,
};

export interface UseReviewProgressState {
  runId: string | null;
  pipeline: ReviewPipelineState;
  log: ReviewActivityEntry[];
}

export interface UseReviewProgress extends UseReviewProgressState {
  /** Clear all progress state (call before starting a new review). */
  reset: () => void;
}

/** Cap the activity feed so a 20-batch scan can't grow it without bound. */
const MAX_LOG_ENTRIES = 400;

function describe(phase: ReviewPhase): string {
  switch (phase.type) {
    case "detector": {
      const where =
        phase.totalChunks > 0
          ? `chunk ${phase.chunk}/${phase.totalChunks}`
          : `chunk ${phase.chunk}`;
      const filesLabel =
        phase.files.length > 0 ? ` — ${phase.files.length} file${phase.files.length === 1 ? "" : "s"}` : "";
      if (phase.status === "starting") return "Review started";
      if (phase.status === "running") return `Detector ${where} (running)${filesLabel}`;
      if (phase.status === "done")
        return `Detector ${where} done — ${phase.candidateCount} candidate${phase.candidateCount === 1 ? "" : "s"}`;
      return `Detector ${where} failed (${phase.status.failed}): ${phase.status.detail}`;
    }
    case "validator":
      if (phase.status === "running")
        return `Validator running — ${phase.candidateCount} candidate${phase.candidateCount === 1 ? "" : "s"}`;
      if (phase.status === "done") return "Validator done";
      return `Validator failed: ${phase.status.detail}`;
    case "finalize":
      if (phase.status === "running") return "Finalizing results";
      if (phase.status === "done") return "Finalize complete";
      return `Finalize failed: ${phase.status.detail}`;
    case "done":
      return `Review complete — ${phase.findings} finding${phase.findings === 1 ? "" : "s"}, ${phase.suppressed} suppressed`;
    case "failed":
      return `Review failed: ${phase.detail}`;
  }
}

function isChunkDone(status: unknown): status is "done" {
  return status === "done";
}

function isChunkFailed(status: unknown): status is { failed: string; detail: string } {
  return typeof status === "object" && status !== null && "failed" in status;
}

function reducePipeline(
  prev: ReviewPipelineState,
  phase: ReviewPhase,
): ReviewPipelineState {
  const next: ReviewPipelineState = { ...prev, detector: { ...prev.detector } };

  switch (phase.type) {
    case "detector": {
      const isLast = phase.totalChunks > 0 && phase.chunk === phase.totalChunks;
      if (phase.status === "starting") {
        next.detector = { chunk: 0, totalChunks: 0, candidateCount: 0, status: "active" };
      } else if (phase.status === "running") {
        next.detector = {
          chunk: phase.chunk,
          totalChunks: phase.totalChunks,
          candidateCount: phase.candidateCount,
          status: "active",
        };
      } else if (isChunkDone(phase.status)) {
        next.detector = {
          chunk: phase.chunk,
          totalChunks: phase.totalChunks,
          candidateCount: phase.candidateCount,
          status: isLast ? "done" : "active",
        };
      } else if (isChunkFailed(phase.status)) {
        // A chunk failed; the run-level `failed` event will mark the node if
        // the whole run fails. Keep the node active so other chunks can still
        // progress, but reflect the failure if it was the last chunk.
        next.detector = {
          chunk: phase.chunk,
          totalChunks: phase.totalChunks,
          candidateCount: phase.candidateCount,
          status: isLast ? "failed" : "active",
        };
      }
      break;
    }
    case "validator":
      next.detector = { ...prev.detector, status: "done" };
      next.validator = phase.status === "running" ? "active" : phase.status === "done" ? "done" : "failed";
      break;
    case "finalize":
      next.detector = { ...prev.detector, status: "done" };
      next.validator = "done";
      next.finalize = phase.status === "running" ? "active" : phase.status === "done" ? "done" : "failed";
      break;
    case "done":
      next.detector = { ...prev.detector, status: "done" };
      next.validator = "done";
      next.finalize = "done";
      next.done = true;
      next.findings = phase.findings;
      next.suppressed = phase.suppressed;
      break;
    case "failed":
      next.failed = true;
      next.failureDetail = phase.detail;
      // Mark the currently-active node as failed so the pipeline reflects it.
      if (next.detector.status === "active") next.detector = { ...next.detector, status: "failed" };
      else if (next.validator === "active") next.validator = "failed";
      else if (next.finalize === "active") next.finalize = "failed";
      break;
  }
  return next;
}

export function useReviewProgress(): UseReviewProgress {
  const [state, setState] = useState<UseReviewProgressState>({
    runId: null,
    pipeline: INITIAL_PIPELINE,
    log: [],
  });
  const runIdRef = useRef<string | null>(null);

  const reset = useCallback(() => {
    runIdRef.current = null;
    setState({ runId: null, pipeline: INITIAL_PIPELINE, log: [] });
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    (async () => {
      try {
        unlisten = await listen<ReviewProgressEvent>("review-progress", (event) => {
          if (cancelled) return;
          const payload = event.payload;
          if (!payload?.run_id || !payload?.phase) return;

          setState((prev) => {
            // Reset on run_id change so a new review starts clean.
            const runChanged = runIdRef.current !== payload.run_id;
            const basePipeline = runChanged ? INITIAL_PIPELINE : prev.pipeline;
            const baseLog = runChanged ? [] : prev.log;
            runIdRef.current = payload.run_id;

            const pipeline = reducePipeline(basePipeline, payload.phase);
            const entry: ReviewActivityEntry = {
              timestamp: payload.timestamp,
              phase: payload.phase.type,
              text: describe(payload.phase),
            };
            const log = [...baseLog, entry];
            if (log.length > MAX_LOG_ENTRIES) {
              log.splice(0, log.length - MAX_LOG_ENTRIES);
            }
            return { runId: payload.run_id, pipeline, log };
          });
        });
      } catch (error) {
        console.error("[useReviewProgress] failed to listen for review-progress", error);
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return { ...state, reset };
}
