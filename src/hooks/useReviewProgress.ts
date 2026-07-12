import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  ChunkFailure,
  ChunkStatus,
  PhaseFailure,
  ReviewActivityEntry,
  ReviewFocus,
  ReviewNodeState,
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

export interface FocusProgress {
  runId: string;
  focus: ReviewFocus;
  pipeline: ReviewPipelineState;
}

export interface UseReviewProgressState {
  runId: string | null;
  done: boolean;
  failed: boolean;
  pipeline: ReviewPipelineState;
  perFocus: Partial<Record<ReviewFocus, FocusProgress>>;
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
        phase.files.length > 0
          ? ` — ${phase.files.length} file${phase.files.length === 1 ? "" : "s"}`
          : "";
      if (phase.status === "starting") return "Review started";
      if (phase.status === "running") return `Detector ${where} (running)${filesLabel}`;
      if (phase.status === "done")
        return `Detector ${where} done — ${phase.candidateCount} candidate${phase.candidateCount === 1 ? "" : "s"}`;
      return `Detector ${where} failed (${phase.status.failed.kind}): ${phase.status.failed.detail}`;
    }
    case "validator":
      if (phase.status === "running")
        return `Validator running — ${phase.candidateCount} candidate${phase.candidateCount === 1 ? "" : "s"}`;
      if (phase.status === "done") return "Validator done";
      return `Validator failed: ${phase.status.failed.detail}`;
    case "finalize":
      if (phase.status === "running") return "Finalizing results";
      if (phase.status === "done") return "Finalize complete";
      return `Finalize failed: ${phase.status.failed.detail}`;
    case "done":
      return `Review complete — ${phase.findings} finding${phase.findings === 1 ? "" : "s"}, ${phase.suppressed} suppressed`;
    case "failed":
      return `Review failed: ${phase.detail}`;
    case "detectorTool": {
      if ("call" in phase.event) {
        const args = phase.event.call.arguments as Record<string, unknown> | undefined;
        const target =
          typeof args === "object" && args !== null
            ? (args.path ?? args.file ?? args.target_file ?? args.query ?? args.command)
            : undefined;
        const targetText = typeof target === "string" ? `: ${target}` : " called";
        return `${phase.toolName}${targetText}`;
      }
      const summary = phase.event.result.summary;
      return `${phase.toolName} returned${summary ? ` — ${summary.slice(0, 60)}${summary.length > 60 ? "…" : ""}` : ""}`;
    }
    case "multiFocusStart":
      return `Multi-focus review started (${phase.total} focus${phase.total === 1 ? "" : "es"})`;
    case "multiFocus": {
      const progress = `${phase.completed}/${phase.total}`;
      if (phase.status === "running") return `${phase.focus} running [${progress}]`;
      if (phase.status === "done") {
        const parts = [
          `${phase.focus} done — ${phase.findings} finding${phase.findings === 1 ? "" : "s"}`,
        ];
        if (phase.suppressed > 0) parts.push(`${phase.suppressed} suppressed`);
        return parts.join(", ");
      }
      return `${phase.focus} failed [${progress}]: ${isPhaseFailed(phase.status) ? phase.status.failed.detail : "unknown"}`;
    }
  }
}

function isChunkDone(status: unknown): status is "done" {
  return status === "done";
}

function hasObjectFailure<TFailure>(
  status: unknown,
  isFailure: (value: unknown) => value is TFailure
): status is { failed: TFailure } {
  return (
    typeof status === "object" &&
    status !== null &&
    "failed" in status &&
    isFailure((status as { failed?: unknown }).failed)
  );
}

function isChunkFailure(value: unknown): value is ChunkFailure {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Partial<ChunkFailure>).kind === "string" &&
    typeof (value as Partial<ChunkFailure>).detail === "string"
  );
}

function isPhaseFailure(value: unknown): value is PhaseFailure {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Partial<PhaseFailure>).detail === "string"
  );
}

function isChunkFailed(status: unknown): status is Extract<ChunkStatus, { failed: ChunkFailure }> {
  return hasObjectFailure(status, isChunkFailure);
}

function isPhaseFailed(status: unknown): status is { failed: PhaseFailure } {
  return hasObjectFailure(status, isPhaseFailure);
}

function phaseStatusToNodeState(status: unknown, previous: ReviewNodeState): ReviewNodeState {
  if (status === "running") return "active";
  if (status === "done") return "done";
  if (isPhaseFailed(status)) return "failed";
  return previous;
}

function reducePipeline(prev: ReviewPipelineState, phase: ReviewPhase): ReviewPipelineState {
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
      next.detector = {
        ...prev.detector,
        status: prev.detector.status === "failed" ? "failed" : "done",
      };
      next.validator = phaseStatusToNodeState(phase.status, prev.validator);
      break;
    case "finalize":
      next.detector = {
        ...prev.detector,
        status: prev.detector.status === "failed" ? "failed" : "done",
      };
      next.validator = "done";
      next.finalize = phaseStatusToNodeState(phase.status, prev.finalize);
      break;
    case "done":
      next.detector = {
        ...prev.detector,
        status: prev.detector.status === "failed" ? "failed" : "done",
      };
      next.validator = "done";
      next.finalize = "done";
      next.done = true;
      next.findings = phase.findings;
      next.suppressed = phase.suppressed;
      break;
    case "multiFocusStart":
      next.detector = { ...prev.detector, status: "active" };
      break;
    case "multiFocus":
      if (phase.status === "running") {
        next.detector = { ...prev.detector, status: "active" };
      } else if (phase.status === "done") {
        next.detector = { ...prev.detector, status: "done" };
        next.validator = "done";
        next.finalize = "done";
        next.done = true;
        next.findings = phase.findings;
        next.suppressed = phase.suppressed;
      } else if (isPhaseFailed(phase.status)) {
        next.failed = true;
        next.failureDetail = phase.status.failed.detail;
        if (next.detector.status === "active")
          next.detector = { ...next.detector, status: "failed" };
        else if (next.validator === "active") next.validator = "failed";
        else if (next.finalize === "active") next.finalize = "failed";
        else next.detector = { ...next.detector, status: "failed" };
      }
      break;
    case "failed":
      next.failed = true;
      next.failureDetail = phase.detail;
      if (next.detector.status === "active") next.detector = { ...next.detector, status: "failed" };
      else if (next.validator === "active") next.validator = "failed";
      else if (next.finalize === "active") next.finalize = "failed";
      break;
    case "detectorTool":
      // Tool-call activity is logged but does not move the pipeline.
      break;
  }
  return next;
}

function reduceReviewState(
  prev: UseReviewProgressState,
  runId: string,
  phase: ReviewPhase,
  focus?: ReviewFocus
): UseReviewProgressState {
  const next: UseReviewProgressState = {
    ...prev,
    perFocus: { ...prev.perFocus },
  };

  if (focus) {
    const existing = next.perFocus[focus];
    const focusProgress: FocusProgress = existing ?? {
      runId,
      focus,
      pipeline: INITIAL_PIPELINE,
    };
    next.perFocus[focus] = {
      ...focusProgress,
      pipeline: reducePipeline(focusProgress.pipeline, phase),
    };
  } else {
    next.pipeline = reducePipeline(prev.pipeline, phase);
  }

  const focusPipelines = Object.values(next.perFocus);
  next.done =
    next.pipeline.done ||
    (focusPipelines.length > 0 && focusPipelines.every((progress) => progress.pipeline.done));
  next.failed = next.pipeline.failed || focusPipelines.some((progress) => progress.pipeline.failed);

  return next;
}

export function useReviewProgress(): UseReviewProgress {
  const [state, setState] = useState<UseReviewProgressState>({
    runId: null,
    done: false,
    failed: false,
    pipeline: INITIAL_PIPELINE,
    perFocus: {},
    log: [],
  });
  const runIdRef = useRef<string | null>(null);

  const reset = useCallback(() => {
    runIdRef.current = null;
    setState({
      runId: null,
      done: false,
      failed: false,
      pipeline: INITIAL_PIPELINE,
      perFocus: {},
      log: [],
    });
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    (async () => {
      try {
        const unsubscribe = await listen<ReviewProgressEvent>("review-progress", (event) => {
          if (cancelled) return;
          const payload = event.payload;
          if (!payload?.run_id || !payload?.phase) return;
          const phase = payload.phase;
          const focus = payload.focus;

          setState((prev) => {
            const isAggregate = focus == null;
            const runChanged =
              runIdRef.current === null || (isAggregate && runIdRef.current !== payload.run_id);
            if (runChanged) {
              runIdRef.current = payload.run_id;
            }

            const base: UseReviewProgressState = runChanged
              ? {
                  runId: payload.run_id,
                  done: false,
                  failed: false,
                  pipeline: INITIAL_PIPELINE,
                  perFocus: {},
                  log: [],
                }
              : prev;

            const next = reduceReviewState(base, payload.run_id, phase, focus);
            const entry: ReviewActivityEntry = {
              timestamp: payload.timestamp,
              phase: phase.type,
              focus: payload.focus,
              text: describe(phase),
            };
            const log = [...(runChanged ? [] : prev.log), entry];
            if (log.length > MAX_LOG_ENTRIES) {
              log.splice(0, log.length - MAX_LOG_ENTRIES);
            }
            return { ...next, runId: payload.run_id, log };
          });
        });
        if (cancelled) {
          unsubscribe();
        } else {
          unlisten = unsubscribe;
        }
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
