import { invoke } from "@tauri-apps/api/core";
import {
  type Dispatch,
  type SetStateAction,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { updateMessageById } from "../subagentActivity";
import type {
  AgentStatus,
  ApprovalDecision,
  CurrentTurn,
  Message,
  ModelOption,
  Session,
  TurnBlock,
} from "../types";
import { type ModelVariantWarningPayload, useChatStream } from "./useChatStream";
import type { SelectedModel } from "./useModelAvailability";

export interface SessionManagerStreamOptions {
  provider: string;
  prompt: string;
  model: string;
  variant?: string | null;
  sessionId: string | null;
  /** Frontend Session.id used for stream event routing; differs from the
   * backend sessionId for local-only sessions. */
  runSessionId?: string | null;
  invokedSkill?: { name: string; args?: string } | null;
  agentId?: string | null;
  delegatedAgent?: { name: string; task: string } | null;
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
  agentId?: string | null;
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
  /** Background sessions with a reply the user has not opened yet. */
  unreadSessionIds: string[];
  /** Live (unfinalized) turn per session, including background tasks. */
  liveTurnsBySession: Record<string, CurrentTurn | null>;
  /** Latest known status per session. */
  statusBySession: Record<string, AgentStatus>;
  /** Epoch ms when each streaming session's current run started. */
  runStartedAtBySession: Record<string, number>;
  /** The focused session's first transcript page is still loading. */
  historyLoading: boolean;
  /** An earlier-history page is being prepended to the focused session. */
  earlierHistoryLoading: boolean;
  /** Whether the focused session has older transcript pages to load.
   * `undefined` for sessions that were never paged (drafts, local-only,
   * sessions created in this app lifetime). */
  hasMoreHistory: boolean | undefined;
  loadEarlierMessages: () => Promise<void>;
  /** Begins fetching a session's first transcript page on intent signals
   * (hover/focus) so the eventual select resolves instantly. */
  prefetchSession: (session: Session) => void;
  createTask: (title: string, workspaceId: string) => Promise<Session>;
  messageActionsPending: boolean;
  retryMessage: (messageId: string) => Promise<void>;
  forkMessage: (messageId: string) => Promise<void>;
  handleSend: (
    message: string,
    invokedSkill?: { name: string; args?: string },
    delegatedAgent?: { name: string; task: string }
  ) => Promise<void>;
  handleSessionSelect: (session: Session) => Promise<void>;
  handleNewSession: () => void;
  resolveApproval: (id: string, decision: ApprovalDecision) => Promise<void>;
  cancelStream: () => Promise<void>;
  cancelStreamForSession: (sessionId: string | null) => Promise<void>;
}

function isRunningStatus(status: AgentStatus): boolean {
  return status === "thinking" || status === "acting";
}

/** Transcript entries loaded per fetch; older pages prepend on scroll-up. */
const HISTORY_PAGE_SIZE = 40;
const HISTORY_CACHE_LIMIT = 25;

interface SessionTranscriptPage {
  id: string;
  display_transcript: string;
  transcript_start: number;
  transcript_total: number;
}

interface HistoryMeta {
  /** Index of the next earlier page boundary (`before` cursor). */
  nextBefore: number;
  hasMore: boolean;
}

function transcriptPageToMessages(
  sessionId: string,
  timestamp: Date,
  page: SessionTranscriptPage
): Message[] {
  const transcript = JSON.parse(page.display_transcript) as Array<{
    role: string;
    content: string;
    blocks?: TurnBlock[];
  }>;
  return transcript.map((msg, i) => {
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
      id: `m-${sessionId}-${page.transcript_start + i}-${msg.role}`,
      role,
      content: msg.content,
      timestamp,
      blocks,
    };
  });
}

/** A draft has no run of its own, so it must not inherit the spinner of the
 * session that was focused before it. */
function draftStatus(current: AgentStatus): AgentStatus {
  return isRunningStatus(current) ? "connected" : current;
}

export function useSessionManager({
  models,
  selectedModel,
  sessions,
  onSessionsChange,
  activeWorkspaceId,
  agentId,
  onSwitchWorkspace,
  onError,
  onSuccess,
  onOpenSettings,
  onModelVariantFallback,
}: UseSessionManagerParams): UseSessionManagerResult {
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [status, setStatus] = useState<AgentStatus>("idle");
  const [messageActionsPending, setMessageActionsPending] = useState(false);
  const messageActionPendingRef = useRef(false);
  // ponytail: unread state is session-memory only; persist read timestamps if it must survive restarts.
  const [unreadSessionIds, setUnreadSessionIds] = useState<string[]>([]);
  const [liveTurnsBySession, setLiveTurnsBySession] = useState<Record<string, CurrentTurn | null>>(
    {}
  );
  const [statusBySession, setStatusBySession] = useState<Record<string, AgentStatus>>({});
  const [runStartedAtBySession, setRunStartedAtBySession] = useState<Record<string, number>>({});
  const statusRef = useRef(status);
  statusRef.current = status;
  const sessionsRef = useRef(sessions);
  useEffect(() => {
    sessionsRef.current = sessions;
  }, [sessions]);
  const latestSelectedSessionRef = useRef<string | null>(null);
  const skipNextWorkspaceResetRef = useRef<string | null>(null);
  const activeSessionIdRef = useRef<string | null>(activeSessionId);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [earlierHistoryLoading, setEarlierHistoryLoading] = useState(false);
  const earlierLoadingRef = useRef(false);
  /** In-flight/resolved first transcript pages, keyed by session. Lets hover
   * prefetch feed the same promise a later click awaits. */
  const historyCacheRef = useRef(new Map<string, Promise<SessionTranscriptPage>>());
  const [historyMeta, setHistoryMeta] = useState<Record<string, HistoryMeta>>({});
  const historyMetaRef = useRef(historyMeta);
  useEffect(() => {
    historyMetaRef.current = historyMeta;
  }, [historyMeta]);

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
    activeSessionIdRef.current = null;
    setMessages([]);
    setStatus(draftStatus);
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
      setUnreadSessionIds((prev) => (prev.includes(sessionId) ? prev : [...prev, sessionId]));
      // A prefetched transcript may now be missing this turn; the appended
      // messages are newer than any cached page.
      historyCacheRef.current.delete(sessionId);
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

  const handleMessageUpdateForSession = useCallback(
    (sessionId: string | null, messageId: string, update: (message: Message) => Message) => {
      if (!sessionId || sessionId === activeSessionIdRef.current) {
        setMessages((prev) => updateMessageById(prev, messageId, update));
        return;
      }
      onSessionsChange((prev) =>
        prev.map((session) =>
          session.id === sessionId
            ? { ...session, messages: updateMessageById(session.messages, messageId, update) }
            : session
        )
      );
    },
    [onSessionsChange]
  );

  const handleLiveTurnForSession = useCallback(
    (sessionId: string | null, turn: CurrentTurn | null) => {
      if (!sessionId) return;
      setLiveTurnsBySession((prev) => ({ ...prev, [sessionId]: turn }));
    },
    []
  );

  // One writer for the per-session maps so run start times are recorded at
  // event time and cleared when a run ends (thinking/acting -> anything else).
  const markSessionStatus = useCallback((sessionId: string | null, next: AgentStatus) => {
    if (!sessionId) return;
    setStatusBySession((prev) => ({ ...prev, [sessionId]: next }));
    setRunStartedAtBySession((prev) => {
      if (isRunningStatus(next)) {
        if (prev[sessionId] !== undefined) return prev;
        return { ...prev, [sessionId]: Date.now() };
      }
      if (!(sessionId in prev)) return prev;
      const remaining = { ...prev };
      delete remaining[sessionId];
      return remaining;
    });
  }, []);

  const handleStatusForSession = useCallback(
    (sessionId: string | null, next: AgentStatus) => {
      markSessionStatus(sessionId, next);
      if (sessionId === activeSessionIdRef.current) {
        setStatus(next);
      }
    },
    [markSessionStatus]
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
    onMessageUpdateForSession: handleMessageUpdateForSession,
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

  // One availability rule for every prompt-sending path: the selected model
  // must be listed and its provider must be credentialed.
  const selectedModelAvailable = Boolean(
    selectedModel &&
      models.some(
        (model) =>
          model.provider.toLowerCase() === selectedModel.provider.toLowerCase() &&
          model.model === selectedModel.model &&
          model.configured !== false
      )
  );

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
    async (
      message: string,
      invokedSkill?: { name: string; args?: string },
      delegatedAgent?: { name: string; task: string }
    ) => {
      if (messageActionPendingRef.current) return;
      if (!selectedModel || !selectedModelAvailable) {
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
      const firstMessageSession = activeSessionId
        ? sessions.find(
            (session) =>
              session.id === activeSessionId &&
              session.backendCreated &&
              session.messages.length === 0 &&
              session.title === "New chat"
          )
        : undefined;
      if (firstMessageSession) {
        const context = userMsg.content.trim().replace(/\s+/g, " ");
        const characters = Array.from(context);
        const fallbackTitle =
          characters.slice(0, 50).join("") + (characters.length > 50 ? "..." : "");
        onSessionsChange((previous) =>
          previous.map((session) =>
            session.id === firstMessageSession.id && session.title === "New chat"
              ? { ...session, title: fallbackTitle }
              : session
          )
        );
        void invoke<string | null>("generate_session_title", {
          sessionId: firstMessageSession.id,
          prompt: userMsg.content,
        })
          .then((title) => {
            if (!title?.trim()) return;
            onSessionsChange((previous) =>
              previous.map((session) =>
                session.id === firstMessageSession.id && session.title === fallbackTitle
                  ? { ...session, title }
                  : session
              )
            );
          })
          .catch(() => {});
      }
      setMessages((prev) => [...prev, userMsg]);
      setStatus("thinking");
      markSessionStatus(activeSessionIdRef.current, "thinking");
      resetStream();

      let effectiveSessionId = activeSessionId;
      let streamSessionId: string | null = effectiveSessionId;
      if (!activeSessionId) {
        const title = userMsg.content.slice(0, 50) + (userMsg.content.length > 50 ? "..." : "");

        // Try backend session creation first
        let backendSession: { id: string } | null = null;
        try {
          backendSession = await invoke<{ id: string }>("create_session", {
            title,
            provider: selectedModel.provider,
            model: selectedModel.model,
            variant: selectedModel.variant ?? null,
            workspaceId: activeWorkspaceId ?? null,
            agentId: agentId ?? null,
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
          agentId,
          timestamp: new Date(),
          messages: [userMsg],
          status: "active",
          backendCreated: !!backendSession,
          workspaceId: activeWorkspaceId,
        };
        onSessionsChange((prev) => [newSession, ...prev]);
        setActiveSessionId(sessionId);
        activeSessionIdRef.current = sessionId;
        markSessionStatus(sessionId, "thinking");
        effectiveSessionId = sessionId;
        streamSessionId = isLocalOnly ? null : sessionId;
      } else {
        const existing = sessions.find((session) => session.id === activeSessionId);
        const isLocalOnly = existing?.backendCreated === false;
        streamSessionId = isLocalOnly ? null : effectiveSessionId;
        markSessionStatus(effectiveSessionId, "thinking");
      }

      try {
        await startStream({
          provider: selectedModel.provider,
          prompt: message,
          model: selectedModel.model,
          variant: selectedModel.variant ?? null,
          sessionId: streamSessionId,
          // Route the run's events by the frontend Session.id so local-only
          // sessions (backend sessionId null) still get their own stream.
          runSessionId: effectiveSessionId,
          invokedSkill: invokedSkill ?? null,
          agentId: agentId ?? null,
          delegatedAgent: delegatedAgent ?? null,
        });
      } catch (e) {
        setStatus("error");
        markSessionStatus(effectiveSessionId ?? activeSessionIdRef.current, "error");
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
      agentId,
      markSessionStatus,
      onError,
      onOpenSettings,
      onSessionsChange,
      resetStream,
      selectedModel,
      selectedModelAvailable,
      sessions,
      startStream,
    ]
  );

  /** Fetches (or reuses a prefetched) first transcript page. Records history
   * metadata so "load earlier" knows whether more pages exist. */
  const requestFirstPage = useCallback((session: Session) => {
    const cached = historyCacheRef.current.get(session.id);
    if (cached) return cached;
    const pending = invoke<SessionTranscriptPage>("get_session", {
      sessionId: session.id,
      tail: HISTORY_PAGE_SIZE,
    })
      .then((page) => {
        if (!page) return page;
        setHistoryMeta((prev) =>
          prev[session.id]
            ? prev
            : {
                ...prev,
                [session.id]: {
                  nextBefore: page.transcript_start,
                  hasMore: page.transcript_start > 0,
                },
              }
        );
        return page;
      })
      .catch((error) => {
        historyCacheRef.current.delete(session.id);
        throw error;
      });
    const cache = historyCacheRef.current;
    cache.set(session.id, pending);
    // Bound the cache; Map iteration order is insertion order.
    while (cache.size > HISTORY_CACHE_LIMIT) {
      const oldest = cache.keys().next().value;
      if (oldest === undefined) break;
      cache.delete(oldest);
    }
    return pending;
  }, []);

  const prefetchSession = useCallback(
    (session: Session) => {
      if (!session.backendCreated || session.messages.length > 0) return;
      void requestFirstPage(session).catch(() => {});
    },
    [requestFirstPage]
  );

  const loadEarlierMessages = useCallback(async () => {
    const sessionId = activeSessionIdRef.current;
    if (!sessionId || earlierLoadingRef.current) return;
    const meta = historyMetaRef.current[sessionId];
    if (!meta?.hasMore || meta.nextBefore <= 0) return;
    earlierLoadingRef.current = true;
    setEarlierHistoryLoading(true);
    try {
      const page = await invoke<SessionTranscriptPage>("get_session", {
        sessionId,
        tail: HISTORY_PAGE_SIZE,
        before: meta.nextBefore,
      });
      const timestamp =
        sessionsRef.current.find((item) => item.id === sessionId)?.timestamp ?? new Date();
      const older = transcriptPageToMessages(sessionId, new Date(timestamp), page);
      const prepend = (prev: Message[]) => {
        const known = new Set(prev.map((m) => m.id));
        return [...older.filter((m) => !known.has(m.id)), ...prev];
      };
      if (activeSessionIdRef.current === sessionId) setMessages(prepend);
      onSessionsChange((prev) =>
        prev.map((item) =>
          item.id === sessionId ? { ...item, messages: prepend(item.messages) } : item
        )
      );
      setHistoryMeta((prev) => ({
        ...prev,
        [sessionId]: {
          nextBefore: page.transcript_start,
          hasMore: page.transcript_start > 0,
        },
      }));
    } catch (e) {
      onError?.(`Unable to load earlier messages: ${e}`);
    } finally {
      earlierLoadingRef.current = false;
      setEarlierHistoryLoading(false);
    }
  }, [onError, onSessionsChange]);

  const handleSessionSelect = useCallback(
    async (session: Session) => {
      const selectionId = session.id;
      latestSelectedSessionRef.current = selectionId;
      const previousSessionId = activeSessionIdRef.current;

      // Selection applies in the same frame: the rail highlight and the chat
      // pane respond before any history loads.
      setActiveSessionId(selectionId);
      activeSessionIdRef.current = selectionId;
      setUnreadSessionIds((prev) => prev.filter((id) => id !== selectionId));
      setMessages(session.messages);
      // Restore that task's last known status so running tasks show live
      // while viewed and idle tasks do not inherit the previous spinner.
      // The live turn itself is restored by the stream hook's focus switch.
      setStatus((current) => {
        const known = statusBySession[selectionId];
        if (known) return known;
        // If the previous view was streaming but this task has no known
        // status, fall back to connected/idle instead of the stale spinner.
        if (current === "thinking" || current === "acting") return "connected";
        return current;
      });

      const needsWorkspaceSwitch = Boolean(
        onSwitchWorkspace && session.workspaceId && session.workspaceId !== activeWorkspaceId
      );
      const needsTranscript = Boolean(session.backendCreated && session.messages.length === 0);
      setHistoryLoading(needsTranscript);
      // The transcript fetch starts alongside the workspace switch — they are
      // independent, so neither should wait on the other.
      const transcriptPromise = needsTranscript ? requestFirstPage(session) : null;

      try {
        if (needsWorkspaceSwitch && onSwitchWorkspace && session.workspaceId) {
          skipNextWorkspaceResetRef.current = session.workspaceId;
          const switched = await onSwitchWorkspace(session.workspaceId);
          if (!switched) {
            skipNextWorkspaceResetRef.current = null;
            if (latestSelectedSessionRef.current !== selectionId) return;
            // Roll back the optimistic selection.
            const previous = sessionsRef.current.find((item) => item.id === previousSessionId);
            setActiveSessionId(previousSessionId);
            activeSessionIdRef.current = previousSessionId;
            setMessages(previous?.messages ?? []);
            setHistoryLoading(false);
            onError?.("Unable to switch workspace for this session.");
            return;
          }
        }

        if (!transcriptPromise) {
          if (latestSelectedSessionRef.current === selectionId) setHistoryLoading(false);
          return;
        }

        try {
          const page = await transcriptPromise;
          if (latestSelectedSessionRef.current !== selectionId || !page) return;
          const loadedMessages = transcriptPageToMessages(
            selectionId,
            new Date(session.timestamp),
            page
          );
          // A background turn may have finished while the page was in flight;
          // streamed-in messages are newer than the fetched snapshot.
          const freshest = sessionsRef.current.find((item) => item.id === selectionId);
          const finalMessages =
            freshest && freshest.messages.length > 0 ? freshest.messages : loadedMessages;
          setMessages(finalMessages);
          onSessionsChange((prev) => {
            if (!prev.some((item) => item.id === selectionId)) {
              return [{ ...session, messages: finalMessages }, ...prev];
            }
            return prev.map((item) =>
              item.id === selectionId
                ? {
                    ...item,
                    messages: item.messages.length > 0 ? item.messages : loadedMessages,
                  }
                : item
            );
          });
        } catch (e) {
          if (latestSelectedSessionRef.current !== selectionId) return;
          console.warn("Failed to load session detail from backend:", e);
        } finally {
          if (latestSelectedSessionRef.current === selectionId) setHistoryLoading(false);
        }
      } catch (e) {
        if (latestSelectedSessionRef.current !== selectionId) return;
        setHistoryLoading(false);
        onError?.(`Unable to open session: ${e}`);
      }
    },
    [
      activeWorkspaceId,
      onError,
      onSessionsChange,
      onSwitchWorkspace,
      requestFirstPage,
      statusBySession,
    ]
  );

  const runMessageAction = useCallback(
    async (messageId: string, fork: boolean) => {
      if (messageActionPendingRef.current || isRunningStatus(statusRef.current)) return;
      const source = sessions.find((item) => item.id === activeSessionId);
      const index = messages.findIndex(
        (message) => message.id === messageId && message.role === "agent"
      );
      if (!source || index < 0) return;
      if (source.backendCreated === false) {
        onError?.("Save this conversation before retrying or forking it.");
        return;
      }
      const preceding = messages.slice(0, index);
      let userIndex = preceding.length - 1;
      while (userIndex >= 0 && preceding[userIndex].role !== "user") userIndex -= 1;
      const userMessage = preceding[userIndex];
      if (!userMessage) {
        onError?.("There is no user message to retry or fork.");
        return;
      }
      if (!fork && (!selectedModel || !selectedModelAvailable)) {
        onError?.("Select an available model before retrying.", {
          label: "Open Settings",
          onClick: () => onOpenSettings?.(),
        });
        return;
      }
      if (
        !fork &&
        index < messages.length - 1 &&
        !window.confirm(
          "Retry this response? Later messages in this conversation will be removed. Fork instead to keep them."
        )
      )
        return;
      const userTurns = preceding.filter((message) => message.role === "user").length;
      messageActionPendingRef.current = true;
      setMessageActionsPending(true);
      try {
        const detail = await invoke<{
          id: string;
          title: string;
          provider: string;
          model: string;
          variant?: string | null;
          workspace_id?: string;
          agent_id?: string | null;
        }>("run_session_message_action", {
          request: {
            sessionId: source.id,
            turnCount: fork ? userTurns : userTurns - 1,
            fork,
            responseText: messages[index].content,
            expectedUserTurns: messages.filter((message) => message.role === "user").length,
            failedResponse:
              !fork && Boolean(messages[index].error) && index === messages.length - 1,
            retryPrompt: userMessage.content,
          },
        });
        if (!detail?.id) throw new Error("The conversation could not be saved.");
        if (fork) {
          const task: Session = {
            ...source,
            id: detail.id,
            title: detail.title,
            provider: detail.provider,
            model: detail.model,
            variant: detail.variant,
            workspaceId: detail.workspace_id ?? source.workspaceId,
            agentId: detail.agent_id ?? source.agentId ?? null,
            messages: messages.slice(0, index + 1),
            timestamp: new Date(),
            status: "active",
            backendCreated: true,
          };
          onSessionsChange((previous) => [task, ...previous]);
          // Do not steal focus if the user navigated away while saving.
          if (activeSessionIdRef.current === source.id) {
            setActiveSessionId(task.id);
            activeSessionIdRef.current = task.id;
            latestSelectedSessionRef.current = task.id;
            setMessages(task.messages);
            setStatus("connected");
          }
          onSuccess?.("Conversation forked.");
          return;
        }
        const retryMessages = messages.slice(0, userIndex + 1);
        onSessionsChange((previous) =>
          previous.map((item) =>
            item.id === source.id ? { ...item, messages: retryMessages } : item
          )
        );
        if (activeSessionIdRef.current === source.id) {
          setMessages(retryMessages);
          setStatus("thinking");
          resetStream();
        }
        markSessionStatus(source.id, "thinking");
        if (!selectedModel) return;
        void startStream({
          ...selectedModel,
          prompt: userMessage.content,
          sessionId: source.id,
          runSessionId: source.id,
          agentId: source.agentId ?? agentId ?? null,
        }).catch((error) => {
          handleStatusForSession(source.id, "error");
          handleMessagesForSession(source.id, (previous) => {
            // llm-error may already have finalized the failed turn. Command
            // failures that arrive without that event still need a retryable row.
            if (previous[previous.length - 1]?.error) return previous;
            return [
              ...previous,
              {
                id: `m-${crypto.randomUUID()}-error`,
                role: "agent",
                content: "",
                timestamp: new Date(),
                error: "The retry failed. Try again or check provider settings.",
              },
            ];
          });
          onError?.(`Failed to retry: ${error}`);
        });
      } catch (error) {
        onError?.(`Failed to ${fork ? "fork" : "retry"}: ${error}`);
      } finally {
        messageActionPendingRef.current = false;
        setMessageActionsPending(false);
      }
    },
    [
      activeSessionId,
      agentId,
      handleStatusForSession,
      handleMessagesForSession,
      markSessionStatus,
      messages,
      onError,
      onOpenSettings,
      onSessionsChange,
      onSuccess,
      resetStream,
      selectedModel,
      selectedModelAvailable,
      sessions,
      startStream,
    ]
  );

  const retryMessage = useCallback(
    (messageId: string) => runMessageAction(messageId, false),
    [runMessageAction]
  );
  const forkMessage = useCallback(
    (messageId: string) => runMessageAction(messageId, true),
    [runMessageAction]
  );

  const handleNewSession = useCallback(() => {
    const leavingRun = isRunningStatus(statusRef.current);
    latestSelectedSessionRef.current = null;
    setActiveSessionId(null);
    activeSessionIdRef.current = null;
    setMessages([]);
    setStatus(draftStatus);
    // A running session keeps going in the background: the stream hook stashes
    // its live turn when focus moves to the draft. Resetting here would drop it.
    if (!leavingRun) resetStream();
  }, [resetStream]);

  const createTask = useCallback(
    async (title: string, workspaceId: string): Promise<Session> => {
      if (!title.trim() || !workspaceId) throw new Error("A task name and workspace are required.");
      if (!selectedModel || !selectedModelAvailable) {
        throw new Error("Select an available model before creating a task.");
      }
      if (workspaceId !== activeWorkspaceId) {
        if (!onSwitchWorkspace) throw new Error("Unable to switch to the selected workspace.");
        skipNextWorkspaceResetRef.current = workspaceId;
        try {
          if (!(await onSwitchWorkspace(workspaceId)))
            throw new Error("Unable to switch to the selected workspace.");
        } catch (error) {
          skipNextWorkspaceResetRef.current = null;
          throw error;
        }
      }
      const record = await invoke<{ id: string }>("create_session", {
        title: title.trim(),
        workspaceId,
        provider: selectedModel.provider,
        model: selectedModel.model,
        variant: selectedModel.variant ?? null,
        agentId: agentId ?? null,
      });
      if (!record?.id) throw new Error("The task could not be saved. Try again.");
      const task: Session = {
        id: record.id,
        title: title.trim(),
        workspaceId,
        provider: selectedModel.provider,
        model: selectedModel.model,
        variant: selectedModel.variant ?? null,
        agentId,
        messages: [],
        timestamp: new Date(),
        status: "idle",
        backendCreated: true,
      };
      onSessionsChange((previous) => [task, ...previous]);
      latestSelectedSessionRef.current = task.id;
      setActiveSessionId(task.id);
      activeSessionIdRef.current = task.id;
      setMessages([]);
      setStatus("connected");
      return task;
    },
    [
      activeWorkspaceId,
      agentId,
      onSessionsChange,
      onSwitchWorkspace,
      selectedModel,
      selectedModelAvailable,
    ]
  );

  const hasMoreHistory = activeSessionId ? historyMeta[activeSessionId]?.hasMore : undefined;

  return {
    sessions,
    activeSessionId,
    messages,
    status,
    currentTurn,
    isStreaming,
    isThinking,
    streamingSessionIds,
    unreadSessionIds,
    liveTurnsBySession,
    statusBySession,
    runStartedAtBySession,
    historyLoading,
    earlierHistoryLoading,
    hasMoreHistory,
    loadEarlierMessages,
    prefetchSession,
    createTask,
    messageActionsPending,
    retryMessage,
    forkMessage,
    handleSend,
    handleSessionSelect,
    handleNewSession,
    resolveApproval,
    cancelStream,
    cancelStreamForSession,
  };
}
