import { useState, useRef, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AgentStatus,
  CurrentTurn,
  FinalizedToolActivity,
  Message,
} from "../types";

interface CorpusAutoBuildComplete {
  success: boolean;
  symbol_count: number;
}

interface UseChatStreamOptions {
  onMessages?: React.Dispatch<React.SetStateAction<Message[]>>;
  onFinalizeToolActivities?: (toolActivity: FinalizedToolActivity) => void;
  onStatusChange?: (status: AgentStatus) => void;
  onErrorToast?: (message: string, action?: { label: string; onClick: () => void }) => void;
  onSuccessToast?: (message: string) => void;
  onOpenSettings?: () => void;
  onRetry?: () => void;
}

interface LlmDonePayloadObject {
  response: string;
  prompt_tokens?: number;
  response_tokens?: number;
  tool_calls?: number;
}

type LlmDonePayload = string | LlmDonePayloadObject;

interface StartStreamOptions {
  provider: string;
  prompt: string;
  model: string;
  sessionId: string | null;
  invokedSkill?: { name: string; args?: string } | null;
}

export function useChatStream(options: UseChatStreamOptions = {}) {
  const [currentTurn, setCurrentTurn] = useState<CurrentTurn | null>(null);
  const currentTurnRef = useRef<CurrentTurn | null>(null);
  const turnSequenceRef = useRef(0);
  const optionsRef = useRef(options);
  optionsRef.current = options;

  const createTurn = useCallback((): CurrentTurn => {
    turnSequenceRef.current += 1;
    return {
      id: `turn-${Date.now()}-${turnSequenceRef.current}`,
      content: "",
      toolActivities: [],
      createdAt: new Date(),
    };
  }, []);

  const updateCurrentTurn = useCallback(
    (updater: (turn: CurrentTurn) => CurrentTurn) => {
      const existing = currentTurnRef.current ?? createTurn();
      const next = updater(existing);
      currentTurnRef.current = next;
      setCurrentTurn(next);
      return next;
    },
    [createTurn],
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

      const track = (p: Promise<() => void>) => p.then((u) => { unlisteners.push(u); return u; });
      try {
        await Promise.all([
          track(listen<string>("llm-token", (event) => {
            updateCurrentTurn((turn) => ({
              ...turn,
              content: turn.content + event.payload,
            }));
          })),
          track(listen<LlmDonePayload>("llm-done", (event) => {
            const payload = event.payload;
            const finalTurn = currentTurnRef.current;
            const payloadContent =
              typeof payload === "string"
                ? payload
                : payload?.response ?? "";
            const content = payloadContent || finalTurn?.content || "";
            const messageId = finalTurn?.id ?? createTurn().id;
            const activities = finalTurn?.toolActivities ?? [];

            if (content || activities.length > 0) {
              optionsRef.current.onMessages?.((prev) => [
                ...prev,
                {
                  id: messageId,
                  role: "agent",
                  content: content || "Completed.",
                  timestamp: new Date(),
                },
              ]);
            }

            if (activities.length > 0) {
              optionsRef.current.onFinalizeToolActivities?.({
                messageId,
                activities,
              });
            }

            clearCurrentTurn();
            optionsRef.current.onStatusChange?.("connected");
          })),
          track(listen<{ code: string; message: string }>("llm-error", (event) => {
            const err = event.payload;
            const finalTurn = currentTurnRef.current;
            const messageId = finalTurn?.id ?? createTurn().id;
            const activities = finalTurn?.toolActivities ?? [];

            if (err?.message || finalTurn?.content || activities.length > 0) {
              optionsRef.current.onMessages?.((prev) => [
                ...prev,
                {
                  id: messageId,
                  role: "agent",
                  content: finalTurn?.content ?? "",
                  timestamp: new Date(),
                  error: err?.message || "Completion failed.",
                },
              ]);
            }

            if (activities.length > 0) {
              optionsRef.current.onFinalizeToolActivities?.({
                messageId,
                activities,
              });
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
          })),
          track(listen<{ id: string; name: string; arguments?: unknown }>("llm-tool-call", (event) => {
            updateCurrentTurn((turn) => ({
              ...turn,
              toolActivities: [
                ...turn.toolActivities,
                {
                  id: event.payload.id,
                  name: event.payload.name,
                  arguments: event.payload.arguments,
                  status: "calling" as const,
                },
              ],
            }));
            optionsRef.current.onStatusChange?.("acting");
          })),
          track(listen<{ id: string; name: string; result: string }>("llm-tool-result", (event) => {
            updateCurrentTurn((turn) => {
              const idx = turn.toolActivities.findIndex((a) => a.id === event.payload.id);
              if (idx >= 0) {
                const updated = [...turn.toolActivities];
                updated[idx] = { ...updated[idx], result: event.payload.result, status: "completed" };
                return {
                  ...turn,
                  toolActivities: updated,
                };
              }
              console.warn(
                `[useChatStream] Received llm-tool-result for id "${event.payload.id}" with no matching llm-tool-call; appending as completed.`,
                { name: event.payload.name },
              );
              return {
                ...turn,
                toolActivities: [
                  ...turn.toolActivities,
                  {
                    id: event.payload.id,
                    name: event.payload.name,
                    result: event.payload.result,
                    status: "completed" as const,
                  },
                ],
              };
            });
            optionsRef.current.onStatusChange?.("acting");
          })),
          track(listen<CorpusAutoBuildComplete>("corpus-auto-build-complete", (event) => {
            if (event.payload.success) {
              optionsRef.current.onSuccessToast?.(`Corpus ready with ${event.payload.symbol_count} symbols.`);
            } else {
              optionsRef.current.onErrorToast?.("Corpus auto-build failed. Use Build Corpus to retry.");
            }
          })),
        ]);
      } catch (error) {
        unlisteners.forEach((unlisten) => unlisten());
        throw error;
      }

      cleanup = () => {
        unlisteners.forEach((unlisten) => unlisten());
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
  }, []);

  const startStream = useCallback(async (opts: StartStreamOptions) => {
    await invoke("complete_streaming", {
      provider: opts.provider,
      prompt: opts.prompt,
      model: opts.model,
      sessionId: opts.sessionId ?? null,
      invokedSkill: opts.invokedSkill ?? null,
    });
  }, []);

  const resetStream = useCallback(() => {
    clearCurrentTurn();
  }, [clearCurrentTurn]);

  return {
    currentTurn,
    startStream,
    resetStream,
  };
}
