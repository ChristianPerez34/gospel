import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useSessionManager } from "./useSessionManager";
import type { Message, ModelOption, Session } from "../types";

type ListenerCallback = (event: { payload: unknown }) => void;

interface CapturedListeners {
  [eventName: string]: ListenerCallback[];
}

const SAMPLE_MODELS: ModelOption[] = [
  { id: "openai::gpt-4o", name: "gpt-4o", provider: "openai", configured: true },
  { id: "anthropic::claude-3-5-sonnet", name: "claude-3-5-sonnet", provider: "anthropic", configured: true },
];

let capturedListeners: CapturedListeners = {};

function triggerEvent<T>(eventName: string, payload: T) {
  const listeners = capturedListeners[eventName] ?? [];
  for (const listener of listeners) {
    listener({ payload });
  }
}

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "s-existing",
    title: "Existing session",
    provider: "openai",
    model: "gpt-4o",
    timestamp: new Date("2024-01-01T00:00:00Z"),
    messages: [
      {
        id: "m-existing-1",
        role: "user",
        content: "old prompt",
        timestamp: new Date("2024-01-01T00:00:00Z"),
      },
    ],
    status: "idle",
    ...overrides,
  };
}

describe("useSessionManager", () => {
  beforeEach(() => {
    capturedListeners = {};
    vi.mocked(listen).mockImplementation(async (eventName, callback) => {
      if (!capturedListeners[eventName]) capturedListeners[eventName] = [];
      capturedListeners[eventName].push(callback as ListenerCallback);
      return () => {};
    });
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_sessions") return [];
      return undefined;
    });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  describe("session lifecycle", () => {
    it("creates a new session, sets activeSessionId, and adds the user message when sending without an active session", async () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(result.current.activeSessionId).not.toBeNull();
      expect(result.current.sessions).toHaveLength(1);
      const created = result.current.sessions[0]!;
      expect(created.title).toBe("hello");
      expect(created.provider).toBe("openai");
      expect(created.model).toBe("gpt-4o");
      expect(created.status).toBe("active");

      const userMessage = result.current.messages.find((m) => m.role === "user");
      expect(userMessage?.content).toBe("hello");
    });

    it("uses the first 50 chars of the message as the session title and truncates with ellipsis", async () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      const longPrompt = "x".repeat(80);
      await act(async () => {
        await result.current.handleSend(longPrompt);
      });

      const created = result.current.sessions[0]!;
      expect(created.title.length).toBeLessThanOrEqual(53);
      expect(created.title.endsWith("...")).toBe(true);
    });

    it("does not create a new session when one is already active and reuses the existing one", async () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      await act(async () => {
        await result.current.handleSend("first");
      });

      const firstSessionId = result.current.activeSessionId;
      expect(firstSessionId).not.toBeNull();

      await act(async () => {
        await result.current.handleSend("second");
      });

      expect(result.current.sessions).toHaveLength(1);
      expect(result.current.activeSessionId).toBe(firstSessionId);
      const userMessages = result.current.messages.filter((m) => m.role === "user");
      expect(userMessages).toHaveLength(2);
      expect(userMessages.map((m) => m.content)).toEqual(["first", "second"]);
    });

    it("handleSessionSelect switches activeSessionId and loads that session's messages", async () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      const target = makeSession({
        id: "s-target",
        title: "Target session",
        messages: [
          {
            id: "m-target-1",
            role: "agent",
            content: "prior agent reply",
            timestamp: new Date("2024-01-02T00:00:00Z"),
          },
        ],
      });

      act(() => {
        result.current.handleSessionSelect(target);
      });

      expect(result.current.activeSessionId).toBe("s-target");
      expect(result.current.messages).toEqual(target.messages);
    });

    it("handleSessionSelect hydrates a backend-created session via get_session so the next turn continues prior history", async () => {
      vi.mocked(invoke).mockImplementation(async (cmd: string) => {
        if (cmd === "list_sessions") return [];
        if (cmd === "get_session") {
          return {
            id: "s-restored",
            display_transcript: JSON.stringify([
              { role: "user", content: "earlier prompt" },
              { role: "agent", content: "earlier reply" },
            ]),
          };
        }
        return undefined;
      });

      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      const restored = makeSession({
        id: "s-restored",
        title: "Restored",
        messages: [],
        backendCreated: true,
      });

      await act(async () => {
        result.current.handleSessionSelect(restored);
      });

      expect(invoke).toHaveBeenCalledWith("get_session", {
        sessionId: "s-restored",
      });
      expect(result.current.activeSessionId).toBe("s-restored");
      expect(result.current.messages.map((m) => m.content)).toEqual([
        "earlier prompt",
        "earlier reply",
      ]);
    });

    it("handleNewSession resets messages to empty and activeSessionId to null", async () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(result.current.activeSessionId).not.toBeNull();
      expect(result.current.messages.length).toBeGreaterThan(0);

      act(() => {
        result.current.handleNewSession();
      });

      expect(result.current.activeSessionId).toBeNull();
      expect(result.current.messages).toEqual([]);
    });
  });

  describe("message orchestration", () => {
    it("invokes startStream with the correct provider, model, prompt, and sessionId when a valid model is selected", async () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      await act(async () => {
        await result.current.handleSend("hello world");
      });

      expect(invoke).toHaveBeenCalledWith(
        "complete_streaming",
        expect.objectContaining({
          provider: "openai",
          model: "gpt-4o",
          prompt: "hello world",
          sessionId: result.current.activeSessionId,
        }),
      );
    });

    it("invokes onError and does not call startStream when no model is selected", async () => {
      const onError = vi.fn();
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: null,
          onError,
        }),
      );

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(onError).toHaveBeenCalledTimes(1);
      expect(onError.mock.calls[0]![0]).toMatch(/select an available model/i);
      expect(invoke).not.toHaveBeenCalledWith(
        "complete_streaming",
        expect.anything(),
      );
    });

    it("invokes onError and does not call startStream when the selected model is not in the models list", async () => {
      const onError = vi.fn();
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-99-not-listed" },
          onError,
        }),
      );

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(onError).toHaveBeenCalledTimes(1);
      expect(invoke).not.toHaveBeenCalledWith(
        "complete_streaming",
        expect.anything(),
      );
    });

    it("invokes onError and sets status to error when startStream throws", async () => {
      vi.mocked(invoke).mockImplementation(async (cmd: string) => {
        if (cmd === "list_sessions") return [];
        if (cmd === "complete_streaming") throw new Error("network down");
        return undefined;
      });

      const onError = vi.fn();
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
          onError,
        }),
      );

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(onError).toHaveBeenCalled();
      expect(result.current.status).toBe("error");
    });
  });

  describe("status derivation", () => {
    it("initial status is 'idle' when no models are available", () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: [],
          selectedModel: null,
        }),
      );

      expect(result.current.status).toBe("idle");
    });

    it("initial status is 'connected' when at least one model is available", () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      expect(result.current.status).toBe("connected");
    });

    it("transitions to 'thinking' when handleSend is called", async () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      expect(result.current.status).toBe("connected");

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(result.current.status).toBe("thinking");
    });

    it("transitions to 'connected' when the onStatusChange callback receives 'connected' from useChatStream", async () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(result.current.status).toBe("thinking");

      act(() => {
        triggerEvent<string>("llm-done", "the assistant's final reply");
      });

      expect(result.current.status).toBe("connected");
    });

    it("transitions to 'acting' when the onStatusChange callback receives 'acting' from useChatStream", async () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      await act(async () => {
        await result.current.handleSend("hello");
      });

      act(() => {
        triggerEvent<{ id: string; name: string; arguments?: unknown }>(
          "llm-tool-call",
          { id: "tool-1", name: "read_file", arguments: { path: "x" } },
        );
      });

      expect(result.current.status).toBe("acting");
    });

    it("transitions to 'error' when the onStatusChange callback receives 'error' from useChatStream", async () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      await act(async () => {
        await result.current.handleSend("hello");
      });

      act(() => {
        triggerEvent<{ code: string; message: string }>("llm-error", {
          code: "BOOM",
          message: "boom",
        });
      });

      expect(result.current.status).toBe("error");
    });

    it("appends the streamed assistant message to messages when llm-done fires", async () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      await act(async () => {
        await result.current.handleSend("hi");
      });

      act(() => {
        triggerEvent<string>("llm-done", "hello back");
      });

      const agentMessages = result.current.messages.filter((m) => m.role === "agent");
      expect(agentMessages).toHaveLength(1);
      expect(agentMessages[0]!.content).toBe("hello back");
    });
  });

  describe("session message sync", () => {
    it("persists streamed assistant messages into the active session after llm-done", async () => {
      const { result } = renderHook(() =>
        useSessionManager({
          models: SAMPLE_MODELS,
          selectedModel: { provider: "openai", model: "gpt-4o" },
        }),
      );

      await act(async () => {
        await result.current.handleSend("hi");
      });

      const created = result.current.sessions[0]!;
      expect(created.messages.map((m: Message) => m.role)).toEqual(["user"]);

      act(() => {
        triggerEvent<string>("llm-done", "hello back");
      });

      const refreshed = result.current.sessions[0]!;
      expect(refreshed.messages.map((m: Message) => m.role)).toEqual(["user", "agent"]);
      expect(refreshed.messages[1]!.content).toBe("hello back");
    });
  });
});
