import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApprovalRequest, CurrentTurn, Message, TurnBlock } from "../types";
import { setFrameSchedulerForTest, useChatStream } from "./useChatStream";

type ListenerCallback = (event: { payload: unknown }) => void;

interface CapturedListeners {
  [eventName: string]: ListenerCallback[];
}

let capturedListeners: CapturedListeners = {};
let unlistenCalls = 0;
let registeredUnlisteners: Array<() => void> = [];

function triggerEvent<T>(eventName: string, payload: T) {
  const listeners = capturedListeners[eventName] ?? [];
  for (const listener of listeners) {
    listener({ payload });
  }
}

interface RenderChatStreamOptions {
  onMessages?: React.Dispatch<React.SetStateAction<Message[]>>;
  onMessagesForSession?: (
    sessionId: string | null,
    updater: (prev: Message[]) => Message[]
  ) => void;
  onLiveTurnForSession?: (sessionId: string | null, turn: CurrentTurn | null) => void;
  onStatusChange?: (status: string) => void;
  onErrorToast?: (message: string, action?: { label: string; onClick: () => void }) => void;
  onSuccessToast?: (message: string) => void;
  onOpenSettings?: () => void;
  onResolveApproval?: (id: string, decision: string) => Promise<unknown>;
  sessionId?: string | null;
}

function renderChatStream(options: RenderChatStreamOptions = {}) {
  return renderHook(() => useChatStream(options));
}

function latestCompleteStreamingRunId(): string {
  const call = [...vi.mocked(invoke).mock.calls]
    .reverse()
    .find(([command]) => command === "complete_streaming");
  const runId = (call?.[1] as { runId?: string } | undefined)?.runId;
  if (!runId) throw new Error("complete_streaming was not invoked with a runId");
  return runId;
}

async function startBgRun(result: { current: { startStream: (o: never) => Promise<unknown> } }) {
  await act(async () => {
    await result.current.startStream({
      provider: "openai",
      prompt: "hi",
      model: "gpt-4o",
      sessionId: "session-b",
    } as never);
  });
  return latestCompleteStreamingRunId();
}

function livePublishes(mock: { mock: { calls: unknown[][] } }) {
  return mock.mock.calls.filter((call) => (call as unknown[])[1] != null);
}

const BASE_APPROVAL_REQUEST: ApprovalRequest = {
  id: "approval-1",
  kind: "command",
  tool_name: "shell",
  title: "Run command",
  summary: "Runs `ls`",
  reason: "Mutating shell command",
  risk: "mutating",
};

function turnText(turn: { blocks?: TurnBlock[] } | null | undefined): string {
  return (turn?.blocks ?? [])
    .filter((b) => b.kind === "text")
    .map((b) => (b as { text: string }).text)
    .join("");
}

function lastLiveTurn(
  onLiveTurnForSession: ReturnType<typeof vi.fn>,
  sessionId: string
): CurrentTurn | null {
  const calls = onLiveTurnForSession.mock.calls.filter(([sid]) => sid === sessionId);
  return calls.length > 0 ? (calls[calls.length - 1][1] as CurrentTurn | null) : null;
}

function streamedRunIds(): string[] {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "complete_streaming")
    .map(([, args]) => (args as { runId: string }).runId);
}

describe("useChatStream", () => {
  beforeEach(() => {
    capturedListeners = {};
    unlistenCalls = 0;
    registeredUnlisteners = [];
    vi.mocked(listen).mockImplementation(async (eventName, callback) => {
      if (!capturedListeners[eventName]) capturedListeners[eventName] = [];
      capturedListeners[eventName].push(callback as ListenerCallback);
      const unlisten = () => {
        unlistenCalls += 1;
      };
      registeredUnlisteners.push(unlisten);
      return unlisten;
    });
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "complete_streaming") return "run-active";
      return undefined as unknown;
    });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  describe("token and done listeners", () => {
    it("llm-token appends a token to currentTurn.blocks", async () => {
      const { result } = renderChatStream();
      await act(async () => {});

      await act(async () => {
        triggerEvent<string>("llm-token", "Hello");
      });

      expect(result.current.currentTurn).not.toBeNull();
      const turn = result.current.currentTurn!;
      expect(turn.blocks).toHaveLength(1);
      expect(turn.blocks[0]).toMatchObject({ kind: "text", text: "Hello" });
    });

    it("llm-token with no current turn lazily creates one", async () => {
      const { result } = renderChatStream();
      await act(async () => {});

      expect(result.current.currentTurn).toBeNull();

      await act(async () => {
        triggerEvent<string>("llm-token", "lazy");
      });

      expect(result.current.currentTurn).not.toBeNull();
      const turn = result.current.currentTurn!;
      expect(turn.id).toMatch(/^turn-/);
      expect(turn.createdAt).toBeInstanceOf(Date);
      expect(turn.blocks).toHaveLength(1);
    });

    it("llm-done finalizes the turn, strips reasoning blocks, calls onMessages", async () => {
      const onMessages = vi.fn();
      const onStatusChange = vi.fn();
      const { result } = renderChatStream({ onMessages, onStatusChange });
      await act(async () => {});

      await act(async () => {
        triggerEvent("llm-token", "visible-text");
      });
      await act(async () => {
        triggerEvent("llm-reasoning", { id: "r1", text: "secret thoughts", phase: "delta" });
      });

      expect(result.current.currentTurn!.blocks.some((b) => b.kind === "reasoning")).toBe(true);

      await act(async () => {
        triggerEvent<string>("llm-done", "final-answer");
      });

      expect(result.current.currentTurn).toBeNull();
      expect(onStatusChange).toHaveBeenCalledWith("connected");

      expect(onMessages).toHaveBeenCalledTimes(1);
      const setter = onMessages.mock.calls[0][0] as (prev: Message[]) => Message[];
      const next = setter([]);
      expect(next).toHaveLength(1);
      const message = next[0];
      expect(message.role).toBe("agent");
      expect(message.content).toBe("final-answer");
      expect(message.blocks).toBeDefined();
      expect(message.blocks!.some((b) => b.kind === "reasoning")).toBe(false);
      expect(message.blocks!.some((b) => b.kind === "text")).toBe(true);
    });

    it("llm-done with object payload prefers response field over streamed text", async () => {
      const onMessages = vi.fn();
      renderChatStream({ onMessages });
      await act(async () => {});

      await act(async () => {
        triggerEvent("llm-token", "streamed-text");
      });

      await act(async () => {
        triggerEvent("llm-done", {
          response: "backend-text",
          prompt_tokens: 10,
          response_tokens: 5,
          tool_calls: 0,
        });
      });

      expect(onMessages).toHaveBeenCalledTimes(1);
      const setter = onMessages.mock.calls[0][0] as (prev: Message[]) => Message[];
      const message = setter([])[0];
      expect(message.content).toBe("backend-text");
    });

    it("llm-error finalizes the turn with an error state", async () => {
      const onMessages = vi.fn();
      const onStatusChange = vi.fn();
      const onErrorToast = vi.fn();
      const { result } = renderChatStream({ onMessages, onStatusChange, onErrorToast });
      await act(async () => {});

      await act(async () => {
        triggerEvent("llm-token", "partial");
      });

      await act(async () => {
        triggerEvent<{ code: string; message: string }>("llm-error", {
          code: "RUNTIME",
          message: "boom",
        });
      });

      expect(result.current.currentTurn).toBeNull();
      expect(onStatusChange).toHaveBeenCalledWith("error");

      const setter = onMessages.mock.calls[0][0] as (prev: Message[]) => Message[];
      const message = setter([])[0];
      expect(message.role).toBe("agent");
      expect(message.error).toBe("boom");

      expect(onErrorToast).toHaveBeenCalledTimes(1);
      const [toastMessage, toastAction] = onErrorToast.mock.calls[0];
      expect(toastMessage).toBe("boom");
      expect(toastAction).toMatchObject({ label: "Retry" });
    });

    it("entitlement failures toast Open Settings instead of Retry", async () => {
      const onErrorToast = vi.fn();
      const onOpenSettings = vi.fn();
      renderChatStream({ onErrorToast, onOpenSettings });
      await act(async () => {});

      await act(async () => {
        triggerEvent<{ code: string; message: string }>("llm-error", {
          code: "ENTITLEMENT_FAILED",
          message: "Signing in again will not fix this.",
        });
      });

      expect(onErrorToast).toHaveBeenCalledWith(
        "Signing in again will not fix this.",
        expect.objectContaining({ label: "Open Settings" })
      );
    });

    it("auth-expired failures toast Sign in again", async () => {
      const onErrorToast = vi.fn();
      renderChatStream({ onErrorToast });
      await act(async () => {});

      await act(async () => {
        triggerEvent<{ code: string; message: string }>("llm-error", {
          code: "AUTH_EXPIRED",
          message: "Sign in again.",
        });
      });

      expect(onErrorToast).toHaveBeenCalledWith(
        "Sign in again.",
        expect.objectContaining({ label: "Sign in again" })
      );
    });
  });

  describe("tool and approval lifecycle", () => {
    it("llm-tool-call adds a pending tool block; matching llm-tool-result resolves it", async () => {
      const { result } = renderChatStream();
      await act(async () => {});

      await act(async () => {
        triggerEvent("llm-tool-call", { id: "t1", name: "shell", arguments: "ls" });
      });

      const turnAfterCall = result.current.currentTurn!;
      expect(turnAfterCall.blocks).toHaveLength(1);
      expect(turnAfterCall.blocks[0]).toMatchObject({
        kind: "tool",
        id: "t1",
        name: "shell",
        status: "calling",
      });

      await act(async () => {
        triggerEvent("llm-tool-result", { id: "t1", name: "shell", result: "output" });
      });

      const turnAfterResult = result.current.currentTurn!;
      expect(turnAfterResult.blocks).toHaveLength(1);
      expect(turnAfterResult.blocks[0]).toMatchObject({
        kind: "tool",
        id: "t1",
        status: "completed",
        result: "output",
      });
    });

    it("llm-tool-result with an unknown id logs a warning and appends as completed", async () => {
      const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
      const { result } = renderChatStream();
      await act(async () => {});

      await act(async () => {
        triggerEvent("llm-tool-result", { id: "orphan", name: "x", result: "r" });
      });

      expect(warnSpy).toHaveBeenCalled();
      const warnArgs = warnSpy.mock.calls[0];
      expect(String(warnArgs[0])).toContain("appending as completed");

      const turn = result.current.currentTurn!;
      const block = turn.blocks.find((b) => b.kind === "tool" && b.id === "orphan");
      expect(block).toBeDefined();
      expect(block).toMatchObject({
        kind: "tool",
        id: "orphan",
        status: "completed",
        result: "r",
      });

      warnSpy.mockRestore();
    });

    it("approval-requested emits the request; approval-resolved transitions the card", async () => {
      const onResolveApproval = vi.fn().mockResolvedValue(undefined);
      const { result } = renderChatStream({ onResolveApproval });
      await act(async () => {});

      await act(async () => {
        triggerEvent("approval-requested", BASE_APPROVAL_REQUEST);
      });

      const pendingTurn = result.current.currentTurn!;
      const approvalBlock = pendingTurn.blocks.find((b) => b.kind === "approval");
      expect(approvalBlock).toBeDefined();
      expect(approvalBlock).toMatchObject({
        kind: "approval",
        id: "approval-1",
        status: "pending",
        toolName: "shell",
        approvalKind: "command",
      });

      await act(async () => {
        await result.current.resolveApproval("approval-1", "approve");
      });
      expect(onResolveApproval).toHaveBeenCalledWith("approval-1", "approve");

      await act(async () => {
        triggerEvent("approval-resolved", { id: "approval-1", outcome: "approved" });
      });

      const resolvedTurn = result.current.currentTurn!;
      const resolvedBlock = resolvedTurn.blocks.find(
        (b) => b.kind === "approval" && b.id === "approval-1"
      );
      expect(resolvedBlock).toMatchObject({
        kind: "approval",
        id: "approval-1",
        status: "approved",
      });
    });

    it("resolveApproval invokes the resolver on every call (not call-site idempotent); card transitions once per approval-resolved event", async () => {
      const onResolveApproval = vi.fn().mockResolvedValue(undefined);
      const { result } = renderChatStream({ onResolveApproval });
      await act(async () => {});

      await act(async () => {
        triggerEvent("approval-requested", BASE_APPROVAL_REQUEST);
      });

      await act(async () => {
        await result.current.resolveApproval("approval-1", "approve");
        await result.current.resolveApproval("approval-1", "approve");
      });

      expect(onResolveApproval).toHaveBeenCalledTimes(2);

      await act(async () => {
        triggerEvent("approval-resolved", { id: "approval-1", outcome: "approved" });
      });

      const turn = result.current.currentTurn!;
      const approvalBlocks = turn.blocks.filter(
        (b) => b.kind === "approval" && b.id === "approval-1"
      );
      expect(approvalBlocks).toHaveLength(1);
      expect(approvalBlocks[0]).toMatchObject({ status: "approved" });
    });

    it("approval-requested with a background run_id stashes the card under that session, not currentTurn", async () => {
      const onLiveTurnForSession = vi.fn();
      const { result } = renderChatStream({
        sessionId: "session-a",
        onLiveTurnForSession,
      });
      await act(async () => {});

      // Start a run owned by a different (background) session.
      const runId = await startBgRun(result);

      await act(async () => {
        triggerEvent("approval-requested", {
          ...BASE_APPROVAL_REQUEST,
          run_id: runId,
        });
      });

      // The focused session's live turn stays empty; the card landed on the
      // owning session's stashed turn instead.
      expect(result.current.currentTurn).toBeNull();
      const backgroundCall = onLiveTurnForSession.mock.calls.find(
        ([sessionId]) => sessionId === "session-b"
      );
      expect(backgroundCall).toBeDefined();
      const stashedTurn = backgroundCall![1] as CurrentTurn;
      const approvalBlock = stashedTurn.blocks.find((b) => b.kind === "approval");
      expect(approvalBlock).toMatchObject({
        kind: "approval",
        id: "approval-1",
        status: "pending",
        toolName: "shell",
        approvalKind: "command",
      });
    });

    it("approval-resolved for a background run updates the stashed card", async () => {
      const onLiveTurnForSession = vi.fn();
      const { result } = renderChatStream({
        sessionId: "session-a",
        onLiveTurnForSession,
      });
      await act(async () => {});

      const runId = await startBgRun(result);

      await act(async () => {
        triggerEvent("approval-requested", {
          ...BASE_APPROVAL_REQUEST,
          run_id: runId,
        });
      });
      await act(async () => {
        triggerEvent("approval-resolved", {
          id: "approval-1",
          outcome: "approved",
          run_id: runId,
        });
      });

      expect(result.current.currentTurn).toBeNull();
      const sessionCalls = onLiveTurnForSession.mock.calls.filter(
        ([sessionId]) => sessionId === "session-b"
      );
      expect(sessionCalls.length).toBeGreaterThanOrEqual(2);
      const lastTurn = sessionCalls[sessionCalls.length - 1][1] as CurrentTurn;
      const block = lastTurn.blocks.find((b) => b.kind === "approval" && b.id === "approval-1");
      expect(block).toMatchObject({
        kind: "approval",
        id: "approval-1",
        status: "approved",
      });
    });

    it("approval-requested with no run_id falls back to the focused session", async () => {
      const onLiveTurnForSession = vi.fn();
      const { result } = renderChatStream({
        sessionId: "session-a",
        onLiveTurnForSession,
      });
      await act(async () => {});

      // A background run exists, but a legacy payload with no run_id still
      // routes to the focused session (status quo for runId-less emitters).
      await startBgRun(result);

      await act(async () => {
        triggerEvent("approval-requested", BASE_APPROVAL_REQUEST);
      });

      const turn = result.current.currentTurn!;
      const approvalBlock = turn.blocks.find((b) => b.kind === "approval");
      expect(approvalBlock).toMatchObject({
        kind: "approval",
        id: "approval-1",
        status: "pending",
      });
      // Nothing was stashed for the background session.
      expect(
        livePublishes(onLiveTurnForSession).filter(([sessionId]) => sessionId === "session-b")
      ).toHaveLength(0);
    });

    it("approval-requested with an unknown run_id is dropped as stale", async () => {
      const onLiveTurnForSession = vi.fn();
      const { result } = renderChatStream({
        sessionId: "session-a",
        onLiveTurnForSession,
      });
      await act(async () => {});

      await startBgRun(result);

      await act(async () => {
        triggerEvent("approval-requested", {
          ...BASE_APPROVAL_REQUEST,
          run_id: "run-stale",
        });
      });

      expect(result.current.currentTurn).toBeNull();
      expect(livePublishes(onLiveTurnForSession)).toHaveLength(0);
    });

    it("approval-resolved with no matching card creates no turn (no ghost)", async () => {
      const onLiveTurnForSession = vi.fn();
      const { result } = renderChatStream({
        sessionId: "session-a",
        onLiveTurnForSession,
      });
      await act(async () => {});

      const runId = await startBgRun(result);

      // Resolved for an unknown approval id on an active background run:
      // no stashed card exists, so nothing is published and no ghost turn
      // is created.
      await act(async () => {
        triggerEvent("approval-resolved", {
          id: "approval-unknown",
          outcome: "approved",
          run_id: runId,
        });
      });

      expect(result.current.currentTurn).toBeNull();
      expect(livePublishes(onLiveTurnForSession)).toHaveLength(0);
    });

    it("approval with run_id renders foreground when no session is focused", async () => {
      const onLiveTurnForSession = vi.fn();
      const { result } = renderChatStream({ onLiveTurnForSession });
      await act(async () => {});

      await act(async () => {
        result.current.startStream({
          provider: "openai",
          prompt: "hi",
          model: "gpt-4o",
          sessionId: "session-b",
        });
      });
      const runId = latestCompleteStreamingRunId();

      await act(async () => {
        triggerEvent("approval-requested", {
          ...BASE_APPROVAL_REQUEST,
          run_id: runId,
        });
      });

      // Matches llm-token behavior with focused == null: foreground.
      expect(result.current.currentTurn).not.toBeNull();
      expect(result.current.currentTurn!.blocks.find((b) => b.kind === "approval")).toMatchObject({
        id: "approval-1",
        status: "pending",
      });
      expect(livePublishes(onLiveTurnForSession)).toHaveLength(0);
    });
  });

  describe("listener lifecycle", () => {
    it("unmounting the hook calls each listener's unlisten", async () => {
      const { unmount } = renderChatStream();
      await act(async () => {});

      const registeredCount = registeredUnlisteners.length;
      expect(registeredCount).toBe(10);

      act(() => {
        unmount();
      });

      expect(unlistenCalls).toBe(registeredCount);
    });

    it("accepts events for the active run before complete_streaming resolves", async () => {
      const onMessages = vi.fn();
      const onStatusChange = vi.fn();
      let resolveStreaming: ((runId: string) => void) | undefined;
      vi.mocked(invoke).mockImplementation((cmd: string) => {
        if (cmd === "complete_streaming") {
          return new Promise<string>((resolve) => {
            resolveStreaming = resolve;
          });
        }
        return Promise.resolve(undefined as unknown);
      });

      // The hook's sessionId prop mirrors the focused session; the run's own
      // session is focused so its tokens stream into the visible turn.
      const { result } = renderChatStream({
        onMessages,
        onStatusChange,
        sessionId: "s-pending",
      });
      await act(async () => {});

      let streamingPromise: Promise<string> | undefined;
      act(() => {
        streamingPromise = result.current.startStream({
          provider: "openai",
          prompt: "hi",
          model: "gpt-4o",
          sessionId: "s-pending",
        });
      });

      const runId = latestCompleteStreamingRunId();
      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId,
          token: "arrived while invoke is pending",
        });
      });

      expect(result.current.currentTurn?.blocks[0]).toMatchObject({
        kind: "text",
        text: "arrived while invoke is pending",
      });

      await act(async () => {
        triggerEvent("llm-done", {
          runId,
          response: "completed while invoke is pending",
        });
      });

      expect(result.current.currentTurn).toBeNull();
      expect(onStatusChange).toHaveBeenCalledWith("connected");
      const setter = onMessages.mock.calls[0][0] as (prev: Message[]) => Message[];
      expect(setter([])[0]?.content).toBe("completed while invoke is pending");

      await act(async () => {
        resolveStreaming?.(runId);
        await streamingPromise;
      });
    });

    it("late llm-token with a non-matching run_id is ignored (per-run isolation)", async () => {
      // Focused on the run's session so its tokens stream into the visible
      // turn; an unknown run_id must not leak in.
      const { result } = renderChatStream({ sessionId: "s-1" });
      await act(async () => {});

      // Start a run: the active run id is registered. Simulate tokens from the
      // active run to build up a turn.
      await act(async () => {
        result.current.startStream({
          provider: "openai",
          prompt: "hi",
          model: "gpt-4o",
          sessionId: "s-1",
        });
      });
      const runId = latestCompleteStreamingRunId();
      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId,
          token: "first",
        });
      });
      expect(result.current.currentTurn).not.toBeNull();
      expect(result.current.currentTurn!.blocks).toHaveLength(1);

      // A late token from a *previous* run (different run_id) must be ignored,
      // not appended to the active turn.
      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId: "run-stale",
          token: "stale",
        });
      });

      expect(result.current.currentTurn).not.toBeNull();
      const turn = result.current.currentTurn!;
      expect(turn.blocks).toHaveLength(1);
      expect(turn.blocks[0]).toMatchObject({ kind: "text", text: "first" });
    });
  });

  describe("cancelStream", () => {
    it("cancelStream invokes cancel_streaming and finalizes the current turn as cancelled", async () => {
      const onMessages = vi.fn();
      const onStatusChange = vi.fn();
      let resolveStreaming: ((runId: string) => void) | undefined;
      vi.mocked(invoke).mockImplementation((cmd: string) => {
        if (cmd === "complete_streaming") {
          return new Promise<string>((resolve) => {
            resolveStreaming = resolve;
          });
        }
        return Promise.resolve(undefined as unknown);
      });
      const { result } = renderChatStream({
        onMessages,
        onStatusChange,
        sessionId: "s-cancel",
      });
      await act(async () => {});

      let streamingPromise: Promise<string> | undefined;
      act(() => {
        streamingPromise = result.current.startStream({
          provider: "openai",
          prompt: "hi",
          model: "gpt-4o",
          sessionId: "s-cancel",
        });
      });
      const runId = latestCompleteStreamingRunId();
      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId,
          token: "partial",
        });
      });
      expect(result.current.currentTurn).not.toBeNull();

      await act(async () => {
        await result.current.cancelStream();
      });

      expect(invoke).toHaveBeenCalledWith("cancel_streaming", { sessionId: "s-cancel" });
      expect(onStatusChange).toHaveBeenCalledWith("connected");
      expect(result.current.currentTurn).toBeNull();
      expect(onMessages).toHaveBeenCalledTimes(1);
      const setter = onMessages.mock.calls[0][0] as (prev: Message[]) => Message[];
      const message = setter([])[0];
      expect(message.role).toBe("agent");
      expect(message.content).toBe("partial");

      // Subsequent tokens do not create a new turn (run id is now null but the
      // stale guard drops events whose runId doesn't match null active).
      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId,
          token: "after-cancel",
        });
      });
      expect(result.current.currentTurn).toBeNull();

      await act(async () => {
        resolveStreaming?.(runId);
        await streamingPromise;
      });
    });

    it("cancelStream without an active run is a no-op", async () => {
      const onMessages = vi.fn();
      const { result } = renderChatStream({ onMessages, sessionId: "s-2" });
      await act(async () => {});

      await act(async () => {
        await result.current.cancelStream();
      });

      expect(onMessages).not.toHaveBeenCalled();
      expect(result.current.currentTurn).toBeNull();
    });
  });

  describe("buffered text coalescing", () => {
    // A deterministic frame queue: scheduled callbacks are held until
    // `flushFrames()` runs them. Cancelled handles are skipped. This proves
    // coalescing without wall-clock timing.
    let scheduledFrames: Map<number, () => void> = new Map();
    let frameHandles = 0;
    let cancelledHandles: number[] = [];

    beforeEach(() => {
      scheduledFrames = new Map();
      frameHandles = 0;
      cancelledHandles = [];
      setFrameSchedulerForTest({
        schedule: (cb) => {
          const handle = ++frameHandles;
          scheduledFrames.set(handle, () => cb());
          return handle;
        },
        cancel: (handle) => {
          cancelledHandles.push(handle);
          scheduledFrames.delete(handle);
        },
      });
    });

    afterEach(() => {
      setFrameSchedulerForTest(null);
    });

    function flushFrames() {
      const pending = [...scheduledFrames.values()];
      scheduledFrames.clear();
      for (const cb of pending) cb();
    }

    it("multiple tokens in one frame produce a single flush with concatenated text in order", async () => {
      const { result } = renderChatStream();
      await act(async () => {});

      await act(async () => {
        triggerEvent<string>("llm-token", "Hello");
        triggerEvent<string>("llm-token", " ");
        triggerEvent<string>("llm-token", "world");
      });

      // Before the frame flushes, no text block is visible yet.
      expect(result.current.currentTurn).toBeNull();
      expect(scheduledFrames.size).toBe(1);

      await act(async () => {
        flushFrames();
      });

      const turn = result.current.currentTurn!;
      expect(turn.blocks).toHaveLength(1);
      expect(turn.blocks[0]).toMatchObject({ kind: "text", text: "Hello world" });
    });

    it("a tool call flushes buffered text before the tool block", async () => {
      const { result } = renderChatStream();
      await act(async () => {});

      await act(async () => {
        triggerEvent<string>("llm-token", "pre-tool");
        triggerEvent("llm-tool-call", { id: "t1", name: "shell", arguments: "ls" });
      });

      const turn = result.current.currentTurn!;
      expect(turn.blocks).toHaveLength(2);
      expect(turn.blocks[0]).toMatchObject({ kind: "text", text: "pre-tool" });
      expect(turn.blocks[1]).toMatchObject({ kind: "tool", id: "t1" });
      // The pending frame was cancelled by the synchronous flush.
      expect(cancelledHandles.length).toBeGreaterThanOrEqual(1);
    });

    it("llm-done flushes buffered text even when the frame has not run", async () => {
      const onMessages = vi.fn();
      const { result } = renderChatStream({ onMessages });
      await act(async () => {});

      await act(async () => {
        triggerEvent<string>("llm-token", "almost");
        triggerEvent<string>("llm-token", " done");
        triggerEvent<string>("llm-done", "final-answer");
      });

      // done flushed synchronously; no frame should remain pending.
      expect(scheduledFrames.size).toBe(0);
      expect(result.current.currentTurn).toBeNull();

      const setter = onMessages.mock.calls[0][0] as (prev: Message[]) => Message[];
      const message = setter([])[0];
      // Authoritative backend response wins, but buffered text was flushed
      // into the finalized blocks before the message was captured.
      expect(message.content).toBe("final-answer");
      expect(message.blocks?.some((b) => b.kind === "text")).toBe(true);
    });

    it("llm-error flushes buffered text before finalizing", async () => {
      const onMessages = vi.fn();
      const { result } = renderChatStream({ onMessages });
      await act(async () => {});

      await act(async () => {
        triggerEvent<string>("llm-token", "partial");
        triggerEvent<{ code: string; message: string }>("llm-error", {
          code: "RUNTIME",
          message: "boom",
        });
      });

      expect(scheduledFrames.size).toBe(0);
      expect(result.current.currentTurn).toBeNull();

      const setter = onMessages.mock.calls[0][0] as (prev: Message[]) => Message[];
      const message = setter([])[0];
      expect(message.error).toBe("boom");
      expect(message.blocks?.some((b) => b.kind === "text" && b.text === "partial")).toBe(true);
    });

    it("cancelStream flushes buffered text before finalizing as cancelled", async () => {
      const onMessages = vi.fn();
      const { result } = renderChatStream({ onMessages, sessionId: "s-coal" });
      await act(async () => {});

      await act(async () => {
        result.current.startStream({
          provider: "openai",
          prompt: "hi",
          model: "gpt-4o",
          sessionId: "s-coal",
        });
      });
      const runId = latestCompleteStreamingRunId();

      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", { runId, token: "buffered" });
        await result.current.cancelStream();
      });

      expect(scheduledFrames.size).toBe(0);
      const setter = onMessages.mock.calls[0][0] as (prev: Message[]) => Message[];
      const message = setter([])[0];
      expect(message.content).toBe("buffered");
    });

    it("a stale token never enters the pending buffer", async () => {
      const { result } = renderChatStream({ sessionId: "s-stale" });
      await act(async () => {});

      await act(async () => {
        result.current.startStream({
          provider: "openai",
          prompt: "hi",
          model: "gpt-4o",
          sessionId: "s-stale",
        });
      });
      const runId = latestCompleteStreamingRunId();

      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId: "run-stale",
          token: "ignored",
        });
      });

      // No frame was scheduled because the stale token was rejected before
      // buffering.
      expect(scheduledFrames.size).toBe(0);
      expect(result.current.currentTurn).toBeNull();

      // An accepted token for the active run still works.
      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId,
          token: "accepted",
        });
        flushFrames();
      });
      expect(result.current.currentTurn!.blocks[0]).toMatchObject({
        kind: "text",
        text: "accepted",
      });
    });

    it("reset cancels pending work and a later run starts with an empty buffer", async () => {
      const { result } = renderChatStream({ sessionId: "s-next" });
      await act(async () => {});

      await act(async () => {
        triggerEvent<string>("llm-token", "first run");
      });
      expect(scheduledFrames.size).toBe(1);

      act(() => {
        result.current.resetStream();
      });

      // Reset cancelled the pending frame and cleared the buffer.
      expect(cancelledHandles.length).toBeGreaterThanOrEqual(1);
      expect(scheduledFrames.size).toBe(0);
      expect(result.current.currentTurn).toBeNull();

      // A later run starts clean.
      await act(async () => {
        result.current.startStream({
          provider: "openai",
          prompt: "hi",
          model: "gpt-4o",
          sessionId: "s-next",
        });
      });
      const runId = latestCompleteStreamingRunId();
      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId,
          token: "second run",
        });
        flushFrames();
      });
      expect(result.current.currentTurn!.blocks[0]).toMatchObject({
        kind: "text",
        text: "second run",
      });
    });

    it("unmount cancels pending work and cannot leak text into a later mount", async () => {
      const { unmount } = renderChatStream();
      await act(async () => {});

      await act(async () => {
        triggerEvent<string>("llm-token", "about to unmount");
      });
      expect(scheduledFrames.size).toBe(1);

      act(() => {
        unmount();
      });

      expect(cancelledHandles.length).toBeGreaterThanOrEqual(1);
      expect(scheduledFrames.size).toBe(0);
    });

    it("preserves tool-result pairing after buffered text flushes", async () => {
      const { result } = renderChatStream();
      await act(async () => {});

      await act(async () => {
        triggerEvent<string>("llm-token", "text before tool");
        triggerEvent("llm-tool-call", { id: "pair-1", name: "read_file", arguments: {} });
      });
      await act(async () => {
        triggerEvent("llm-tool-result", {
          id: "pair-1",
          name: "read_file",
          result: "contents",
        });
      });

      const turn = result.current.currentTurn!;
      expect(turn.blocks).toHaveLength(2);
      expect(turn.blocks[0]).toMatchObject({ kind: "text", text: "text before tool" });
      const toolBlock = turn.blocks[1];
      expect(toolBlock).toMatchObject({
        kind: "tool",
        id: "pair-1",
        status: "completed",
        result: "contents",
      });
    });
  });

  describe("per-session stream routing", () => {
    it("local-only run streams tokens into the focused currentTurn", async () => {
      const { result } = renderChatStream({ sessionId: "s-1" });
      await act(async () => {});

      // A local-only session has no backend session id; runSessionId carries
      // the frontend Session.id so the run still routes to the focused view.
      await act(async () => {
        result.current.startStream({
          provider: "openai",
          prompt: "hi",
          model: "gpt-4o",
          sessionId: null,
          runSessionId: "s-1",
        });
      });
      const runId = latestCompleteStreamingRunId();

      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId,
          token: "local tokens",
        });
      });

      expect(result.current.currentTurn).not.toBeNull();
      expect(result.current.currentTurn!.blocks[0]).toMatchObject({
        kind: "text",
        text: "local tokens",
      });
    });

    it("local-only run completion lands on its own transcript while another session is focused", async () => {
      const onMessagesForSession = vi.fn();
      const onLiveTurnForSession = vi.fn();
      const { result } = renderChatStream({
        sessionId: "s-2",
        onMessagesForSession,
        onLiveTurnForSession,
      });
      await act(async () => {});

      await act(async () => {
        result.current.startStream({
          provider: "openai",
          prompt: "hi",
          model: "gpt-4o",
          sessionId: null,
          runSessionId: "s-1",
        });
      });
      const runId = latestCompleteStreamingRunId();

      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId,
          token: "bg text",
        });
      });
      // s-1 is not focused; its tokens stash instead of rendering.
      expect(result.current.currentTurn).toBeNull();

      await act(async () => {
        triggerEvent("llm-done", { runId, response: "bg reply" });
      });

      // The completion is attributed to the run's own session — never the
      // focused session or a null-session fallback.
      expect(onMessagesForSession).toHaveBeenCalledWith("s-1", expect.any(Function));
      expect(onMessagesForSession).not.toHaveBeenCalledWith("s-2", expect.any(Function));
      expect(onMessagesForSession).not.toHaveBeenCalledWith(null, expect.any(Function));
    });

    it("token arriving between focus change and stash effect lands on the new session", async () => {
      const onLiveTurnForSession = vi.fn();
      const { result, rerender } = renderHook(
        ({ sessionId }: { sessionId: string | null }) =>
          useChatStream({ sessionId, onLiveTurnForSession }),
        { initialProps: { sessionId: "sess-a" as string | null } }
      );
      await act(async () => {});

      await act(async () => {
        result.current.startStream({
          provider: "openai",
          prompt: "a",
          model: "gpt-4o",
          sessionId: "sess-a",
        });
        result.current.startStream({
          provider: "openai",
          prompt: "b",
          model: "gpt-4o",
          sessionId: "sess-b",
        });
      });
      const [runA, runB] = streamedRunIds();

      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId: runA,
          token: "a-text",
        });
      });
      expect(turnText(result.current.currentTurn)).toBe("a-text");

      // Switch focus A -> B and emit B's token inside the same commit, before
      // the stash effect runs. The token is keyed to B's session from the
      // start, so it can never be spliced into A's stash.
      await act(async () => {
        rerender({ sessionId: "sess-b" });
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId: runB,
          token: "b-token",
        });
      });

      // B's buffered text surfaced on the restored visible turn; A's stash
      // holds only A's text.
      expect(turnText(result.current.currentTurn)).toBe("b-token");
      expect(turnText(lastLiveTurn(onLiveTurnForSession, "sess-a"))).toBe("a-text");
    });

    it("two concurrent runs while no session is focused keep separate buffers", async () => {
      const onLiveTurnForSession = vi.fn();
      const { result, rerender } = renderHook(
        ({ sessionId }: { sessionId: string | null }) =>
          useChatStream({ sessionId, onLiveTurnForSession }),
        { initialProps: { sessionId: null as string | null } }
      );
      await act(async () => {});

      await act(async () => {
        result.current.startStream({
          provider: "openai",
          prompt: "a",
          model: "gpt-4o",
          sessionId: "sa",
        });
        result.current.startStream({
          provider: "openai",
          prompt: "b",
          model: "gpt-4o",
          sessionId: "sb",
        });
      });
      const [runA, runB] = streamedRunIds();

      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId: runA,
          token: "a1",
        });
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId: runB,
          token: "b1",
        });
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId: runA,
          token: "-a2",
        });
      });

      // No session focused: each run accumulates under its own session's
      // stash — they do not interleave into one shared foreground buffer.
      expect(result.current.currentTurn).toBeNull();
      expect(turnText(lastLiveTurn(onLiveTurnForSession, "sa"))).toBe("a1-a2");
      expect(turnText(lastLiveTurn(onLiveTurnForSession, "sb"))).toBe("b1");

      // When focus lands on sa, its stash restores; sb stays stashed.
      await act(async () => {
        rerender({ sessionId: "sa" });
      });

      expect(turnText(result.current.currentTurn)).toBe("a1-a2");
      expect(turnText(lastLiveTurn(onLiveTurnForSession, "sb"))).toBe("b1");
    });

    it("runId-less llm-done attributes to the single active run's session even when another is focused", async () => {
      const onMessagesForSession = vi.fn();
      const { result } = renderChatStream({
        sessionId: "s-2",
        onMessagesForSession,
      });
      await act(async () => {});

      await act(async () => {
        result.current.startStream({
          provider: "openai",
          prompt: "hi",
          model: "gpt-4o",
          sessionId: "s-1",
        });
      });
      const runId = latestCompleteStreamingRunId();

      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId,
          token: "partial ",
        });
      });

      // A completion carrying no runId resolves to the sole in-flight session
      // instead of wiping the focused session's transcript.
      await act(async () => {
        triggerEvent("llm-done", { response: "single run reply" });
      });

      expect(onMessagesForSession).toHaveBeenCalledWith("s-1", expect.any(Function));
      expect(onMessagesForSession).not.toHaveBeenCalledWith("s-2", expect.any(Function));
      expect(onMessagesForSession).not.toHaveBeenCalledWith(null, expect.any(Function));
      // The finalized message includes the run's stashed turn blocks.
      const updater = onMessagesForSession.mock.calls.find(([sid]) => sid === "s-1")![1] as (
        prev: Message[]
      ) => Message[];
      const message = updater([])[0];
      expect(message.content).toBe("single run reply");
      expect(turnText(message)).toBe("partial ");
    });
  });
});
