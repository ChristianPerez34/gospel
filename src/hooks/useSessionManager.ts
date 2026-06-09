import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AgentStatus, Message, ModelOption, Session, ToolCallActivity } from "../types";
import type { SelectedModel } from "./useModelAvailability";
import { useChatStream } from "./useChatStream";

export interface SessionManagerStreamOptions {
  provider: string;
  prompt: string;
  model: string;
  sessionId: string | null;
  invokedSkill?: { name: string; args?: string } | null;
}

export interface SessionManagerErrorAction {
  label: string;
  onClick: () => void;
}

export interface UseSessionManagerParams {
  models: ModelOption[];
  selectedModel: SelectedModel | null;
  activeWorkspaceId?: string;
  onError?: (message: string, action?: SessionManagerErrorAction) => void;
  onSuccess?: (message: string) => void;
  onOpenSettings?: () => void;
}

export interface UseSessionManagerResult {
  sessions: Session[];
  activeSessionId: string | null;
  messages: Message[];
  status: AgentStatus;
  streamingContent: string;
  toolActivities: ToolCallActivity[];
  isStreaming: boolean;
  isThinking: boolean;
  handleSend: (message: string, invokedSkill?: { name: string; args?: string }) => Promise<void>;
  handleSessionSelect: (session: Session) => void;
  handleNewSession: () => void;
}

export function useSessionManager({
  models,
  selectedModel,
  activeWorkspaceId,
  onError,
  onSuccess,
  onOpenSettings,
}: UseSessionManagerParams): UseSessionManagerResult {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [status, setStatus] = useState<AgentStatus>("idle");
  const statusRef = useRef(status);
  statusRef.current = status;

  // Load persisted sessions from backend on mount
  useEffect(() => {
    invoke<Array<{ id: string; title: string; provider: string; model: string; status: string; updated_at: string }>>("list_sessions")
      .then((backendSessions) => {
        const loaded: Session[] = backendSessions.map((s) => ({
          id: s.id,
          title: s.title,
          provider: s.provider,
          model: s.model,
          timestamp: new Date(s.updated_at),
          messages: [],
          status: s.status === "active" ? "idle" : "error",
          backendCreated: true,
        }));
        setSessions(loaded);
      })
      .catch((e) => {
        console.warn("Failed to load sessions from backend:", e);
      });
  }, []);

  // Clear active session on workspace change
  const prevWorkspaceRef = useRef(activeWorkspaceId);
  useEffect(() => {
    if (prevWorkspaceRef.current !== activeWorkspaceId) {
      prevWorkspaceRef.current = activeWorkspaceId;
      if (statusRef.current === "thinking" || statusRef.current === "acting") return;
      setActiveSessionId(null);
      setMessages([]);
      // Reload sessions for the new workspace
      invoke<Array<{ id: string; title: string; provider: string; model: string; status: string; updated_at: string }>>("list_sessions")
        .then((backendSessions) => {
          const loaded: Session[] = backendSessions.map((s) => ({
            id: s.id,
            title: s.title,
            provider: s.provider,
            model: s.model,
            timestamp: new Date(s.updated_at),
            messages: [],
            status: s.status === "active" ? "idle" : "error",
            backendCreated: true,
          }));
          setSessions(loaded);
        })
        .catch((e) => {
          console.warn("Failed to reload sessions after workspace change:", e);
        });
    }
  }, [activeWorkspaceId]);

  const {
    streamingContent,
    toolActivities,
    startStream,
    resetStream,
  } = useChatStream({
    onMessages: setMessages,
    onStatusChange: setStatus,
    onErrorToast: onError,
    onSuccessToast: onSuccess,
    onOpenSettings,
  });

  const isStreaming = status === "thinking" || status === "acting";
  const isThinking = isStreaming && streamingContent === "";

  useEffect(() => {
    if (statusRef.current === "thinking" || statusRef.current === "acting") return;
    setStatus(models.length > 0 ? "connected" : "idle");
  }, [models.length]);

  useEffect(() => {
    if (!activeSessionId) return;
    setSessions((prev) =>
      prev.map((session) =>
        session.id === activeSessionId
          ? {
              ...session,
              messages,
              timestamp: messages[messages.length - 1]?.timestamp ?? session.timestamp,
            }
          : session
      )
    );
  }, [activeSessionId, messages]);

  const handleSend = useCallback(
    async (message: string, invokedSkill?: { name: string; args?: string }) => {
      if (
        !selectedModel ||
        !models.some(
          (m) =>
            m.name === selectedModel.model &&
            m.provider.toLowerCase() === selectedModel.provider.toLowerCase(),
        )
      ) {
        onError?.("Select an available model before sending.", {
          label: "Open Settings",
          onClick: () => onOpenSettings?.(),
        });
        return;
      }

      const userMsg: Message = {
        id: `m-${Date.now()}-user`,
        role: "user",
        content: invokedSkill
          ? `/${invokedSkill.name}${invokedSkill.args ? " " + invokedSkill.args : ""}`
          : message,
        timestamp: new Date(),
      };
      setMessages((prev) => [...prev, userMsg]);
      setStatus("thinking");
      resetStream();

      let effectiveSessionId = activeSessionId;
      if (!activeSessionId) {
        const title =
          userMsg.content.slice(0, 50) + (userMsg.content.length > 50 ? "..." : "");

        // Try backend session creation first
        let backendSession: { id: string } | null = null;
        try {
          backendSession = await invoke<{ id: string }>("create_session", {
            title,
            provider: selectedModel.provider,
            model: selectedModel.model,
          });
        } catch (e) {
          console.warn("Backend session creation failed, using local session:", e);
        }

        const sessionId = backendSession?.id ?? `s-${Date.now()}`;
        const newSession: Session = {
          id: sessionId,
          title,
          provider: selectedModel.provider,
          model: selectedModel.model,
          timestamp: new Date(),
          messages: [userMsg],
          status: "active",
          backendCreated: !!backendSession,
        };
        setSessions((prev) => [newSession, ...prev]);
        setActiveSessionId(sessionId);
        effectiveSessionId = sessionId;
      }

      try {
        await startStream({
          provider: selectedModel.provider,
          prompt: message,
          model: selectedModel.model,
          sessionId: effectiveSessionId,
          invokedSkill: invokedSkill ?? null,
        });
      } catch (e) {
        setStatus("error");
        resetStream();
        onError?.(`Failed to send: ${e}`, {
          label: "Open Settings",
          onClick: () => onOpenSettings?.(),
        });
      }
    },
    [activeSessionId, models, selectedModel, onError, onOpenSettings, resetStream, startStream],
  );

  const handleSessionSelect = useCallback(
    async (session: Session) => {
      if (statusRef.current === "thinking" || statusRef.current === "acting") return;

      // If session was backend-created and has no local messages, load from backend
      if (session.backendCreated && session.messages.length === 0) {
        try {
          const detail = await invoke<{
            id: string;
            display_transcript: string;
          }>("get_session", { sessionId: session.id });

          const transcript = JSON.parse(detail.display_transcript) as Array<{
            role: string;
            content: string;
          }>;
          const loadedMessages: Message[] = transcript.map((msg, i) => ({
            id: `m-${session.id}-${i}-${msg.role}`,
            role: msg.role === "user" ? ("user" as const) : ("agent" as const),
            content: msg.content,
            timestamp: new Date(session.timestamp),
          }));

          setActiveSessionId(session.id);
          setMessages(loadedMessages);
          setSessions((prev) =>
            prev.map((s) =>
              s.id === session.id ? { ...s, messages: loadedMessages } : s,
            ),
          );
          return;
        } catch (e) {
          console.warn("Failed to load session detail from backend:", e);
        }
      }

      setActiveSessionId(session.id);
      setMessages(session.messages);
    },
    [],
  );

  const handleNewSession = useCallback(() => {
    setActiveSessionId(null);
    setMessages([]);
    resetStream();
  }, [resetStream]);

  return {
    sessions,
    activeSessionId,
    messages,
    status,
    streamingContent,
    toolActivities,
    isStreaming,
    isThinking,
    handleSend,
    handleSessionSelect,
    handleNewSession,
  };
}
