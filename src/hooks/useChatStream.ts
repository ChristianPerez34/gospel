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
}

interface LlmDonePayloadObject {
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
  id: string;
  text: string;
  phase: "delta" | "complete";
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

export function useChatStream(options: UseChatStreamOptions = {}) {
  const [currentTurn, setCurrentTurn] = useState<CurrentTurn | null>(null);
  const currentTurnRef = useRef<CurrentTurn | null>(null);
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
    currentTurnRef.current = null;
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
      try {
        await Promise.all([
          track(
            listen<string>("llm-token", (event) => {
              updateCurrentTurn((turn) => {
                const blocks = [...turn.blocks];
                const last = blocks[blocks.length - 1];
                if (last && last.kind === "text") {
                  blocks[blocks.length - 1] = {
                    ...last,
                    text: last.text + event.payload,
                  };
                } else {
                  blocks.push({
                    kind: "text",
                    id: `text-${blocks.length}`,
                    text: event.payload,
                  });
                }
                return { ...turn, blocks };
              });
            })
          ),
          track(
            listen<LlmDonePayload>("llm-done", (event) => {
              const payload = event.payload;
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
            listen<{ code: string; message: string }>("llm-error", (event) => {
              const err = event.payload;
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
            listen<{ id: string; name: string; arguments?: unknown }>("llm-tool-call", (event) => {
              updateCurrentTurn((turn) => ({
                ...turn,
                blocks: [
                  ...turn.blocks,
                  {
                    kind: "tool",
                    id: event.payload.id,
                    name: event.payload.name,
                    arguments: event.payload.arguments,
                    status: "calling" as const,
                  },
                ],
              }));
              optionsRef.current.onStatusChange?.("acting");
            })
          ),
          track(
            listen<{ id: string; name: string; result: string }>("llm-tool-result", (event) => {
              updateCurrentTurn((turn) => {
                const idx = turn.blocks.findIndex(
                  (b): b is TurnBlock & { kind: "tool" } =>
                    b.kind === "tool" && b.id === event.payload.id
                );
                if (idx >= 0) {
                  const blocks = [...turn.blocks];
                  const existing = blocks[idx];
                  if (existing.kind === "tool") {
                    blocks[idx] = {
                      ...existing,
                      result: event.payload.result,
                      status: "completed",
                    };
                  }
                  return { ...turn, blocks };
                }
                console.warn(
                  `[useChatStream] Received llm-tool-result for id "${event.payload.id}" with no matching llm-tool-call; appending as completed.`,
                  { name: event.payload.name }
                );
                return {
                  ...turn,
                  blocks: [
                    ...turn.blocks,
                    {
                      kind: "tool",
                      id: event.payload.id,
                      name: event.payload.name,
                      result: event.payload.result,
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
              const { id, text, phase } = event.payload;
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
                    blocks: [
                      ...turn.blocks,
                      { kind: "reasoning", id, text, phase: "complete" },
                    ],
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
                  blocks: [
                    ...turn.blocks,
                    { kind: "reasoning", id, text, phase: "delta" },
                  ],
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
      cleanup?.();
    };
  }, [updateCurrentTurn, generateTurnId, clearCurrentTurn]);

  const startStream = useCallback(async (opts: StartStreamOptions) => {
    await invoke("complete_streaming", {
      provider: opts.provider,
      prompt: opts.prompt,
      model: opts.model,
      variant: opts.variant ?? null,
      sessionId: opts.sessionId ?? null,
      invokedSkill: opts.invokedSkill ?? null,
    });
  }, []);

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
    resolveApproval,
  };
}
