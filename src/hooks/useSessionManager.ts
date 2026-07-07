import {
  type Dispatch,
  type SetStateAction,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { normalizeSessionMode } from "../types";
import type {
  AgentStatus,
  CurrentTurn,
  Message,
  ModelOption,
  Session,
  SessionMode,
  TurnBlock,
} from "../types";
import type { SelectedModel } from "./useModelAvailability";
import { useChatStream } from "./useChatStream";

export interface SessionManagerStreamOptions {
  provider: string;
  prompt: string;
  model: string;
  variant?: string | null;
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
  sessions: Session[];
  onSessionsChange: Dispatch<SetStateAction<Session[]>>;
  activeWorkspaceId?: string;
  onSwitchWorkspace?: (workspaceId: string) => Promise<boolean>;
  onError?: (message: string, action?: SessionManagerErrorAction) => void;
  onSuccess?: (message: string) => void;
  onOpenSettings?: () => void;
}

export interface UseSessionManagerResult {
  sessions: Session[];
  activeSessionId: string | null;
  messages: Message[];
  status: AgentStatus;
  currentTurn: CurrentTurn | null;
  isStreaming: boolean;
  isThinking: boolean;
  handleSend: (message: string, invokedSkill?: { name: string; args?: string }) => Promise<void>;
  handleSessionSelect: (session: Session) => Promise<void>;
  handleNewSession: () => void;
  activeSessionMode: SessionMode;
  handleSessionModeChange: (mode: SessionMode) => Promise<void>;
}

export function useSessionManager({
  models,
  selectedModel,
  sessions,
  onSessionsChange,
  activeWorkspaceId,
  onSwitchWorkspace,
  onError,
  onSuccess,
  onOpenSettings,
}: UseSessionManagerParams): UseSessionManagerResult {
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [status, setStatus] = useState<AgentStatus>("idle");
  const [draftSessionMode, setDraftSessionMode] = useState<SessionMode>("Build");
  const statusRef = useRef(status);
  statusRef.current = status;
  const latestSelectedSessionRef = useRef<string | null>(null);
  const skipNextWorkspaceResetRef = useRef<string | null>(null);

  const prevWorkspaceRef = useRef(activeWorkspaceId);
  useEffect(() => {
    if (statusRef.current === "thinking" || statusRef.current === "acting") return;

    const workspaceChanged = prevWorkspaceRef.current !== activeWorkspaceId;
    if (!workspaceChanged) return;

    prevWorkspaceRef.current = activeWorkspaceId;
    if (
      skipNextWorkspaceResetRef.current &&
      skipNextWorkspaceResetRef.current === activeWorkspaceId
    ) {
      skipNextWorkspaceResetRef.current = null;
      return;
    }

    skipNextWorkspaceResetRef.current = null;
    latestSelectedSessionRef.current = null;
    setActiveSessionId(null);
    setMessages([]);
    setDraftSessionMode("Build");
  }, [activeWorkspaceId, status]);

  const {
    currentTurn,
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
  const isThinking = status === "thinking";
  const activeSession = sessions.find((session) => session.id === activeSessionId);
  const activeSessionMode = activeSession
    ? normalizeSessionMode(activeSession.mode)
    : draftSessionMode;

  useEffect(() => {
    if (statusRef.current === "thinking" || statusRef.current === "acting") return;
    setStatus(models.length > 0 ? "connected" : "idle");
  }, [models.length]);

  useEffect(() => {
    if (!activeSessionId) return;
    onSessionsChange((prev) =>
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
  }, [activeSessionId, messages, onSessionsChange]);

  const handleSend = useCallback(
    async (message: string, invokedSkill?: { name: string; args?: string }) => {
      const selectedParentAvailable = selectedModel
        ? models.some(
            (m) =>
              m.model === selectedModel.model &&
              m.provider.toLowerCase() === selectedModel.provider.toLowerCase(),
          )
        : false;
      if (!selectedModel || !selectedParentAvailable) {
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
        const mode = draftSessionMode;

        // Try backend session creation first
        let backendSession: { id: string } | null = null;
        try {
          if (activeWorkspaceId) {
            backendSession = await invoke<{ id: string }>("create_session", {
              title,
              provider: selectedModel.provider,
              model: selectedModel.model,
              variant: selectedModel.variant ?? null,
              workspaceId: activeWorkspaceId,
              mode,
            });
          }
        } catch (e) {
          console.warn("Backend session creation failed, using local session:", e);
        }

        const sessionId = backendSession?.id ?? `s-${Date.now()}`;
        const newSession: Session = {
          id: sessionId,
          title,
          provider: selectedModel.provider,
          model: selectedModel.model,
          variant: selectedModel.variant ?? null,
          mode,
          timestamp: new Date(),
          messages: [userMsg],
          status: "active",
          backendCreated: !!backendSession,
          workspaceId: activeWorkspaceId,
        };
        onSessionsChange((prev) => [newSession, ...prev]);
        setActiveSessionId(sessionId);
        effectiveSessionId = sessionId;
      }

      try {
        await startStream({
          provider: selectedModel.provider,
          prompt: message,
          model: selectedModel.model,
          variant: selectedModel.variant ?? null,
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
    [
      activeSessionId,
      activeWorkspaceId,
      draftSessionMode,
      models,
      onError,
      onOpenSettings,
      onSessionsChange,
      resetStream,
      selectedModel,
      startStream,
    ],
  );

  const handleSessionSelect = useCallback(
    async (session: Session) => {
      if (statusRef.current === "thinking" || statusRef.current === "acting") return;
      const selectionId = session.id;
      latestSelectedSessionRef.current = selectionId;

      try {
        const selectedSession = session;

        if (
          onSwitchWorkspace &&
          selectedSession.workspaceId &&
          selectedSession.workspaceId !== activeWorkspaceId
        ) {
          skipNextWorkspaceResetRef.current = selectedSession.workspaceId;
          const switched = await onSwitchWorkspace(selectedSession.workspaceId);
          if (!switched) {
            skipNextWorkspaceResetRef.current = null;
            onError?.("Unable to switch workspace for this session.");
            return;
          }
        }

        // If session was backend-created and has no local messages, load from backend
        if (selectedSession.backendCreated && selectedSession.messages.length === 0) {
          try {
            const detail = await invoke<{
              id: string;
              mode?: string | null;
              display_transcript: string;
            }>("get_session", { sessionId: selectedSession.id });

            if (latestSelectedSessionRef.current !== selectionId) return;

            const transcript = JSON.parse(detail.display_transcript) as Array<{
              role: string;
              content: string;
              blocks?: TurnBlock[];
            }>;
            const loadedMessages: Message[] = transcript.map((msg, i) => {
              const role = msg.role === "user" ? ("user" as const) : ("agent" as const);
              // Legacy assistant entries without `blocks` synthesize a single
              // text block from `content` so the renderer has a uniform model.
              const blocks: TurnBlock[] | undefined =
                msg.blocks && msg.blocks.length > 0
                  ? msg.blocks
                  : role === "agent"
                    ? [{ kind: "text", id: `text-0`, text: msg.content }]
                    : undefined;
              return {
                id: `m-${selectedSession.id}-${i}-${msg.role}`,
                role,
                content: msg.content,
                timestamp: new Date(selectedSession.timestamp),
                blocks,
              };
            });

            setActiveSessionId(selectedSession.id);
            setMessages(loadedMessages);
            onSessionsChange((prev) => {
              const updated = {
                ...selectedSession,
                mode: normalizeSessionMode(detail.mode),
                messages: loadedMessages,
              };
              if (!prev.some((s) => s.id === selectedSession.id)) {
                return [updated, ...prev];
              }
              return prev.map((s) => (s.id === selectedSession.id ? updated : s));
            });
            return;
          } catch (e) {
            if (latestSelectedSessionRef.current !== selectionId) return;
            console.warn("Failed to load session detail from backend:", e);
          }
        }

        if (latestSelectedSessionRef.current !== selectionId) return;
        setActiveSessionId(selectedSession.id);
        setMessages(selectedSession.messages);
      } catch (e) {
        if (latestSelectedSessionRef.current !== selectionId) return;
        onError?.(`Unable to open session: ${e}`);
      }
    },
    [activeWorkspaceId, onError, onSessionsChange, onSwitchWorkspace],
  );

  const handleNewSession = useCallback(() => {
    latestSelectedSessionRef.current = null;
    setActiveSessionId(null);
    setMessages([]);
    setDraftSessionMode("Build");
    resetStream();
  }, [resetStream]);

  const handleSessionModeChange = useCallback(
    async (mode: SessionMode) => {
      if (!activeSessionId) {
        setDraftSessionMode(mode);
        return;
      }

      const target = sessions.find((session) => session.id === activeSessionId);
      const previousMode = normalizeSessionMode(target?.mode);
      onSessionsChange((prev) =>
        prev.map((session) =>
          session.id === activeSessionId ? { ...session, mode } : session,
        ),
      );

      if (!target?.backendCreated) return;

      try {
        await invoke("update_session_mode", {
          sessionId: activeSessionId,
          mode,
        });
      } catch (e) {
        onSessionsChange((prev) =>
          prev.map((session) =>
            session.id === activeSessionId ? { ...session, mode: previousMode } : session,
          ),
        );
        onError?.(`Failed to update session mode: ${e}`);
      }
    },
    [activeSessionId, onError, onSessionsChange, sessions],
  );

  return {
    sessions,
    activeSessionId,
    messages,
    status,
    currentTurn,
    isStreaming,
    isThinking,
    handleSend,
    handleSessionSelect,
    handleNewSession,
    activeSessionMode,
    handleSessionModeChange,
  };
}
