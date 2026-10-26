import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import type {
  AgentStatus,
  ApprovalDecision,
  ApprovalRequest,
  ApprovalResolution,
  CurrentTurn,
  Message,
  TurnBlock,
} from "../types";

interface CorpusAutoBuildComplete {
  success: boolean;
  symbol_count: number;
}

interface UseChatStreamOptions {
  onMessages?: React.Dispatch<React.SetStateAction<Message[]>>;
  /** Routed completion for a specific session. When provided, llm-done /
   * llm-error / cancel finalize via this instead of the focused onMessages,
   * enabling free switching between running tasks. */
  onMessagesForSession?: (
    sessionId: string | null,
    updater: (prev: Message[]) => Message[]
  ) => void;
  /** Live turn updates for background (non-focused) sessions. */
  onLiveTurnForSession?: (sessionId: string | null, turn: CurrentTurn | null) => void;
  /** Per-session status updates. Falls back to onStatusChange for focused. */
  onStatusForSession?: (sessionId: string | null, status: AgentStatus) => void;
  onStatusChange?: (status: AgentStatus) => void;
  onErrorToast?: (message: string, action?: { label: string; onClick: () => void }) => void;
  onSuccessToast?: (message: string) => void;
  onOpenSettings?: () => void;
  onRetry?: () => void;
  onModelVariantWarning?: (warning: ModelVariantWarningPayload) => void;
  /** Invoked when the frontend must resolve a pending approval (e.g. an
   *  in-app card asks the backend to approve/deny). Resolves with the
   *  backend's acknowledgement. */
  onResolveApproval?: (id: string, decision: ApprovalDecision) => Promise<unknown>;
  /** Active session id; used by `cancelStream` to target the in-flight run.
   *  May be null for local-only sessions (cancel is a no-op then). */
  sessionId?: string | null;
}

interface LlmTokenPayload {
  runId?: string;
  token: string;
}

interface LlmDonePayloadObject {
  runId?: string;
  response: string;
  prompt_tokens?: number;
  response_tokens?: number;
  tool_calls?: number;
}

type LlmDonePayload = string | LlmDonePayloadObject;

export interface ModelVariantWarningPayload {
  kind: string;
  provider: string;
  model: string;
  variant: string;
  message: string;
}

interface LlmReasoningPayload {
  runId?: string;
  id: string;
  text: string;
  phase: "delta" | "complete";
}

interface LlmToolCallPayload {
  runId?: string;
  id: string;
  name: string;
  arguments?: unknown;
}

interface LlmToolResultPayload {
  runId?: string;
  id: string;
  name: string;
  result: string;
}

interface LlmErrorPayload {
  runId?: string;
  code: string;
  message: string;
}

function joinTextBlocks(blocks: TurnBlock[]): string {
  return blocks
    .filter((block): block is { kind: "text"; id: string; text: string } => block.kind === "text")
    .map((block) => block.text)
    .join("");
}

/** Strip ephemeral reasoning blocks. Reasoning is shown live only and must
 * never reach a finalized `Message` (which is persisted, copied, or fed to
 * verification and tracing downstream). */
function dropReasoningBlocks(blocks: TurnBlock[]): TurnBlock[] {
  return blocks.filter((block) => block.kind !== "reasoning");
}

function hasApproval(turn: CurrentTurn | null, id: string): boolean {
  return !!turn?.blocks.some((b) => b.kind === "approval" && b.id === id);
}

function appendApproval(turn: CurrentTurn, req: ApprovalRequest): CurrentTurn {
  if (turn.blocks.some((b) => b.kind === "approval" && b.id === req.id)) return turn;
  return {
    ...turn,
    blocks: [
      ...turn.blocks,
      {
        kind: "approval",
        id: req.id,
        toolName: req.tool_name,
        approvalKind: req.kind,
        title: req.title,
        summary: req.summary,
        reason: req.reason,
        risk: req.risk,
        status: "pending",
      },
    ],
  };
}

function markApprovalResolved(
  turn: CurrentTurn,
  id: string,
  status: ApprovalResolution["outcome"]
): CurrentTurn {
  return {
    ...turn,
    blocks: turn.blocks.map((block) =>
      block.kind === "approval" && block.id === id ? { ...block, status } : block
    ),
  };
}

interface StartStreamOptions {
  provider: string;
  prompt: string;
  model: string;
  variant?: string | null;
  sessionId: string | null;
  /** Frontend Session.id used for event routing; may differ from backend
   * sessionId for local-only sessions. */
  runSessionId?: string | null;
  invokedSkill?: { name: string; args?: string } | null;
}

/** Schedules a single frame flush of buffered streamed text. Defaults to the
 * browser animation frame; tests substitute a deterministic queue via
 * `setFrameSchedulerForTest`. A scheduled handle is cancelled by
 * `cancelFrameHandle` before a synchronous flush or cleanup. */
type FrameHandle = number;
interface FrameScheduler {
  schedule: (cb: () => void) => FrameHandle;
  cancel: (handle: FrameHandle) => void;
}

const browserFrameScheduler: FrameScheduler = {
  schedule:
    typeof requestAnimationFrame === "function"
      ? (cb) => requestAnimationFrame(cb)
      : (cb) => setTimeout(cb, 0) as unknown as FrameHandle,
  cancel:
    typeof cancelAnimationFrame === "function"
      ? (handle) => cancelAnimationFrame(handle)
      : (handle) => clearTimeout(handle as number),
};

let testFrameScheduler: FrameScheduler | null = null;

/** Test-only seam: replace the frame scheduler/canceller with a deterministic
 * queue. Pass `null` to restore the browser animation-frame scheduler. */
export function setFrameSchedulerForTest(scheduler: FrameScheduler | null) {
  testFrameScheduler = scheduler;
}

function scheduleFlush(cb: () => void): FrameHandle {
  return (testFrameScheduler ?? browserFrameScheduler).schedule(cb);
}

function cancelFrameHandle(handle: FrameHandle | null) {
  if (handle == null) return;
  (testFrameScheduler ?? browserFrameScheduler).cancel(handle);
}

export function useChatStream(options: UseChatStreamOptions = {}) {
  const [currentTurn, setCurrentTurn] = useState<CurrentTurn | null>(null);
  const currentTurnRef = useRef<CurrentTurn | null>(null);
  const activeRunIdRef = useRef<string | null>(null);
  /** All in-flight run ids (control-plane: multiple tasks may run at once). */
  const activeRunIdsRef = useRef<Set<string>>(new Set());
  /** runId -> sessionId captured at startStream time (events carry runId
   * only). Maps to the frontend Session.id so every run — including
   * local-only ones with no backend session — routes to a real session. */
  const runSessionMapRef = useRef<Map<string, string | null>>(new Map());
  /** Live turns for background (non-focused) sessions, keyed by sessionId. */
  const backgroundTurnsRef = useRef<Map<string | null, CurrentTurn>>(new Map());
  // Buffered streamed text per session: accepted `llm-token` events append to
  // the buffer for their run's session and are flushed at frame cadence (or
  // synchronously before any ordering-sensitive event / lifecycle finalizer)
  // so long responses do not trigger one React state update per token. A
  // flush applies to the visible turn only while that session is focused;
  // otherwise it lands on the session's stashed live turn. Keying by session
  // guarantees text can never be attributed to the wrong session.
  const pendingBySessionRef = useRef<Map<string | null, string>>(new Map());
  const pendingFrameBySessionRef = useRef<Map<string | null, FrameHandle>>(new Map());
  const prevFocusedSessionRef = useRef<string | null | undefined>(undefined);
  const turnSequenceRef = useRef(0);
  const optionsRef = useRef(options);
  optionsRef.current = options;

  const generateTurnId = useCallback(() => {
    turnSequenceRef.current += 1;
    return `turn-${Date.now()}-${turnSequenceRef.current}`;
  }, []);

  const createTurn = useCallback((): CurrentTurn => {
    return {
      id: generateTurnId(),
      blocks: [],
      createdAt: new Date(),
    };
  }, [generateTurnId]);

  const updateCurrentTurn = useCallback(
    (updater: (turn: CurrentTurn) => CurrentTurn) => {
      const existing = currentTurnRef.current ?? createTurn();
      const next = updater(existing);
      currentTurnRef.current = next;
      setCurrentTurn(next);
      return next;
    },
    [createTurn]
  );

  const clearCurrentTurn = useCallback(() => {
    // The visible turn belongs to the focused session; drop its pending
    // buffer and frame. Read the prop directly (getFocusedSession is
    // declared below) so this callback stays dep-free.
    const focused = optionsRef.current.sessionId ?? null;
    const frame = pendingFrameBySessionRef.current.get(focused);
    if (frame != null) {
      cancelFrameHandle(frame);
      pendingFrameBySessionRef.current.delete(focused);
    }
    pendingBySessionRef.current.delete(focused);
    currentTurnRef.current = null;
    activeRunIdRef.current = null;
    setCurrentTurn(null);
  }, []);

  const getFocusedSession = useCallback((): string | null => {
    return optionsRef.current.sessionId ?? null;
  }, []);

  const sessionForRun = useCallback(
    (runId: unknown): string | null => {
      if (typeof runId !== "string") return getFocusedSession();
      return runSessionMapRef.current.get(runId) ?? getFocusedSession();
    },
    [getFocusedSession]
  );

  const emitStatus = useCallback(
    (sessionId: string | null, status: AgentStatus) => {
      optionsRef.current.onStatusForSession?.(sessionId, status);
      if (sessionId === getFocusedSession()) {
        optionsRef.current.onStatusChange?.(status);
      }
    },
    [getFocusedSession]
  );

  const updateBackgroundTurn = useCallback(
    (sessionId: string | null, updater: (turn: CurrentTurn) => CurrentTurn) => {
      const existing = backgroundTurnsRef.current.get(sessionId) ?? {
        id: `turn-${Date.now()}-bg`,
        blocks: [],
        createdAt: new Date(),
      };
      const next = updater(existing);
      backgroundTurnsRef.current.set(sessionId, next);
      optionsRef.current.onLiveTurnForSession?.(sessionId, next);
      return next;
    },
    []
  );

  /** Apply any buffered streamed text for a session to that session's turn:
   * the visible current turn while the session is focused, otherwise its
   * stashed background turn (published via onLiveTurnForSession). Then clear
   * the buffer and cancel any pending frame. Synchronous and idempotent:
   * safe to call before any ordering-sensitive event or lifecycle finalizer.
   * Every call site derives the session from the event payload
   * (sessionForRun) or the known focused session — never a bare flush. */
  const flushPendingForSession = useCallback(
    (sessionId: string | null) => {
      const frame = pendingFrameBySessionRef.current.get(sessionId);
      if (frame != null) {
        cancelFrameHandle(frame);
        pendingFrameBySessionRef.current.delete(sessionId);
      }
      const buffered = pendingBySessionRef.current.get(sessionId) ?? "";
      if (!buffered) return;
      // Clear the buffer before applying so a reentrant event cannot
      // duplicate text.
      pendingBySessionRef.current.delete(sessionId);
      const appendText = (turn: CurrentTurn): CurrentTurn => {
        const blocks = [...turn.blocks];
        const last = blocks[blocks.length - 1];
        if (last && last.kind === "text") {
          blocks[blocks.length - 1] = { ...last, text: last.text + buffered };
        } else {
          blocks.push({
            kind: "text",
            id: `text-${blocks.length}`,
            text: buffered,
          });
        }
        return { ...turn, blocks };
      };
      if (sessionId === getFocusedSession()) {
        updateCurrentTurn(appendText);
      } else {
        updateBackgroundTurn(sessionId, appendText);
      }
    },
    [getFocusedSession, updateCurrentTurn, updateBackgroundTurn]
  );

  const clearRun = useCallback((runId: string | null) => {
    if (runId) {
      activeRunIdsRef.current.delete(runId);
      runSessionMapRef.current.delete(runId);
    }
    if (activeRunIdRef.current && runId === activeRunIdRef.current) {
      const remaining = Array.from(activeRunIdsRef.current);
      activeRunIdRef.current =
        remaining.length > 0 ? (remaining[remaining.length - 1] ?? null) : null;
    }
  }, []);

  // When the user switches tasks mid-stream, stash the visible live turn into
  // the background map for the previous task and restore the new task's live
  // turn (if any). This keeps free switching lossless. Stash only when the
  // previous task actually has an in-flight run; otherwise the visible turn
  // already belongs to the incoming task (new-task interim) and must stay.
  useEffect(() => {
    const focused = options.sessionId ?? null;
    const prev = prevFocusedSessionRef.current;
    if (prev === undefined) {
      prevFocusedSessionRef.current = focused;
      return;
    }
    if (prev === focused) return;
    prevFocusedSessionRef.current = focused;
    const prevHasActiveRun =
      prev != null &&
      Array.from(runSessionMapRef.current.entries()).some(
        ([runId, mapped]) => mapped === prev && activeRunIdsRef.current.has(runId)
      );
    // Flush + stash outgoing only for a previously streaming task. Pending
    // text is keyed by session, so only the outgoing session's buffer is
    // read here — a token for another run can never be spliced into this
    // stash.
    const prevFrame = pendingFrameBySessionRef.current.get(prev);
    if (prevFrame != null) {
      cancelFrameHandle(prevFrame);
      pendingFrameBySessionRef.current.delete(prev);
    }
    const outgoingText = pendingBySessionRef.current.get(prev) ?? "";
    pendingBySessionRef.current.delete(prev);
    if (prevHasActiveRun && prev != null) {
      if (outgoingText && currentTurnRef.current) {
        const turn = currentTurnRef.current;
        const blocks = [...turn.blocks];
        const last = blocks[blocks.length - 1];
        if (last && last.kind === "text") {
          blocks[blocks.length - 1] = { ...last, text: last.text + outgoingText };
        } else {
          blocks.push({ kind: "text", id: `text-${blocks.length}`, text: outgoingText });
        }
        const stashed = { ...turn, blocks };
        backgroundTurnsRef.current.set(prev, stashed);
        optionsRef.current.onLiveTurnForSession?.(prev, stashed);
      } else if (currentTurnRef.current) {
        backgroundTurnsRef.current.set(prev, currentTurnRef.current);
        optionsRef.current.onLiveTurnForSession?.(prev, currentTurnRef.current);
      }
      // Restore incoming — including any text buffered for it since the last
      // frame (mirrors the outgoing fold). flushPendingForSession routes to
      // the visible turn because `focused` is now focused.
      const incoming = backgroundTurnsRef.current.get(focused) ?? null;
      currentTurnRef.current = incoming;
      setCurrentTurn(incoming);
      flushPendingForSession(focused);
    } else if (outgoingText && currentTurnRef.current) {
      // No active run for prev (e.g. new-task interim with pending text):
      // fold it into the visible turn without swapping views.
      const turn = currentTurnRef.current;
      const blocks = [...turn.blocks];
      const last = blocks[blocks.length - 1];
      if (last && last.kind === "text") {
        blocks[blocks.length - 1] = { ...last, text: last.text + outgoingText };
      } else {
        blocks.push({ kind: "text", id: `text-${blocks.length}`, text: outgoingText });
      }
      const next = { ...turn, blocks };
      currentTurnRef.current = next;
      setCurrentTurn(next);
    }
    // Otherwise (idle switch): leave the visible turn alone; incoming idle
    // tasks correctly show null and incoming streaming tasks will populate
    // via their background restore on their next event... except when the
    // background turn already exists — restore it now, along with any text
    // buffered for it.
    if (!prevHasActiveRun) {
      const incoming = backgroundTurnsRef.current.get(focused);
      if (incoming) {
        currentTurnRef.current = incoming;
        setCurrentTurn(incoming);
      }
      flushPendingForSession(focused);
    }
  }, [options.sessionId, flushPendingForSession]);

  useEffect(() => {
    let cancelled = false;
    let cleanup: (() => void) | null = null;

    (async () => {
      const unlisteners: (() => void)[] = [];

      const track = (p: Promise<() => void>) =>
        p.then((u) => {
          if (cancelled) {
            u();
          } else {
            unlisteners.push(u);
          }
          return u;
        });

      // Stale-event guard: ignore events whose runId is not in the active
      // set. Multiple tasks may run concurrently, so every started run stays
      // active until done/error/cancel. Events with no runId (e.g. approval-*
      // from the broker, legacy string payloads) pass through to focused.
      // Unknown string runIds are always stale (e.g. post-cancel late events).
      const isStale = (runId: unknown): boolean => {
        if (runId == null) return false;
        if (typeof runId !== "string") return false;
        if (!runSessionMapRef.current.has(runId)) return true;
        return !activeRunIdsRef.current.has(runId);
      };
      const isBackground = (runId: unknown): boolean => {
        if (runId == null) return false;
        if (typeof runId !== "string") return false;
        const focused = getFocusedSession();
        // No focused task (draft/new-task interim): treat live events as
        // foreground so the just-started run renders immediately.
        if (focused == null) return false;
        const mapped = runSessionMapRef.current.get(runId);
        if (mapped === undefined) return false;
        return mapped !== focused;
      };
      // When a completion arrives without a runId (legacy/test payloads)
      // while no task is focused, attribute it to the single in-flight run
      // so background completions still land in the right transcript.
      const singleActiveSession = (): string | null => {
        const sessions = new Set<string | null>();
        for (const [runId, mapped] of runSessionMapRef.current.entries()) {
          if (activeRunIdsRef.current.has(runId)) sessions.add(mapped);
        }
        if (sessions.size === 1) {
          const only = Array.from(sessions)[0];
          return only ?? null;
        }
        return null;
      };
      const clearRunsForSession = (sessionId: string | null) => {
        for (const [runId, mapped] of Array.from(runSessionMapRef.current.entries())) {
          if (mapped === sessionId && activeRunIdsRef.current.has(runId)) {
            activeRunIdsRef.current.delete(runId);
            runSessionMapRef.current.delete(runId);
            if (activeRunIdRef.current === runId) activeRunIdRef.current = null;
          }
        }
        if (activeRunIdsRef.current.size > 0 && !activeRunIdRef.current) {
          const remaining = Array.from(activeRunIdsRef.current);
          activeRunIdRef.current = remaining[remaining.length - 1] ?? null;
        }
      };
      // Resolve which session a completion (done/error) belongs to: the run's
      // own session, or — for legacy/test payloads with no runId — the single
      // in-flight run's session, so it cannot corrupt the focused view. Also
      // flushes that session's buffered text and picks the authoritative final
      // turn: its stashed turn when background, the visible turn when focused.
      const resolveCompletionSession = (
        runId: string | null
      ): {
        session: string | null;
        fallbackSingle: string | null;
        finalTurn: CurrentTurn | null;
      } => {
        const eventSession = sessionForRun(runId);
        flushPendingForSession(eventSession);
        const focusedBefore = getFocusedSession();
        const fallbackSingle = runId == null ? singleActiveSession() : null;
        const session = fallbackSingle ?? eventSession;
        if (session !== eventSession) {
          flushPendingForSession(session);
        }
        const isBackgroundCompletion =
          session !== focusedBefore && backgroundTurnsRef.current.has(session);
        const finalTurn = isBackgroundCompletion
          ? (backgroundTurnsRef.current.get(session) ?? null)
          : currentTurnRef.current;
        return { session, fallbackSingle, finalTurn };
      };
      // NOTE: approval handlers below branch on `isBackground(runId)` (not
      // `sid === focused`) so the draft case (`focused == null` renders
      // foreground, matching `llm-token`) stays consistent, and they drop
      // stale runIds via `isStale` instead of falling back to focused.
      try {
        await Promise.all([
          track(
            listen<LlmTokenPayload>("llm-token", (event) => {
              const payload = event.payload;
              const token = typeof payload === "string" ? payload : payload?.token;
              const runId = typeof payload === "string" ? null : payload?.runId;
              if (isStale(runId)) return;
              if (!token) return;
              // Buffer under the run's own session — never a shared buffer —
              // so a token can never be attributed to the wrong session. The
              // scheduled flush routes to the visible turn only while that
              // session is still focused.
              const sessionId = sessionForRun(runId);
              const prev = pendingBySessionRef.current.get(sessionId) ?? "";
              pendingBySessionRef.current.set(sessionId, prev + token);
              if (!pendingFrameBySessionRef.current.has(sessionId)) {
                const handle = scheduleFlush(() => {
                  pendingFrameBySessionRef.current.delete(sessionId);
                  flushPendingForSession(sessionId);
                });
                pendingFrameBySessionRef.current.set(sessionId, handle);
              }
              if (sessionId !== getFocusedSession()) {
                emitStatus(sessionId, "thinking");
              }
            })
          ),
          track(
            listen<LlmDonePayload>("llm-done", (event) => {
              const payload = event.payload;
              if (typeof payload !== "string" && isStale(payload?.runId)) return;
              const runId = typeof payload === "string" ? null : (payload?.runId ?? null);
              if (runId != null && isBackground(runId)) {
                const sessionId = sessionForRun(runId);
                flushPendingForSession(sessionId);
                const finalTurn = backgroundTurnsRef.current.get(sessionId);
                const payloadContent =
                  typeof payload === "string" ? payload : (payload?.response ?? "");
                const rawBlocks = finalTurn?.blocks ?? [];
                const blocks = dropReasoningBlocks(rawBlocks);
                const derivedContent = joinTextBlocks(blocks);
                const content = payloadContent || derivedContent || "";
                const messageId = finalTurn?.id ?? generateTurnId();
                if (content || blocks.length > 0) {
                  const message: Message = {
                    id: messageId,
                    role: "agent",
                    content: content || "Completed.",
                    timestamp: new Date(),
                    blocks: blocks.length > 0 ? blocks : undefined,
                  };
                  if (optionsRef.current.onMessagesForSession) {
                    optionsRef.current.onMessagesForSession(sessionId, (prev) => [
                      ...prev,
                      message,
                    ]);
                  } else {
                    optionsRef.current.onMessages?.((prev) => [...prev, message]);
                  }
                }
                backgroundTurnsRef.current.delete(sessionId);
                pendingBySessionRef.current.delete(sessionId);
                optionsRef.current.onLiveTurnForSession?.(sessionId, null);
                emitStatus(sessionId, "connected");
                clearRun(typeof runId === "string" ? runId : null);
                return;
              }
              const {
                session: completionSession,
                fallbackSingle,
                finalTurn,
              } = resolveCompletionSession(runId);
              const payloadContent =
                typeof payload === "string" ? payload : (payload?.response ?? "");
              const rawBlocks = finalTurn?.blocks ?? [];
              // Reasoning blocks are ephemeral: do not let them leak into
              // the finalized message content, blocks, or anything that
              // gets copied or persisted downstream.
              const blocks = dropReasoningBlocks(rawBlocks);
              const derivedContent = joinTextBlocks(blocks);
              // Prefer the backend's authoritative response text when present;
              // otherwise fall back to streamed text blocks.
              const content = payloadContent || derivedContent || "";
              const messageId = finalTurn?.id ?? generateTurnId();

              if (content || blocks.length > 0) {
                const message: Message = {
                  id: messageId,
                  role: "agent",
                  content: content || "Completed.",
                  timestamp: new Date(),
                  blocks: blocks.length > 0 ? blocks : undefined,
                };
                if (optionsRef.current.onMessagesForSession) {
                  optionsRef.current.onMessagesForSession(completionSession, (prev) => [
                    ...prev,
                    message,
                  ]);
                } else {
                  optionsRef.current.onMessages?.((prev) => [...prev, message]);
                }
              }

              // Clear the visible turn only when the completion belongs to
              // the focused session; a completion routed elsewhere must not
              // discard an in-progress visible turn.
              if (completionSession === getFocusedSession()) {
                clearCurrentTurn();
              }
              if (typeof runId === "string") {
                clearRun(runId);
              } else if (fallbackSingle != null) {
                clearRunsForSession(fallbackSingle);
              }
              const focusedDone = completionSession;
              // Clear any stashed background turn for the completed session
              // so a later switch does not resurrect it.
              backgroundTurnsRef.current.delete(focusedDone);
              pendingBySessionRef.current.delete(focusedDone);
              optionsRef.current.onLiveTurnForSession?.(focusedDone, null);
              emitStatus(focusedDone, "connected");
            })
          ),
          track(
            listen<LlmErrorPayload>("llm-error", (event) => {
              const err = event.payload;
              if (isStale(err?.runId)) return;
              const runId = err?.runId ?? null;
              if (typeof runId === "string" && isBackground(runId)) {
                const sessionId = sessionForRun(runId);
                flushPendingForSession(sessionId);
                const finalTurn = backgroundTurnsRef.current.get(sessionId);
                const messageId = finalTurn?.id ?? generateTurnId();
                const rawBlocks = finalTurn?.blocks ?? [];
                const blocks = dropReasoningBlocks(rawBlocks);
                const derivedContent = joinTextBlocks(blocks);
                if (err?.message || derivedContent || blocks.length > 0) {
                  const message: Message = {
                    id: messageId,
                    role: "agent",
                    content: derivedContent || "",
                    timestamp: new Date(),
                    error: err?.message || "Completion failed.",
                    blocks: blocks.length > 0 ? blocks : undefined,
                  };
                  if (optionsRef.current.onMessagesForSession) {
                    optionsRef.current.onMessagesForSession(sessionId, (prev) => [
                      ...prev,
                      message,
                    ]);
                  } else {
                    optionsRef.current.onMessages?.((prev) => [...prev, message]);
                  }
                }
                backgroundTurnsRef.current.delete(sessionId);
                optionsRef.current.onLiveTurnForSession?.(sessionId, null);
                emitStatus(sessionId, "error");
                clearRun(runId);
                if (err?.code === "API_KEY_MISSING") {
                  optionsRef.current.onErrorToast?.(err.message, {
                    label: "Open Settings",
                    onClick: optionsRef.current.onOpenSettings ?? (() => {}),
                  });
                } else {
                  optionsRef.current.onErrorToast?.(err?.message || "Completion failed.", {
                    label: "Retry",
                    onClick: optionsRef.current.onRetry ?? (() => {}),
                  });
                }
                return;
              }
              const {
                session: completionSession,
                fallbackSingle,
                finalTurn,
              } = resolveCompletionSession(runId);
              const messageId = finalTurn?.id ?? generateTurnId();
              const rawBlocks = finalTurn?.blocks ?? [];
              const blocks = dropReasoningBlocks(rawBlocks);
              const derivedContent = joinTextBlocks(blocks);

              if (err?.message || derivedContent || blocks.length > 0) {
                const message: Message = {
                  id: messageId,
                  role: "agent",
                  content: derivedContent || "",
                  timestamp: new Date(),
                  error: err?.message || "Completion failed.",
                  blocks: blocks.length > 0 ? blocks : undefined,
                };
                if (optionsRef.current.onMessagesForSession) {
                  optionsRef.current.onMessagesForSession(completionSession, (prev) => [
                    ...prev,
                    message,
                  ]);
                } else {
                  optionsRef.current.onMessages?.((prev) => [...prev, message]);
                }
              }

              // Clear the visible turn only when the failure belongs to the
              // focused session; a failure routed elsewhere must not discard
              // an in-progress visible turn.
              if (completionSession === getFocusedSession()) {
                clearCurrentTurn();
              }
              if (typeof runId === "string") {
                clearRun(runId);
              } else if (fallbackSingle != null) {
                clearRunsForSession(fallbackSingle);
              }
              backgroundTurnsRef.current.delete(completionSession);
              pendingBySessionRef.current.delete(completionSession);
              optionsRef.current.onLiveTurnForSession?.(completionSession, null);
              emitStatus(completionSession, "error");

              if (err?.code === "API_KEY_MISSING" || err?.code === "ENTITLEMENT_FAILED") {
                optionsRef.current.onErrorToast?.(err.message, {
                  label: "Open Settings",
                  onClick: optionsRef.current.onOpenSettings ?? (() => {}),
                });
              } else if (err?.code === "AUTH_EXPIRED") {
                optionsRef.current.onErrorToast?.(err.message, {
                  label: "Sign in again",
                  onClick: optionsRef.current.onOpenSettings ?? (() => {}),
                });
              } else {
                optionsRef.current.onErrorToast?.(err?.message || "Completion failed.", {
                  label: "Retry",
                  onClick: optionsRef.current.onRetry ?? (() => {}),
                });
              }
            })
          ),
          track(
            listen<LlmToolCallPayload>("llm-tool-call", (event) => {
              const payload = event.payload;
              if (isStale(payload?.runId)) return;
              if (isBackground(payload?.runId)) {
                const sessionId = sessionForRun(payload?.runId);
                flushPendingForSession(sessionId);
                updateBackgroundTurn(sessionId, (turn) => ({
                  ...turn,
                  blocks: [
                    ...turn.blocks,
                    {
                      kind: "tool",
                      id: payload.id,
                      name: payload.name,
                      arguments: payload.arguments,
                      status: "calling" as const,
                    },
                  ],
                }));
                emitStatus(sessionId, "acting");
                return;
              }
              // Flush buffered text before appending a tool block so the
              // visible timeline keeps text before tool calls.
              flushPendingForSession(sessionForRun(payload?.runId));
              updateCurrentTurn((turn) => ({
                ...turn,
                blocks: [
                  ...turn.blocks,
                  {
                    kind: "tool",
                    id: payload.id,
                    name: payload.name,
                    arguments: payload.arguments,
                    status: "calling" as const,
                  },
                ],
              }));
              emitStatus(getFocusedSession(), "acting");
            })
          ),
          track(
            listen<LlmToolResultPayload>("llm-tool-result", (event) => {
              const payload = event.payload;
              if (isStale(payload?.runId)) return;
              if (isBackground(payload?.runId)) {
                const sessionId = sessionForRun(payload?.runId);
                flushPendingForSession(sessionId);
                updateBackgroundTurn(sessionId, (turn) => {
                  const idx = turn.blocks.findIndex(
                    (b): b is TurnBlock & { kind: "tool" } =>
                      b.kind === "tool" && b.id === payload.id
                  );
                  if (idx >= 0) {
                    const blocks = [...turn.blocks];
                    const existing = blocks[idx];
                    if (existing.kind === "tool") {
                      blocks[idx] = {
                        ...existing,
                        result: payload.result,
                        status: "completed",
                      };
                    }
                    return { ...turn, blocks };
                  }
                  return {
                    ...turn,
                    blocks: [
                      ...turn.blocks,
                      {
                        kind: "tool",
                        id: payload.id,
                        name: payload.name,
                        result: payload.result,
                        status: "completed" as const,
                      },
                    ],
                  };
                });
                emitStatus(sessionId, "acting");
                return;
              }
              // Flush buffered text before pairing a tool result so a late
              // text token cannot interleave between a tool call and result.
              flushPendingForSession(sessionForRun(payload?.runId));
              updateCurrentTurn((turn) => {
                const idx = turn.blocks.findIndex(
                  (b): b is TurnBlock & { kind: "tool" } => b.kind === "tool" && b.id === payload.id
                );
                if (idx >= 0) {
                  const blocks = [...turn.blocks];
                  const existing = blocks[idx];
                  if (existing.kind === "tool") {
                    blocks[idx] = {
                      ...existing,
                      result: payload.result,
                      status: "completed",
                    };
                  }
                  return { ...turn, blocks };
                }
                console.warn(
                  `[useChatStream] Received llm-tool-result for id "${payload.id}" with no matching llm-tool-call; appending as completed.`,
                  { name: payload.name }
                );
                return {
                  ...turn,
                  blocks: [
                    ...turn.blocks,
                    {
                      kind: "tool",
                      id: payload.id,
                      name: payload.name,
                      result: payload.result,
                      status: "completed" as const,
                    },
                  ],
                };
              });
              emitStatus(getFocusedSession(), "acting");
            })
          ),
          track(
            listen<LlmReasoningPayload>("llm-reasoning", (event) => {
              const { runId, id, text, phase } = event.payload;
              if (isStale(runId)) return;
              if (isBackground(runId)) {
                const sessionId = sessionForRun(runId);
                flushPendingForSession(sessionId);
                updateBackgroundTurn(sessionId, (turn) => {
                  const idx = turn.blocks.findIndex(
                    (b): b is Extract<TurnBlock, { kind: "reasoning" }> =>
                      b.kind === "reasoning" && b.id === id
                  );
                  if (phase === "complete") {
                    if (idx >= 0) {
                      const blocks = [...turn.blocks];
                      blocks[idx] = { kind: "reasoning", id, text, phase: "complete" };
                      return { ...turn, blocks };
                    }
                    return {
                      ...turn,
                      blocks: [...turn.blocks, { kind: "reasoning", id, text, phase: "complete" }],
                    };
                  }
                  if (idx >= 0) {
                    const blocks = [...turn.blocks];
                    const existing = blocks[idx];
                    if (existing.kind === "reasoning") {
                      blocks[idx] = {
                        ...existing,
                        text: existing.text + text,
                        phase: "delta",
                      };
                    }
                    return { ...turn, blocks };
                  }
                  return {
                    ...turn,
                    blocks: [...turn.blocks, { kind: "reasoning", id, text, phase: "delta" }],
                  };
                });
                return;
              }
              // Flush buffered text before appending/mutating a reasoning
              // block so reasoning does not appear before trailing text.
              flushPendingForSession(sessionForRun(runId));
              updateCurrentTurn((turn) => {
                const idx = turn.blocks.findIndex(
                  (b): b is Extract<TurnBlock, { kind: "reasoning" }> =>
                    b.kind === "reasoning" && b.id === id
                );
                if (phase === "complete") {
                  // A complete event replaces accumulated deltas with the
                  // provider's authoritative text for the same id. A new
                  // burst with the same id always starts here, so a
                  // previously-completed block is overwritten.
                  if (idx >= 0) {
                    const blocks = [...turn.blocks];
                    blocks[idx] = { kind: "reasoning", id, text, phase: "complete" };
                    return { ...turn, blocks };
                  }
                  return {
                    ...turn,
                    blocks: [...turn.blocks, { kind: "reasoning", id, text, phase: "complete" }],
                  };
                }
                if (idx >= 0) {
                  const blocks = [...turn.blocks];
                  const existing = blocks[idx];
                  if (existing.kind === "reasoning") {
                    blocks[idx] = {
                      ...existing,
                      text: existing.text + text,
                      phase: "delta",
                    };
                  }
                  return { ...turn, blocks };
                }
                return {
                  ...turn,
                  blocks: [...turn.blocks, { kind: "reasoning", id, text, phase: "delta" }],
                };
              });
            })
          ),
          track(
            listen<CorpusAutoBuildComplete>("corpus-auto-build-complete", (event) => {
              if (event.payload.success) {
                optionsRef.current.onSuccessToast?.(
                  `Corpus ready with ${event.payload.symbol_count} symbols.`
                );
              } else {
                optionsRef.current.onErrorToast?.(
                  "Corpus auto-build failed. Use Build Corpus to retry."
                );
              }
            })
          ),
          track(
            listen<ModelVariantWarningPayload>("llm-model-variant-warning", (event) => {
              optionsRef.current.onErrorToast?.(
                event.payload.message || "Model variant was not available; using Default."
              );
              optionsRef.current.onModelVariantWarning?.(event.payload);
            })
          ),
          track(
            listen<ApprovalRequest>("approval-requested", (event) => {
              // Route the card to the run's owning session; payloads without
              // run_id fall back to the focused session (legacy emitters).
              // Unknown run_ids are stale (post-cancel/completion late events)
              // and are dropped instead of misrouting to focused.
              const runId = event.payload.run_id ?? null;
              if (isStale(runId)) return;
              if (isBackground(runId)) {
                const sid = sessionForRun(runId);
                // Flush buffered text before appending an approval block so
                // the approval card appears after trailing text.
                flushPendingForSession(sid);
                updateBackgroundTurn(sid, (turn) => appendApproval(turn, event.payload));
                return;
              }
              flushPendingForSession(sessionForRun(runId));
              updateCurrentTurn((turn) => appendApproval(turn, event.payload));
            })
          ),
          track(
            listen<ApprovalResolution>("approval-resolved", (event) => {
              const runId = event.payload.run_id ?? null;
              if (isStale(runId)) return;
              const status = event.payload.outcome;
              const approvalId = event.payload.id;
              if (isBackground(runId)) {
                const sid = sessionForRun(runId);
                // Flush buffered text before mutating approval block status.
                flushPendingForSession(sid);
                // No-op when there is no stashed card: avoids resurrecting a
                // completed turn with an empty ghost via lazy creation.
                if (!hasApproval(backgroundTurnsRef.current.get(sid) ?? null, approvalId)) {
                  return;
                }
                updateBackgroundTurn(sid, (turn) => markApprovalResolved(turn, approvalId, status));
                return;
              }
              flushPendingForSession(sessionForRun(runId));
              // No-op when the focused turn has no such card: avoids creating
              // an empty turn on a stale/duplicate resolution.
              if (!hasApproval(currentTurnRef.current, approvalId)) {
                return;
              }
              updateCurrentTurn((turn) => markApprovalResolved(turn, approvalId, status));
            })
          ),
        ]);
      } catch (error) {
        cancelled = true;
        unlisteners.forEach((unlisten) => {
          unlisten();
        });
        throw error;
      }

      cleanup = () => {
        unlisteners.forEach((unlisten) => {
          unlisten();
        });
      };

      if (cancelled) {
        cleanup();
        return;
      }
    })();

    return () => {
      cancelled = true;
      // Cancel any pending frame flush so unmount cannot leak buffered text
      // into a later mount or a stale run.
      for (const handle of pendingFrameBySessionRef.current.values()) {
        cancelFrameHandle(handle);
      }
      pendingFrameBySessionRef.current.clear();
      pendingBySessionRef.current.clear();
      cleanup?.();
    };
  }, [
    updateCurrentTurn,
    generateTurnId,
    clearCurrentTurn,
    clearRun,
    flushPendingForSession,
    updateBackgroundTurn,
    emitStatus,
    sessionForRun,
    getFocusedSession,
  ]);

  const startStream = useCallback(
    async (opts: StartStreamOptions) => {
      const runId = crypto.randomUUID();
      activeRunIdRef.current = runId;
      activeRunIdsRef.current.add(runId);
      // Route events by the frontend Session.id so local-only sessions map
      // to a real session instead of null.
      const runSessionId = opts.runSessionId ?? opts.sessionId ?? null;
      runSessionMapRef.current.set(runId, runSessionId);
      // Starting a new turn for the focused session clears any stashed
      // background turn for that session so a stale live view cannot resurface.
      if (runSessionId === getFocusedSession()) {
        backgroundTurnsRef.current.delete(runSessionId);
        pendingBySessionRef.current.delete(runSessionId);
        const frame = pendingFrameBySessionRef.current.get(runSessionId);
        if (frame != null) {
          cancelFrameHandle(frame);
          pendingFrameBySessionRef.current.delete(runSessionId);
        }
        optionsRef.current.onLiveTurnForSession?.(runSessionId, null);
      }
      await invoke<string>("complete_streaming", {
        provider: opts.provider,
        prompt: opts.prompt,
        model: opts.model,
        variant: opts.variant ?? null,
        sessionId: opts.sessionId ?? null,
        invokedSkill: opts.invokedSkill ?? null,
        runId,
      });
      return runId;
    },
    [getFocusedSession]
  );

  const cancelStreamForSession = useCallback(
    async (targetSessionId: string | null) => {
      const sid = targetSessionId ?? optionsRef.current.sessionId ?? null;
      // Find the latest run for this session.
      let targetRun: string | null = null;
      for (const [runId, mapped] of runSessionMapRef.current.entries()) {
        if (mapped === sid && activeRunIdsRef.current.has(runId)) targetRun = runId;
      }
      const isFocused = sid === getFocusedSession();
      const hasLiveTurn = isFocused
        ? currentTurnRef.current != null || (pendingBySessionRef.current.get(sid) ?? "") !== ""
        : backgroundTurnsRef.current.has(sid) ||
          (pendingBySessionRef.current.get(sid) ?? "") !== "";
      // No active run and no live turn: no-op (preserve legacy behavior).
      if (!targetRun && !hasLiveTurn) return;
      if (sid) {
        try {
          await invoke<void>("cancel_streaming", { sessionId: sid });
        } catch {
          // best-effort; the backend may already have finalized
        }
      }
      if (isFocused) {
        flushPendingForSession(sid);
        const finalTurn = currentTurnRef.current;
        const rawBlocks = finalTurn?.blocks ?? [];
        const blocks = dropReasoningBlocks(rawBlocks);
        const derivedContent = joinTextBlocks(blocks);
        const messageId = finalTurn?.id ?? generateTurnId();
        const cancelContent = derivedContent ? derivedContent : "Stream cancelled by user.";
        const message: Message = {
          id: messageId,
          role: "agent",
          content: cancelContent,
          timestamp: new Date(),
          blocks: blocks.length > 0 ? blocks : undefined,
        };
        if (optionsRef.current.onMessagesForSession) {
          optionsRef.current.onMessagesForSession(sid, (prev) => [...prev, message]);
        } else {
          optionsRef.current.onMessages?.((prev) => [...prev, message]);
        }
        clearCurrentTurn();
        backgroundTurnsRef.current.delete(sid);
        optionsRef.current.onLiveTurnForSession?.(sid, null);
        if (targetRun) clearRun(targetRun);
        emitStatus(sid, "connected");
        return;
      }
      // Background cancel: finalize into that session's transcript.
      flushPendingForSession(sid);
      const finalTurn = backgroundTurnsRef.current.get(sid);
      const rawBlocks = finalTurn?.blocks ?? [];
      const blocks = dropReasoningBlocks(rawBlocks);
      const derivedContent = joinTextBlocks(blocks);
      const messageId = finalTurn?.id ?? generateTurnId();
      const cancelContent = derivedContent ? derivedContent : "Stream cancelled by user.";
      const message: Message = {
        id: messageId,
        role: "agent",
        content: cancelContent,
        timestamp: new Date(),
        blocks: blocks.length > 0 ? blocks : undefined,
      };
      if (optionsRef.current.onMessagesForSession) {
        optionsRef.current.onMessagesForSession(sid, (prev) => [...prev, message]);
      } else {
        optionsRef.current.onMessages?.((prev) => [...prev, message]);
      }
      backgroundTurnsRef.current.delete(sid);
      pendingBySessionRef.current.delete(sid);
      optionsRef.current.onLiveTurnForSession?.(sid, null);
      if (targetRun) clearRun(targetRun);
      emitStatus(sid, "connected");
    },
    [
      clearCurrentTurn,
      generateTurnId,
      flushPendingForSession,
      clearRun,
      emitStatus,
      getFocusedSession,
    ]
  );

  const cancelStream = useCallback(async () => {
    await cancelStreamForSession(optionsRef.current.sessionId ?? null);
  }, [cancelStreamForSession]);

  const resolveApproval = useCallback(async (id: string, decision: ApprovalDecision) => {
    // Default to invoking the Tauri command if the consumer did not supply
    // a custom resolver. This keeps the hook self-contained for simple
    // chat views while letting callers swap in test fakes.
    if (optionsRef.current.onResolveApproval) {
      await optionsRef.current.onResolveApproval(id, decision);
      return;
    }
    await invoke("resolve_approval_request", { id, decision });
  }, []);

  const resetStream = useCallback(() => {
    const focused = optionsRef.current.sessionId ?? null;
    const frame = pendingFrameBySessionRef.current.get(focused);
    if (frame != null) {
      cancelFrameHandle(frame);
      pendingFrameBySessionRef.current.delete(focused);
    }
    pendingBySessionRef.current.delete(focused);
    currentTurnRef.current = null;
    setCurrentTurn(null);
    backgroundTurnsRef.current.delete(focused);
    optionsRef.current.onLiveTurnForSession?.(focused, null);
  }, []);

  return {
    currentTurn,
    startStream,
    resetStream,
    cancelStream,
    cancelStreamForSession,
    resolveApproval,
  };
}
