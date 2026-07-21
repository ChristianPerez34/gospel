import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApprovalRequest, Message } from "../types";
import { useChatStream } from "./useChatStream";

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
  onStatusChange?: (status: string) => void;
  onErrorToast?: (message: string, action?: { label: string; onClick: () => void }) => void;
  onSuccessToast?: (message: string) => void;
  onResolveApproval?: (id: string, decision: string) => Promise<unknown>;
  sessionId?: string | null;
}

function renderChatStream(options: RenderChatStreamOptions = {}) {
  return renderHook(() => useChatStream(options));
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

      expect(
        result.current.currentTurn!.blocks.some((b) => b.kind === "reasoning")
      ).toBe(true);

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

    it("late llm-token with a non-matching run_id is ignored (per-run isolation)", async () => {
      const { result } = renderChatStream();
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
      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId: "run-active",
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
      const { result } = renderChatStream({
        onMessages,
        onStatusChange,
        sessionId: "s-cancel",
      });
      await act(async () => {});

      await act(async () => {
        await result.current.startStream({
          provider: "openai",
          prompt: "hi",
          model: "gpt-4o",
          sessionId: "s-cancel",
        });
      });
      await act(async () => {
        triggerEvent<{ runId: string; token: string }>("llm-token", {
          runId: "run-active",
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
          runId: "run-active",
          token: "after-cancel",
        });
      });
      expect(result.current.currentTurn).toBeNull();
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
});