import { listen } from "@tauri-apps/api/event";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReviewFocus, ReviewPhase, ReviewProgressEvent } from "../types";
import { useReviewProgress } from "./useReviewProgress";

type ReviewProgressListener = (event: { payload: ReviewProgressEvent }) => void;

let progressListener: ReviewProgressListener | null = null;

function emitProgress(
  phase: ReviewPhase,
  focus?: ReviewFocus,
  runId = "run-1",
  metadata: { provider?: string; model?: string } = {}
) {
  if (!progressListener) {
    throw new Error("review-progress listener was not registered");
  }

  act(() => {
    progressListener?.({
      payload: {
        run_id: runId,
        focus,
        phase,
        timestamp: 1783094400000,
        ...metadata,
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
        "Detector chunk 1/1 failed (provider_error): model rejected tool-capable request"
      );
    });
    expect(result.current.pipeline.detector.status).toBe("failed");
    expect(result.current.log[result.current.log.length - 1]?.text).not.toContain(
      "[object Object]"
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
        "Validator failed: validator provider failed"
      );
    });
    expect(result.current.pipeline.validator).toBe("failed");
  });

  it("logs detector tool activity while keeping the pipeline idle", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress({
      type: "detectorTool",
      chunk: 1,
      toolName: "read_file",
      event: { call: { id: "call-1", arguments: { path: "/src/main.rs" } } },
    });

    await waitFor(() => {
      expect(result.current.log).toHaveLength(1);
    });
    expect(result.current.log[0]?.text).toBe("read_file: /src/main.rs");
    expect(result.current.pipeline.detector.status).toBe("idle");
  });

  it("retains the runtime model from the review start event", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress(
      {
        type: "detector",
        chunk: 0,
        totalChunks: 0,
        files: [],
        candidateCount: 0,
        status: "starting",
      },
      "Security",
      "run-model",
      { provider: "anthropic", model: "claude-sonnet-4" }
    );

    await waitFor(() => {
      expect(result.current.provider).toBe("anthropic");
      expect(result.current.model).toBe("claude-sonnet-4");
    });
  });

  it("retains structured reviewer tool calls and pairs their results by id", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress(
      {
        type: "detectorTool",
        chunk: 1,
        toolName: "read_file",
        event: { call: { id: "call-7", arguments: { path: "src/review/mod.rs" } } },
      },
      "Architecture"
    );
    emitProgress(
      {
        type: "detectorTool",
        chunk: 1,
        toolName: "read_file",
        event: { result: { id: "call-7", summary: "file contents" } },
      },
      "Architecture"
    );

    await waitFor(() => {
      expect(result.current.tools).toEqual([
        {
          id: "call-7",
          focus: "Architecture",
          stage: "detector",
          chunk: 1,
          toolName: "read_file",
          arguments: { path: "src/review/mod.rs" },
          result: "file contents",
          status: "completed",
        },
      ]);
    });
  });

  it("retains validator file activity under the same review focus", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress(
      {
        type: "validatorTool",
        toolName: "read_file",
        event: { call: { id: "validator-call", arguments: { path: "src/lib.rs" } } },
      },
      "Security"
    );

    await waitFor(() => {
      expect(result.current.tools).toEqual([
        expect.objectContaining({
          id: "validator-call",
          focus: "Security",
          stage: "validator",
          chunk: 0,
          status: "calling",
        }),
      ]);
    });
  });

  it("logs multi-focus start event so the UI leaves waiting state", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    expect(result.current.log).toHaveLength(0);

    emitProgress({ type: "multiFocusStart", total: 3 });

    await waitFor(() => {
      expect(result.current.log).toHaveLength(1);
    });
    expect(result.current.log[0]?.text).toBe("Multi-focus review started (3 focuses)");
    expect(result.current.pipeline.detector.status).toBe("active");
  });

  it("logs a singular multi-focus start when total is 1", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress({ type: "multiFocusStart", total: 1 });

    await waitFor(() => {
      expect(result.current.log).toHaveLength(1);
    });
    expect(result.current.log[0]?.text).toBe("Multi-focus review started (1 focus)");
  });

  it("formats failed multi-focus event with focus name and detail", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress(
      {
        type: "multiFocus",
        focus: "Performance",
        completed: 2,
        total: 3,
        findings: 0,
        suppressed: 0,
        status: { failed: { detail: "API key missing" } },
      },
      "Performance"
    );

    await waitFor(() => {
      expect(result.current.log).toHaveLength(1);
    });
    const text = result.current.log[0]?.text;
    expect(text).toBe("Performance failed [2/3]: API key missing");
    expect(text).not.toContain("[object Object]");
    expect(result.current.perFocus.Performance?.pipeline.failed).toBe(true);
  });

  it("routes per-focus phases to perFocus pipelines and computes whole-run done", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress({ type: "multiFocusStart", total: 2 });
    emitProgress(
      {
        type: "multiFocus",
        focus: "Security",
        completed: 0,
        total: 2,
        findings: 0,
        suppressed: 0,
        status: "running",
      },
      "Security"
    );
    emitProgress(
      {
        type: "detector",
        chunk: 1,
        totalChunks: 1,
        files: [],
        candidateCount: 2,
        status: "running",
      },
      "Security"
    );
    emitProgress({ type: "validator", candidateCount: 2, status: "running" }, "Security");
    emitProgress({ type: "finalize", status: "running" }, "Security");
    emitProgress({ type: "done", findings: 2, suppressed: 0 }, "Security");

    emitProgress(
      {
        type: "multiFocus",
        focus: "BugHunt",
        completed: 1,
        total: 2,
        findings: 1,
        suppressed: 0,
        status: "done",
      },
      "BugHunt"
    );

    await waitFor(() => {
      expect(result.current.done).toBe(true);
    });

    expect(result.current.perFocus.Security?.pipeline.done).toBe(true);
    expect(result.current.perFocus.Security?.pipeline.findings).toBe(2);
    expect(result.current.perFocus.BugHunt?.pipeline.done).toBe(true);
    expect(result.current.failed).toBe(false);
    expect(result.current.log.length).toBeGreaterThanOrEqual(6);
    expect(result.current.log[result.current.log.length - 1]?.focus).toBe("BugHunt");
  });

  it("computes failed when any per-focus pipeline fails", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress({ type: "multiFocusStart", total: 2 });
    emitProgress(
      {
        type: "multiFocus",
        focus: "Security",
        completed: 1,
        total: 2,
        findings: 0,
        suppressed: 0,
        status: "done",
      },
      "Security"
    );
    emitProgress(
      {
        type: "multiFocus",
        focus: "BugHunt",
        completed: 1,
        total: 2,
        findings: 0,
        suppressed: 0,
        status: { failed: { detail: "timeout" } },
      },
      "BugHunt"
    );

    await waitFor(() => {
      expect(result.current.failed).toBe(true);
    });

    expect(result.current.done).toBe(false);
    expect(result.current.perFocus.BugHunt?.pipeline.failed).toBe(true);
    expect(result.current.perFocus.BugHunt?.pipeline.failureDetail).toBe("timeout");
  });

  it("ignores focused events from a different active run", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress(
      {
        type: "detector",
        chunk: 1,
        totalChunks: 2,
        files: ["src/current.ts"],
        candidateCount: 1,
        status: "running",
      },
      "Security",
      "current-run"
    );
    emitProgress({ type: "done", findings: 9, suppressed: 0 }, "Security", "stale-run");

    await waitFor(() => {
      expect(result.current.runId).toBe("current-run");
    });
    expect(result.current.done).toBe(false);
    expect(result.current.perFocus.Security?.pipeline.findings).toBe(0);
    expect(result.current.log).toHaveLength(1);
  });

  it("ignores stale aggregate start events from a different run", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress(
      {
        type: "detector",
        chunk: 1,
        totalChunks: 2,
        files: ["src/current.ts"],
        candidateCount: 1,
        status: "running",
      },
      "Security",
      "current-run"
    );

    const expectedPipeline = { ...result.current.pipeline };

    emitProgress({ type: "multiFocusStart", total: 5 }, undefined, "stale-run");

    await waitFor(() => {
      expect(result.current.runId).toBe("current-run");
    });
    expect(result.current.pipeline).toEqual(expectedPipeline);
    expect(result.current.log).toHaveLength(1);
    expect(result.current.log[0]?.text).toContain("chunk 1/2");
  });

  it("ignores stale aggregate terminal events from a different run", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress(
      {
        type: "detector",
        chunk: 1,
        totalChunks: 2,
        files: ["src/current.ts"],
        candidateCount: 1,
        status: "running",
      },
      "Security",
      "current-run"
    );

    const initialLogText = result.current.log[0]?.text;

    emitProgress({ type: "done", findings: 9, suppressed: 0 }, undefined, "stale-run");

    await waitFor(() => {
      expect(result.current.runId).toBe("current-run");
    });
    expect(result.current.done).toBe(false);
    expect(result.current.pipeline.failed).toBe(false);
    expect(result.current.log).toHaveLength(1);
    expect(result.current.log[0]?.text).toBe(initialLogText);
  });

  it("ignores stale events arriving between reset and a new review invocation", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress({ type: "multiFocusStart", total: 2 }, undefined, "first-run");
    await waitFor(() => {
      expect(result.current.runId).toBe("first-run");
    });

    act(() => {
      result.current.reset();
    });

    expect(result.current.runId).toBeNull();

    // Late event from first-run arrives after reset, before second-run starts
    emitProgress({ type: "done", findings: 5, suppressed: 0 }, undefined, "first-run");

    expect(result.current.runId).toBeNull();

    // Now second-run starts
    emitProgress({ type: "multiFocusStart", total: 3 }, undefined, "second-run");

    await waitFor(() => {
      expect(result.current.runId).toBe("second-run");
    });
    expect(result.current.log).toHaveLength(1);
    expect(result.current.log[0]?.text).toContain("3 focuses");
  });

  it("rejects an event queued before reset when reset flushes first", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress({ type: "multiFocusStart", total: 2 }, undefined, "first-run");
    await waitFor(() => {
      expect(result.current.runId).toBe("first-run");
    });

    act(() => {
      progressListener?.({
        payload: {
          run_id: "first-run",
          phase: { type: "done", findings: 5, suppressed: 0 },
          timestamp: 1783094400000,
        },
      });
      result.current.reset();
    });

    expect(result.current.runId).toBeNull();
    expect(result.current.done).toBe(false);
    expect(result.current.log).toHaveLength(0);
  });

  it("accepts the first aggregate event after reset as a new run", async () => {
    const { result } = renderHook(() => useReviewProgress());

    await waitFor(() => {
      expect(progressListener).not.toBeNull();
    });

    emitProgress({ type: "multiFocusStart", total: 2 }, undefined, "first-run");
    await waitFor(() => {
      expect(result.current.runId).toBe("first-run");
    });

    act(() => {
      result.current.reset();
    });

    emitProgress({ type: "multiFocusStart", total: 3 }, undefined, "second-run");

    await waitFor(() => {
      expect(result.current.runId).toBe("second-run");
    });
    expect(result.current.log).toHaveLength(1);
    expect(result.current.log[0]?.text).toContain("3 focuses");
  });
});
