import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { useState } from "react";
import { renderHook, act } from "@testing-library/react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useSessionManager, type UseSessionManagerParams } from "./useSessionManager";
import type { Message, ModelOption, Session } from "../types";

type ListenerCallback = (event: { payload: unknown }) => void;

interface CapturedListeners {
  [eventName: string]: ListenerCallback[];
}

const SAMPLE_MODELS: ModelOption[] = [
  { id: "openai::gpt-4o", name: "gpt-4o", provider: "openai", model: "gpt-4o", configured: true },
  {
    id: "anthropic::claude-3-5-sonnet",
    name: "claude-3-5-sonnet",
    provider: "anthropic",
    model: "claude-3-5-sonnet",
    configured: true,
  },
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

type RenderSessionManagerOptions = Partial<
  Omit<UseSessionManagerParams, "sessions" | "onSessionsChange">
> & {
  initialSessions?: Session[];
};

function renderSessionManager(options: RenderSessionManagerOptions = {}) {
  const {
    initialSessions = [],
    models = SAMPLE_MODELS,
    selectedModel = { provider: "openai", model: "gpt-4o" },
    ...rest
  } = options;

  return renderHook(() => {
    const [sessions, setSessions] = useState<Session[]>(initialSessions);
    return useSessionManager({
      models,
      selectedModel,
      sessions,
      onSessionsChange: setSessions,
      ...rest,
    });
  });
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
      const { result } = renderSessionManager();

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

    it("passes the active workspace id when creating a backend session", async () => {
      vi.mocked(invoke).mockImplementation(async (cmd: string) => {
        if (cmd === "create_session") return { id: "backend-session" };
        return undefined;
      });

      const { result } = renderSessionManager({
        activeWorkspaceId: "ws-active",
      });

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(invoke).toHaveBeenCalledWith("create_session", {
        title: "hello",
        provider: "openai",
        model: "gpt-4o",
        variant: null,
        workspaceId: "ws-active",
        mode: "Build",
      });
      expect(result.current.activeSessionId).toBe("backend-session");
      expect(result.current.sessions[0]).toMatchObject({
        id: "backend-session",
        backendCreated: true,
        workspaceId: "ws-active",
        mode: "Build",
      });
    });

    it("uses the selected draft session mode when creating a backend session", async () => {
      vi.mocked(invoke).mockImplementation(async (cmd: string) => {
        if (cmd === "create_session") return { id: "backend-session" };
        return undefined;
      });

      const { result } = renderSessionManager({
        activeWorkspaceId: "ws-active",
      });

      await act(async () => {
        await result.current.handleSessionModeChange("ReadOnly");
      });
      await act(async () => {
        await result.current.handleSend("inspect only");
      });

      expect(invoke).toHaveBeenCalledWith("create_session", {
        title: "inspect only",
        provider: "openai",
        model: "gpt-4o",
        variant: null,
        workspaceId: "ws-active",
        mode: "ReadOnly",
      });
      expect(result.current.sessions[0]).toMatchObject({
        id: "backend-session",
        mode: "ReadOnly",
      });
    });

    it("persists mode changes for an active backend session", async () => {
      vi.mocked(invoke).mockImplementation(async (cmd: string) => {
        if (cmd === "update_session_mode") return undefined;
        return [];
      });

      const backendSession = makeSession({
        id: "s-backend",
        backendCreated: true,
        mode: "Build",
      });
      const { result } = renderSessionManager({
        initialSessions: [backendSession],
      });

      await act(async () => {
        await result.current.handleSessionSelect(backendSession);
      });
      await act(async () => {
        await result.current.handleSessionModeChange("ReadOnly");
      });

      expect(invoke).toHaveBeenCalledWith("update_session_mode", {
        sessionId: "s-backend",
        mode: "ReadOnly",
      });
      expect(result.current.activeSessionMode).toBe("ReadOnly");
      expect(result.current.sessions[0]?.mode).toBe("ReadOnly");
    });

    it("uses the first 50 chars of the message as the session title and truncates with ellipsis", async () => {
      const { result } = renderSessionManager();

      const longPrompt = "x".repeat(80);
      await act(async () => {
        await result.current.handleSend(longPrompt);
      });

      const created = result.current.sessions[0]!;
      expect(created.title.length).toBeLessThanOrEqual(53);
      expect(created.title.endsWith("...")).toBe(true);
    });

    it("does not create a new session when one is already active and reuses the existing one", async () => {
      const { result } = renderSessionManager();

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
      const { result } = renderSessionManager();

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

    it("switches workspace before loading a backend session from another workspace", async () => {
      const onSwitchWorkspace = vi.fn().mockResolvedValue(true);
      const onError = vi.fn();

      vi.mocked(invoke).mockImplementation(async (...callArgs: Parameters<typeof invoke>) => {
        const [cmd] = callArgs;
        if (cmd === "get_session") {
          return {
            id: "s-target",
            display_transcript: JSON.stringify([
              { role: "user", content: "older in other workspace" },
              {
                role: "agent",
                content: "reply",
                blocks: [
                  { kind: "text", id: "text-0", text: "reply" },
                  {
                    kind: "tool",
                    id: "tool-1",
                    name: "read_file",
                    arguments: { path: "src/lib.rs" },
                    result: "file contents",
                    status: "completed",
                  },
                ],
              },
            ]),
          };
        }

        return undefined;
      });

      const { result } = renderSessionManager({
        activeWorkspaceId: "ws-active",
        onSwitchWorkspace,
        onError,
      });

      const target = makeSession({
        id: "s-target",
        title: "Target session",
        workspaceId: "ws-other",
        messages: [],
        backendCreated: true,
      });

      await act(async () => {
        await result.current.handleSessionSelect(target);
      });

      expect(onSwitchWorkspace).toHaveBeenCalledWith("ws-other");
      expect(invoke).toHaveBeenCalledWith("get_session", { sessionId: "s-target" });
      expect(result.current.activeSessionId).toBe("s-target");
      expect(result.current.messages.map((m) => m.content)).toEqual([
        "older in other workspace",
        "reply",
      ]);
      expect(result.current.messages[1]?.blocks).toMatchObject([
        { kind: "text", text: "reply" },
        {
          kind: "tool",
          id: "tool-1",
          name: "read_file",
          status: "completed",
        },
      ]);
      expect(onError).not.toHaveBeenCalled();
    });

    it("reports an error when workspace switch fails", async () => {
      const onSwitchWorkspace = vi.fn().mockResolvedValue(false);
      const onError = vi.fn();

      const { result } = renderSessionManager({
        activeWorkspaceId: "ws-active",
        onSwitchWorkspace,
        onError,
      });

      const target = makeSession({
        id: "s-other",
        title: "Blocked",
        workspaceId: "ws-other",
        messages: [],
        backendCreated: true,
      });

      await act(async () => {
        await result.current.handleSessionSelect(target);
      });

      expect(onSwitchWorkspace).toHaveBeenCalledWith("ws-other");
      expect(onError).toHaveBeenCalledWith("Unable to switch workspace for this session.");
      expect(result.current.activeSessionId).toBeNull();
      expect(result.current.messages).toEqual([]);
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

      const { result } = renderSessionManager();

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
      expect(result.current.messages[1]?.blocks).toEqual([
        { kind: "text", id: "text-0", text: "earlier reply" },
      ]);
    });

    it("handleSessionSelect replaces streamed block history from the previous session", async () => {
      const { result } = renderSessionManager();

      await act(async () => {
        await result.current.handleSend("hi");
      });
      act(() => {
        triggerEvent<{ id: string; name: string; arguments?: unknown }>("llm-tool-call", {
          id: "tool-1",
          name: "read_file",
        });
      });
      act(() => {
        triggerEvent<string>("llm-done", "done");
      });

      expect(result.current.messages.find((m) => m.role === "agent")?.blocks).toMatchObject([
        {
          kind: "tool",
          id: "tool-1",
          name: "read_file",
        },
      ]);

      const otherSession = makeSession({
        id: "s-other",
        title: "Other",
      });
      await act(async () => {
        await result.current.handleSessionSelect(otherSession);
      });

      expect(result.current.messages).toEqual(otherSession.messages);
    });

    it("handleNewSession resets messages to empty and activeSessionId to null", async () => {
      const { result } = renderSessionManager();

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
      const { result } = renderSessionManager();

      await act(async () => {
        await result.current.handleSend("hello world");
      });

      expect(invoke).toHaveBeenCalledWith(
        "complete_streaming",
        expect.objectContaining({
          provider: "openai",
          model: "gpt-4o",
          variant: null,
          prompt: "hello world",
          sessionId: result.current.activeSessionId,
        })
      );
    });

    it("sends the selected variant for same-slug model variants", async () => {
      const variantModels: ModelOption[] = [
        {
          id: "openai::gpt-5.2",
          name: "gpt-5.2",
          provider: "openai",
          model: "gpt-5.2",
          variant: null,
          configured: true,
          variants: [
            {
              id: "reasoning-high",
              name: "High reasoning",
              description: "More test-time reasoning",
            },
          ],
        },
      ];
      const { result } = renderSessionManager({
        models: variantModels,
        selectedModel: {
          provider: "openai",
          model: "gpt-5.2",
          variant: "reasoning-high",
        },
      });

      await act(async () => {
        await result.current.handleSend("think hard");
      });

      expect(invoke).toHaveBeenCalledWith(
        "complete_streaming",
        expect.objectContaining({
          provider: "openai",
          model: "gpt-5.2",
          variant: "reasoning-high",
          prompt: "think hard",
          sessionId: result.current.activeSessionId,
        })
      );
      expect(result.current.sessions[0]).toMatchObject({
        provider: "openai",
        model: "gpt-5.2",
        variant: "reasoning-high",
      });
    });

    it("clears a missing model variant warning from the active session", async () => {
      const onModelVariantFallback = vi.fn();
      const variantModels: ModelOption[] = [
        {
          id: "openai::gpt-5.2",
          name: "gpt-5.2",
          provider: "openai",
          model: "gpt-5.2",
          configured: true,
          variants: [
            {
              id: "reasoning-high",
              name: "High reasoning",
              description: "More test-time reasoning",
            },
          ],
        },
      ];
      const { result } = renderSessionManager({
        models: variantModels,
        selectedModel: {
          provider: "openai",
          model: "gpt-5.2",
          variant: "missing-variant",
        },
        onModelVariantFallback,
      });

      await act(async () => {
        await result.current.handleSend("think hard");
      });

      expect(result.current.sessions[0]).toMatchObject({
        provider: "openai",
        model: "gpt-5.2",
        variant: "missing-variant",
      });

      act(() => {
        triggerEvent("llm-model-variant-warning", {
          kind: "missing",
          provider: "openai",
          model: "gpt-5.2",
          variant: "missing-variant",
          message: "using Default",
        });
      });

      expect(onModelVariantFallback).toHaveBeenCalledWith({
        kind: "missing",
        provider: "openai",
        model: "gpt-5.2",
        variant: "missing-variant",
        message: "using Default",
      });
      expect(result.current.sessions[0]!.variant).toBeNull();
    });

    it("invokes onError and does not call startStream when no model is selected", async () => {
      const onError = vi.fn();
      const { result } = renderSessionManager({
        selectedModel: null,
        onError,
      });

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(onError).toHaveBeenCalledTimes(1);
      expect(onError.mock.calls[0]![0]).toMatch(/select an available model/i);
      expect(invoke).not.toHaveBeenCalledWith("complete_streaming", expect.anything());
    });

    it("invokes onError and does not call startStream when the selected model is not in the models list", async () => {
      const onError = vi.fn();
      const { result } = renderSessionManager({
        selectedModel: { provider: "openai", model: "gpt-99-not-listed" },
        onError,
      });

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(onError).toHaveBeenCalledTimes(1);
      expect(invoke).not.toHaveBeenCalledWith("complete_streaming", expect.anything());
    });

    it("invokes onError and sets status to error when startStream throws", async () => {
      vi.mocked(invoke).mockImplementation(async (cmd: string) => {
        if (cmd === "list_sessions") return [];
        if (cmd === "complete_streaming") throw new Error("network down");
        return undefined;
      });

      const onError = vi.fn();
      const { result } = renderSessionManager({
        onError,
      });

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(onError).toHaveBeenCalled();
      expect(result.current.status).toBe("error");
    });
  });

  describe("status derivation", () => {
    it("initial status is 'idle' when no models are available", () => {
      const { result } = renderSessionManager({
        models: [],
        selectedModel: null,
      });

      expect(result.current.status).toBe("idle");
    });

    it("initial status is 'connected' when at least one model is available", () => {
      const { result } = renderSessionManager();

      expect(result.current.status).toBe("connected");
    });

    it("transitions to 'thinking' when handleSend is called", async () => {
      const { result } = renderSessionManager();

      expect(result.current.status).toBe("connected");

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(result.current.status).toBe("thinking");
    });

    it("transitions to 'connected' when the onStatusChange callback receives 'connected' from useChatStream", async () => {
      const { result } = renderSessionManager();

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(result.current.status).toBe("thinking");

      act(() => {
        triggerEvent("llm-done", {
          response: "the assistant's final reply",
        });
      });

      expect(result.current.status).toBe("connected");
    });

    it("transitions to 'acting' when the onStatusChange callback receives 'acting' from useChatStream", async () => {
      const { result } = renderSessionManager();

      await act(async () => {
        await result.current.handleSend("hello");
      });

      act(() => {
        triggerEvent<{ id: string; name: string; arguments?: unknown }>("llm-tool-call", {
          id: "tool-1",
          name: "read_file",
          arguments: { path: "x" },
        });
      });

      expect(result.current.status).toBe("acting");
    });

    it("transitions to 'error' when the onStatusChange callback receives 'error' from useChatStream", async () => {
      const { result } = renderSessionManager();

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
      const { result } = renderSessionManager();

      await act(async () => {
        await result.current.handleSend("hi");
      });

      act(() => {
        triggerEvent("llm-done", {
          response: "hello back",
          prompt_tokens: 42,
          response_tokens: 7,
          tool_calls: 2,
        });
      });

      const agentMessages = result.current.messages.filter((m) => m.role === "agent");
      expect(agentMessages).toHaveLength(1);
      expect(agentMessages[0]!.content).toBe("hello back");
    });

    it("creates a stable currentTurn from the first streamed token", async () => {
      const { result } = renderSessionManager();

      await act(async () => {
        await result.current.handleSend("hi");
      });

      expect(result.current.currentTurn).toBeNull();

      act(() => {
        triggerEvent<string>("llm-token", "hello");
      });

      const turnId = result.current.currentTurn?.id;
      expect(turnId).toMatch(/^turn-/);
      expect(result.current.currentTurn?.blocks).toEqual([
        { kind: "text", id: "text-0", text: "hello" },
      ]);

      act(() => {
        triggerEvent<string>("llm-token", " back");
      });

      expect(result.current.currentTurn?.id).toBe(turnId);
      expect(result.current.currentTurn?.blocks).toEqual([
        { kind: "text", id: "text-0", text: "hello back" },
      ]);
    });

    it("groups live text and tool blocks inside currentTurn and finalizes them into the assistant message", async () => {
      const { result } = renderSessionManager();

      await act(async () => {
        await result.current.handleSend("hi");
      });

      act(() => {
        triggerEvent<string>("llm-token", "Before ");
      });

      act(() => {
        triggerEvent<{ id: string; name: string; arguments?: unknown }>("llm-tool-call", {
          id: "tool-1",
          name: "read_file",
          arguments: { path: "src/lib.rs" },
        });
      });

      const turnId = result.current.currentTurn?.id;
      expect(result.current.currentTurn?.blocks).toMatchObject([
        { kind: "text", text: "Before " },
        {
          kind: "tool",
          id: "tool-1",
          name: "read_file",
          status: "calling",
        },
      ]);

      act(() => {
        triggerEvent<{ id: string; name: string; result: string }>("llm-tool-result", {
          id: "tool-1",
          name: "read_file",
          result: "file contents",
        });
      });

      expect(result.current.currentTurn?.id).toBe(turnId);
      expect(result.current.currentTurn?.blocks[1]).toMatchObject({
        kind: "tool",
        status: "completed",
        result: "file contents",
      });

      act(() => {
        triggerEvent<string>("llm-token", "After.");
      });

      act(() => {
        triggerEvent<string>("llm-done", "Before After.");
      });

      const agentMessages = result.current.messages.filter((m) => m.role === "agent");
      expect(result.current.currentTurn).toBeNull();
      expect(agentMessages).toHaveLength(1);
      expect(agentMessages[0]).toMatchObject({
        id: turnId,
        content: "Before After.",
      });
      expect(agentMessages[0]).not.toHaveProperty("actionCards");
      expect(agentMessages[0]?.blocks).toMatchObject([
        { kind: "text", text: "Before " },
        {
          kind: "tool",
          id: "tool-1",
          name: "read_file",
          status: "completed",
          result: "file contents",
        },
        { kind: "text", text: "After." },
      ]);
    });

    it("warns and appends a completed tool block when an llm-tool-result arrives with no matching call", async () => {
      const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
      const { result } = renderSessionManager();

      await act(async () => {
        await result.current.handleSend("hi");
      });

      act(() => {
        triggerEvent<{ id: string; name: string; result: string }>("llm-tool-result", {
          id: "tool-orphan",
          name: "read_file",
          result: "orphan contents",
        });
      });

      expect(result.current.currentTurn?.blocks).toMatchObject([
        {
          kind: "tool",
          id: "tool-orphan",
          name: "read_file",
          status: "completed",
          result: "orphan contents",
        },
      ]);
      expect(warnSpy).toHaveBeenCalledWith(
        expect.stringContaining("llm-tool-result"),
        expect.objectContaining({ name: "read_file" })
      );
      warnSpy.mockRestore();
    });

    it("clears the live turn on errors while preserving tool blocks on the error message", async () => {
      const { result } = renderSessionManager();

      await act(async () => {
        await result.current.handleSend("hi");
      });

      act(() => {
        triggerEvent<{ id: string; name: string; arguments?: unknown }>("llm-tool-call", {
          id: "tool-1",
          name: "read_file",
          arguments: { path: "src/lib.rs" },
        });
      });

      const turnId = result.current.currentTurn?.id;

      act(() => {
        triggerEvent<{ code: string; message: string }>("llm-error", {
          code: "BOOM",
          message: "boom",
        });
      });

      const agentMessages = result.current.messages.filter((m) => m.role === "agent");
      expect(result.current.status).toBe("error");
      expect(result.current.currentTurn).toBeNull();
      expect(agentMessages[0]).toMatchObject({
        id: turnId,
        content: "",
        error: "boom",
      });
      expect(agentMessages[0]).not.toHaveProperty("actionCards");
      expect(agentMessages[0]?.blocks).toMatchObject([
        {
          kind: "tool",
          id: "tool-1",
          name: "read_file",
          status: "calling",
        },
      ]);
    });
  });

  describe("session message sync", () => {
    it("persists streamed assistant messages into the active session after llm-done", async () => {
      const { result } = renderSessionManager();

      await act(async () => {
        await result.current.handleSend("hi");
      });

      const created = result.current.sessions[0]!;
      expect(created.messages.map((m: Message) => m.role)).toEqual(["user"]);

      act(() => {
        triggerEvent("llm-done", {
          response: "hello back",
        });
      });

      const refreshed = result.current.sessions[0]!;
      expect(refreshed.messages.map((m: Message) => m.role)).toEqual(["user", "agent"]);
      expect(refreshed.messages[1]!.content).toBe("hello back");
    });

    it("accepts llm-done object payload while preserving action content", async () => {
      const { result } = renderSessionManager();

      await act(async () => {
        await result.current.handleSend("hi");
      });

      act(() => {
        triggerEvent("llm-done", {
          response: "metadata-aware reply",
          prompt_tokens: 12,
          response_tokens: 3,
          tool_calls: 1,
        });
      });

      const agentMessages = result.current.messages.filter((m) => m.role === "agent");
      expect(agentMessages).toHaveLength(1);
      expect(agentMessages[0]!.content).toBe("metadata-aware reply");
      expect(result.current.status).toBe("connected");
    });
  });
});
