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

interface StartStreamOptions {
  provider: string;
  prompt: string;
  model: string;
  variant?: string | null;
  sessionId: string | null;
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
  const turnSequenceRef = useRef(0);
  const optionsRef = useRef(options);
  optionsRef.current = options;

  // Buffered streamed text: accepted `llm-token` events append here and are
  // flushed at frame cadence (or synchronously before any ordering-sensitive
  // event / lifecycle finalizer) so long responses do not trigger one React
  // state update per token. The buffer preserves original text order.
  const pendingTextRef = useRef<string>("");
  const pendingFrameRef = useRef<FrameHandle | null>(null);

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

  /** Apply any buffered streamed text to the current turn's last text block
   * (or a new text block), then clear the buffer and cancel any pending frame.
   * Synchronous and idempotent: safe to call before any event that can append
   * or mutate blocks, and before completion, error, cancellation, reset, or
   * unmount. Preserves turn id and text-block occurrence order. */
  const flushPendingText = useCallback(() => {
    if (pendingFrameRef.current != null) {
      cancelFrameHandle(pendingFrameRef.current);
      pendingFrameRef.current = null;
    }
    const buffered = pendingTextRef.current;
    if (!buffered) return;
    // Clear the buffer before applying so a reentrant event cannot duplicate
    // text.
    pendingTextRef.current = "";
    updateCurrentTurn((turn) => {
      const blocks = [...turn.blocks];
      const last = blocks[blocks.length - 1];
      if (last && last.kind === "text") {
        blocks[blocks.length - 1] = {
          ...last,
          text: last.text + buffered,
        };
      } else {
        blocks.push({
          kind: "text",
          id: `text-${blocks.length}`,
          text: buffered,
        });
      }
      return { ...turn, blocks };
    });
  }, [updateCurrentTurn]);

  const clearCurrentTurn = useCallback(() => {
    cancelFrameHandle(pendingFrameRef.current);
    pendingFrameRef.current = null;
    pendingTextRef.current = "";
    currentTurnRef.current = null;
    activeRunIdRef.current = null;
    setCurrentTurn(null);
  }, []);

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

      // Stale-event guard: ignore events whose runId does not match the active
      // run. An event with a runId when there is no active run (e.g. after cancel
      // or reset) is also stale. Events with no runId (e.g. approval-* from the
      // broker) pass through as defense in depth.
      const isStale = (runId: unknown): boolean =>
        runId != null && runId !== activeRunIdRef.current;
      try {
        await Promise.all([
          track(
            listen<LlmTokenPayload>("llm-token", (event) => {
              const payload = event.payload;
              const token = typeof payload === "string" ? payload : payload?.token;
              const runId = typeof payload === "string" ? null : payload?.runId;
              if (isStale(runId)) return;
              if (!token) return;
              // Buffer the token and schedule at most one frame flush. The
              // flush preserves original text order by appending the whole
              // buffer to the last text block.
              pendingTextRef.current += token;
              if (pendingFrameRef.current == null) {
                pendingFrameRef.current = scheduleFlush(() => {
                  pendingFrameRef.current = null;
                  flushPendingText();
                });
              }
            })
          ),
          track(
            listen<LlmDonePayload>("llm-done", (event) => {
              const payload = event.payload;
              if (typeof payload !== "string" && isStale(payload?.runId)) return;
              // Flush any buffered text before capturing the authoritative
              // final turn so no final tokens are lost.
              flushPendingText();
              const finalTurn = currentTurnRef.current;
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
                optionsRef.current.onMessages?.((prev) => [
                  ...prev,
                  {
                    id: messageId,
                    role: "agent",
                    content: content || "Completed.",
                    timestamp: new Date(),
                    blocks: blocks.length > 0 ? blocks : undefined,
                  },
                ]);
              }

              clearCurrentTurn();
              optionsRef.current.onStatusChange?.("connected");
            })
          ),
          track(
            listen<LlmErrorPayload>("llm-error", (event) => {
              const err = event.payload;
              if (isStale(err?.runId)) return;
              // Flush buffered text before finalizing so a quick failure
              // cannot lose trailing tokens.
              flushPendingText();
              const finalTurn = currentTurnRef.current;
              const messageId = finalTurn?.id ?? generateTurnId();
              const rawBlocks = finalTurn?.blocks ?? [];
              const blocks = dropReasoningBlocks(rawBlocks);
              const derivedContent = joinTextBlocks(blocks);

              if (err?.message || derivedContent || blocks.length > 0) {
                optionsRef.current.onMessages?.((prev) => [
                  ...prev,
                  {
                    id: messageId,
                    role: "agent",
                    content: derivedContent || "",
                    timestamp: new Date(),
                    error: err?.message || "Completion failed.",
                    blocks: blocks.length > 0 ? blocks : undefined,
                  },
                ]);
              }

              clearCurrentTurn();
              optionsRef.current.onStatusChange?.("error");

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
            })
          ),
          track(
            listen<LlmToolCallPayload>("llm-tool-call", (event) => {
              const payload = event.payload;
              if (isStale(payload?.runId)) return;
              // Flush buffered text before appending a tool block so the
              // visible timeline keeps text before tool calls.
              flushPendingText();
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
              optionsRef.current.onStatusChange?.("acting");
            })
          ),
          track(
            listen<LlmToolResultPayload>("llm-tool-result", (event) => {
              const payload = event.payload;
              if (isStale(payload?.runId)) return;
              // Flush buffered text before pairing a tool result so a late
              // text token cannot interleave between a tool call and result.
              flushPendingText();
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
              optionsRef.current.onStatusChange?.("acting");
            })
          ),
          track(
            listen<LlmReasoningPayload>("llm-reasoning", (event) => {
              const { runId, id, text, phase } = event.payload;
              if (isStale(runId)) return;
              // Flush buffered text before appending/mutating a reasoning
              // block so reasoning does not appear before trailing text.
              flushPendingText();
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
              // Flush buffered text before appending an approval block so
              // the approval card appears after trailing text.
              flushPendingText();
              updateCurrentTurn((turn) => {
                if (
                  turn.blocks.some(
                    (b): b is Extract<TurnBlock, { kind: "approval" }> =>
                      b.kind === "approval" && b.id === event.payload.id
                  )
                ) {
                  return turn;
                }
                return {
                  ...turn,
                  blocks: [
                    ...turn.blocks,
                    {
                      kind: "approval",
                      id: event.payload.id,
                      toolName: event.payload.tool_name,
                      approvalKind: event.payload.kind,
                      title: event.payload.title,
                      summary: event.payload.summary,
                      reason: event.payload.reason,
                      risk: event.payload.risk,
                      status: "pending",
                    },
                  ],
                };
              });
            })
          ),
          track(
            listen<ApprovalResolution>("approval-resolved", (event) => {
              // Flush buffered text before mutating approval block status.
              flushPendingText();
              const status = event.payload.outcome;
              updateCurrentTurn((turn) => ({
                ...turn,
                blocks: turn.blocks.map((block) =>
                  block.kind === "approval" && block.id === event.payload.id
                    ? { ...block, status }
                    : block
                ),
              }));
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
      cancelFrameHandle(pendingFrameRef.current);
      pendingFrameRef.current = null;
      pendingTextRef.current = "";
      cleanup?.();
    };
  }, [updateCurrentTurn, generateTurnId, clearCurrentTurn, flushPendingText]);

  const startStream = useCallback(async (opts: StartStreamOptions) => {
    const runId = crypto.randomUUID();
    activeRunIdRef.current = runId;
    await invoke<string>("complete_streaming", {
      provider: opts.provider,
      prompt: opts.prompt,
      model: opts.model,
      variant: opts.variant ?? null,
      sessionId: opts.sessionId ?? null,
      invokedSkill: opts.invokedSkill ?? null,
      runId,
    });
  }, []);

  const cancelStream = useCallback(async () => {
    const runId = activeRunIdRef.current;
    if (!runId) return;
    const sessionId = optionsRef.current.sessionId;
    if (sessionId) {
      try {
        await invoke<void>("cancel_streaming", { sessionId });
      } catch {
        // best-effort; the backend may already have finalized
      }
    }
    // Finalize the current turn as cancelled locally. Mirror the llm-error
    // finalize path: persist a plain-language "cancelled" assistant message so
    // the transcript records the controlled stop, and clear streaming state.
    // Flush buffered text first so a cancelled turn cannot lose trailing
    // tokens, then clear (which cancels any pending frame deterministically).
    flushPendingText();
    const finalTurn = currentTurnRef.current;
    const rawBlocks = finalTurn?.blocks ?? [];
    const blocks = dropReasoningBlocks(rawBlocks);
    const derivedContent = joinTextBlocks(blocks);
    const messageId = finalTurn?.id ?? generateTurnId();
    const cancelContent = derivedContent ? derivedContent : "Stream cancelled by user.";
    optionsRef.current.onMessages?.((prev) => [
      ...prev,
      {
        id: messageId,
        role: "agent",
        content: cancelContent,
        timestamp: new Date(),
        blocks: blocks.length > 0 ? blocks : undefined,
      },
    ]);
    clearCurrentTurn();
    optionsRef.current.onStatusChange?.("connected");
  }, [clearCurrentTurn, generateTurnId, flushPendingText]);

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
    clearCurrentTurn();
  }, [clearCurrentTurn]);

  return {
    currentTurn,
    startStream,
    resetStream,
    cancelStream,
    resolveApproval,
  };
}
