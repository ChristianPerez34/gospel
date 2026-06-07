import { useCallback, useEffect, useRef, useState } from "react";
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
  handleSend: (message: string, invokedSkill?: { name: string; args?: string }) => Promise<void>;
  handleSessionSelect: (session: Session) => void;
  handleNewSession: () => void;
}

export function useSessionManager({
  models,
  selectedModel,
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
        const newSession: Session = {
          id: `s-${Date.now()}`,
          title: userMsg.content.slice(0, 50) + (userMsg.content.length > 50 ? "..." : ""),
          provider: selectedModel.provider,
          model: selectedModel.model,
          timestamp: new Date(),
          messages: [userMsg],
          status: "active",
        };
        setSessions((prev) => [newSession, ...prev]);
        setActiveSessionId(newSession.id);
        effectiveSessionId = newSession.id;
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

  const handleSessionSelect = useCallback((session: Session) => {
    if (statusRef.current === "thinking" || statusRef.current === "acting") return;
    setActiveSessionId(session.id);
    setMessages(session.messages);
  }, []);

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
    handleSend,
    handleSessionSelect,
    handleNewSession,
  };
}
