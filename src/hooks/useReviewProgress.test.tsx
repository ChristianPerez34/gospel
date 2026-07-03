import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useReviewProgress } from "./useReviewProgress";
import type { ReviewPhase, ReviewProgressEvent } from "../types";

type ReviewProgressListener = (event: { payload: ReviewProgressEvent }) => void;

let progressListener: ReviewProgressListener | null = null;

function emitProgress(phase: ReviewPhase) {
  if (!progressListener) {
    throw new Error("review-progress listener was not registered");
  }

  act(() => {
    progressListener?.({
      payload: {
        run_id: "run-1",
        phase,
        timestamp: 1783094400000,
      },
    });
  });
}

describe("useReviewProgress", () => {
  beforeEach(() => {
    progressListener = null;
    vi.mocked(listen).mockImplementation(async (_eventName, handler) => {
      progressListener = handler as ReviewProgressListener;
      return vi.fn();
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("formats Rust's nested failed chunk status without object coercion", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress({
      type: "detector",
      chunk: 1,
      totalChunks: 1,
      files: [".github/workflows/deploy-backend.yml"],
      candidateCount: 0,
      status: {
        failed: {
          kind: "provider_error",
          detail: "model rejected tool-capable request",
        },
      },
    });

    await waitFor(() => {
      expect(result.current.log[result.current.log.length - 1]?.text).toBe(
        "Detector chunk 1/1 failed (provider_error): model rejected tool-capable request",
      );
    });
    expect(result.current.pipeline.detector.status).toBe("failed");
    expect(result.current.log[result.current.log.length - 1]?.text).not.toContain(
      "[object Object]",
    );
  });

  it("reads nested phase failure details", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress({
      type: "validator",
      candidateCount: 2,
      status: { failed: { detail: "validator provider failed" } },
    });

    await waitFor(() => {
      expect(result.current.log[result.current.log.length - 1]?.text).toBe(
        "Validator failed: validator provider failed",
      );
    });
    expect(result.current.pipeline.validator).toBe("failed");
  });
});
