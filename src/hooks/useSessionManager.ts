import { invoke } from "@tauri-apps/api/core";
import {
  type Dispatch,
  type SetStateAction,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import type {
  AgentStatus,
  CurrentTurn,
  Message,
  ModelOption,
  Session,
  SessionMode,
  TurnBlock,
} from "../types";
import { normalizeSessionMode } from "../types";
import { type ModelVariantWarningPayload, useChatStream } from "./useChatStream";
import type { SelectedModel } from "./useModelAvailability";

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
  onModelVariantFallback?: (warning: ModelVariantWarningPayload) => void;
}

export interface UseSessionManagerResult {
  sessions: Session[];
  activeSessionId: string | null;
  messages: Message[];
  status: AgentStatus;
  currentTurn: CurrentTurn | null;
  isStreaming: boolean;
  isThinking: boolean;
  /** Sessions with an in-flight turn (control-plane running indicator). */
  streamingSessionIds: string[];
  /** Live (unfinalized) turn per session, including background tasks. */
  liveTurnsBySession: Record<string, CurrentTurn | null>;
  /** Latest known status per session. */
  statusBySession: Record<string, AgentStatus>;
  handleSend: (message: string, invokedSkill?: { name: string; args?: string }) => Promise<void>;
  handleSessionSelect: (session: Session) => Promise<void>;
  handleNewSession: () => void;
  activeSessionMode: SessionMode;
  handleSessionModeChange: (mode: SessionMode) => Promise<void>;
  resolveApproval: (id: string, decision: "approve" | "deny") => Promise<void>;
  cancelStream: () => Promise<void>;
  cancelStreamForSession: (sessionId: string | null) => Promise<void>;
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
  onModelVariantFallback,
}: UseSessionManagerParams): UseSessionManagerResult {
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [status, setStatus] = useState<AgentStatus>("idle");
  const [draftSessionMode, setDraftSessionMode] = useState<SessionMode>("Build");
  const [liveTurnsBySession, setLiveTurnsBySession] = useState<
    Record<string, CurrentTurn | null>
  >({});
  const [statusBySession, setStatusBySession] = useState<Record<string, AgentStatus>>({});
  const statusRef = useRef(status);
  statusRef.current = status;
  const latestSelectedSessionRef = useRef<string | null>(null);
  const skipNextWorkspaceResetRef = useRef<string | null>(null);
  const activeSessionIdRef = useRef<string | null>(activeSessionId);

  const prevWorkspaceRef = useRef(activeWorkspaceId);

  useEffect(() => {
    activeSessionIdRef.current = activeSessionId;
  }, [activeSessionId]);

  // Control plane: workspace switches no longer wait for streams to finish.
  // Selecting a task from another project switches workspace and keeps every
  // in-flight turn running; the reset below only clears the draft view.
  useEffect(() => {
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
  }, [activeWorkspaceId]);

  const handleModelVariantWarning = useCallback(
    (warning: ModelVariantWarningPayload) => {
      if (warning.kind !== "missing") return;
      onModelVariantFallback?.(warning);
      const sessionId = activeSessionIdRef.current;
      if (!sessionId) return;
      onSessionsChange((prev) =>
        prev.map((session) =>
          session.id === sessionId &&
          session.model === warning.model &&
          session.provider.toLowerCase() === warning.provider.toLowerCase() &&
          session.variant === warning.variant
            ? { ...session, variant: null }
            : session
        )
      );
    },
    [onModelVariantFallback, onSessionsChange]
  );

  const handleMessagesForSession = useCallback(
    (sessionId: string | null, updater: (prev: Message[]) => Message[]) => {
      const focused = activeSessionIdRef.current;
      if (sessionId === focused) {
        setMessages(updater);
        return;
      }
      // Background task completed while viewing another task: append to that
      // task's stored transcript so switching back shows the result.
      if (!sessionId) {
        setMessages(updater);
        return;
      }
      onSessionsChange((prev) =>
        prev.map((session) =>
          session.id === sessionId
            ? {
                ...session,
                messages: updater(session.messages),
                timestamp: new Date(),
              }
            : session
        )
      );
    },
    [onSessionsChange]
  );

  const handleLiveTurnForSession = useCallback((sessionId: string | null, turn: CurrentTurn | null) => {
    if (!sessionId) return;
    setLiveTurnsBySession((prev) => ({ ...prev, [sessionId]: turn }));
  }, []);

  const handleStatusForSession = useCallback(
    (sessionId: string | null, next: AgentStatus) => {
      if (sessionId) {
        setStatusBySession((prev) => ({ ...prev, [sessionId]: next }));
      }
      if (sessionId === activeSessionIdRef.current) {
        setStatus(next);
      }
    },
    []
  );

  const {
    currentTurn,
    startStream,
    resetStream,
    cancelStream,
    cancelStreamForSession,
    resolveApproval,
  } = useChatStream({
    onMessages: setMessages,
    onMessagesForSession: handleMessagesForSession,
    onLiveTurnForSession: handleLiveTurnForSession,
    onStatusForSession: handleStatusForSession,
    onStatusChange: setStatus,
    onErrorToast: onError,
    onSuccessToast: onSuccess,
    onOpenSettings,
    onModelVariantWarning: handleModelVariantWarning,
    sessionId: activeSessionId,
  });

  // Keep the focused status in sync when switching tasks: show that task's
  // latest known status (running tasks stay green while viewed elsewhere).
  useEffect(() => {
    if (!activeSessionId) return;
    const known = statusBySession[activeSessionId];
    if (known) setStatus(known);
  }, [activeSessionId, statusBySession]);

  const streamingSessionIds = Object.entries(statusBySession)
    .filter(([, s]) => s === "thinking" || s === "acting")
    .map(([id]) => id);
  // The focused turn also counts as streaming even before the per-session map
  // catches up (status state updates synchronously on send).
  if (
    activeSessionId &&
    (status === "thinking" || status === "acting") &&
    !streamingSessionIds.includes(activeSessionId)
  ) {
    streamingSessionIds.push(activeSessionId);
  }

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
              m.provider.toLowerCase() === selectedModel.provider.toLowerCase()
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
          ? `/${invokedSkill.name}${invokedSkill.args ? ` ${invokedSkill.args}` : ""}`
          : message,
        timestamp: new Date(),
      };
      setMessages((prev) => [...prev, userMsg]);
      setStatus("thinking");
      const sendSessionId = activeSessionIdRef.current;
      if (sendSessionId) {
        setStatusBySession((prev) => ({ ...prev, [sendSessionId]: "thinking" }));
      }
      resetStream();

      let effectiveSessionId = activeSessionId;
      let streamSessionId: string | null = effectiveSessionId;
      if (!activeSessionId) {
        const title = userMsg.content.slice(0, 50) + (userMsg.content.length > 50 ? "..." : "");
        const mode = draftSessionMode;

        // Try backend session creation first
        let backendSession: { id: string } | null = null;
        try {
          backendSession = await invoke<{ id: string }>("create_session", {
            title,
            provider: selectedModel.provider,
            model: selectedModel.model,
            variant: selectedModel.variant ?? null,
            workspaceId: activeWorkspaceId ?? null,
            mode,
          });
        } catch (e) {
          console.warn("Backend session creation failed, using local session:", e);
        }

        const sessionId = backendSession?.id ?? `s-${Date.now()}`;
        const isLocalOnly = !backendSession;
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
        activeSessionIdRef.current = sessionId;
        setStatusBySession((prev) => ({ ...prev, [sessionId]: "thinking" }));
        effectiveSessionId = sessionId;
        streamSessionId = isLocalOnly ? null : sessionId;
      } else {
        const existing = sessions.find((session) => session.id === activeSessionId);
        const isLocalOnly = existing?.backendCreated === false;
        streamSessionId = isLocalOnly ? null : effectiveSessionId;
        if (effectiveSessionId) {
          setStatusBySession((prev) => ({ ...prev, [effectiveSessionId as string]: "thinking" }));
        }
      }

      try {
        await startStream({
          provider: selectedModel.provider,
          prompt: message,
          model: selectedModel.model,
          variant: selectedModel.variant ?? null,
          sessionId: streamSessionId,
          invokedSkill: invokedSkill ?? null,
        });
      } catch (e) {
        setStatus("error");
        const failedId = effectiveSessionId ?? activeSessionIdRef.current;
        if (failedId) {
          setStatusBySession((prev) => ({ ...prev, [failedId]: "error" }));
        }
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
      sessions,
      startStream,
    ]
  );

  const handleSessionSelect = useCallback(
    async (session: Session) => {
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
            activeSessionIdRef.current = selectedSession.id;
            setMessages(loadedMessages);
            const knownLoaded = statusBySession[selectedSession.id];
            if (knownLoaded) setStatus(knownLoaded);
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
        activeSessionIdRef.current = selectedSession.id;
        setMessages(selectedSession.messages);
        // Restore that task's last known status so running tasks show live
        // while viewed and idle tasks do not inherit the previous spinner.
        // The live turn itself is restored by the stream hook's focus switch.
        setStatus((current) => {
          const known = statusBySession[selectedSession.id];
          if (known) return known;
          // If the previous view was streaming but this task has no known
          // status, fall back to connected/idle instead of the stale spinner.
          if (current === "thinking" || current === "acting") return "connected";
          return current;
        });
      } catch (e) {
        if (latestSelectedSessionRef.current !== selectionId) return;
        onError?.(`Unable to open session: ${e}`);
      }
    },
    [activeWorkspaceId, onError, onSessionsChange, onSwitchWorkspace, statusBySession]
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
        prev.map((session) => (session.id === activeSessionId ? { ...session, mode } : session))
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
            session.id === activeSessionId ? { ...session, mode: previousMode } : session
          )
        );
        onError?.(`Failed to update session mode: ${e}`);
      }
    },
    [activeSessionId, onError, onSessionsChange, sessions]
  );

  return {
    sessions,
    activeSessionId,
    messages,
    status,
    currentTurn,
    isStreaming,
    isThinking,
    streamingSessionIds,
    liveTurnsBySession,
    statusBySession,
    handleSend,
    handleSessionSelect,
    handleNewSession,
    activeSessionMode,
    handleSessionModeChange,
    resolveApproval,
    cancelStream,
    cancelStreamForSession,
  };
}
