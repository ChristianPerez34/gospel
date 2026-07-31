import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useConstellation } from "./useConstellation";
import type { UseReviewProgressState } from "./useReviewProgress";

function reviewProgressWithFileRead(): UseReviewProgressState {
  return {
    runId: "run-1",
    provider: "anthropic",
    model: "claude-sonnet-4",
    done: false,
    failed: false,
    pipeline: {
      detector: { chunk: 0, totalChunks: 0, candidateCount: 0, status: "active" },
      validator: "idle",
      finalize: "idle",
      done: false,
      failed: false,
      failureDetail: null,
      findings: 0,
      suppressed: 0,
    },
    perFocus: {
      Architecture: {
        runId: "run-1",
        focus: "Architecture",
        pipeline: {
          detector: { chunk: 1, totalChunks: 2, candidateCount: 0, status: "active" },
          validator: "idle",
          finalize: "idle",
          done: false,
          failed: false,
          failureDetail: null,
          findings: 0,
          suppressed: 0,
        },
      },
    },
    tools: [
      {
        id: "call-7",
        focus: "Architecture",
        stage: "detector",
        chunk: 1,
        toolName: "read_file",
        arguments: { path: "src/review/mod.rs" },
        status: "calling",
      },
    ],
    log: [],
  };
}

describe("useConstellation", () => {
  it("creates reviewer-owned file nodes with the runtime model", () => {
    const { result } = renderHook(() =>
      useConstellation([], null, reviewProgressWithFileRead(), false)
    );

    expect(result.current.reviewerNodes).toContainEqual(
      expect.objectContaining({
        id: "Architecture",
        provider: "anthropic",
        model: "claude-sonnet-4",
      })
    );
    expect(result.current.toolNodes).toContainEqual(
      expect.objectContaining({
        id: "review-tool:Architecture:detector:1:call-7",
        reviewerId: "Architecture",
        kind: "read",
        target: "src/review/mod.rs",
        status: "running",
      })
    );
  });

  it("keeps surviving reviewer activity visible after another focus fails", () => {
    const progress = reviewProgressWithFileRead();
    progress.failed = true;
    progress.perFocus.Security = {
      runId: "run-1",
      focus: "Security",
      pipeline: {
        detector: { chunk: 1, totalChunks: 1, candidateCount: 0, status: "failed" },
        validator: "idle",
        finalize: "idle",
        done: false,
        failed: true,
        failureDetail: "provider unavailable",
        findings: 0,
        suppressed: 0,
      },
    };

    const { result } = renderHook(() => useConstellation([], null, progress, false));

    expect(result.current.reviewActive).toBe(true);
    expect(result.current.reviewVisible).toBe(true);
    expect(result.current.reviewerNodes).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ focus: "Architecture", status: "active" }),
        expect.objectContaining({ focus: "Security", status: "failed" }),
      ])
    );
    expect(result.current.toolNodes).toContainEqual(
      expect.objectContaining({
        reviewerId: "Architecture",
        target: "src/review/mod.rs",
      })
    );
  });
});
