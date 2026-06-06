import { useState, useRef, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toolActivitiesToActionCards } from "../toolActivityCards";
import type { Message, AgentStatus, ToolCallActivity } from "../types";

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
}

interface StartStreamOptions {
  provider: string;
  prompt: string;
  model: string;
  sessionId: string | null;
  invokedSkill?: { name: string; args?: string } | null;
}

export function useChatStream(options: UseChatStreamOptions = {}) {
  const [streamingContent, setStreamingContent] = useState("");
  const [toolActivities, setToolActivities] = useState<ToolCallActivity[]>([]);
  const [isThinking, setIsThinking] = useState(false);
  const toolActivitiesRef = useRef<ToolCallActivity[]>([]);
  const optionsRef = useRef(options);
  optionsRef.current = options;

  useEffect(() => {
    let cancelled = false;
    let cleanup: (() => void) | null = null;

    (async () => {
      const unlisteners: (() => void)[] = [];

      try {
        const [u1, u2, u3, u4, u5, u6] = await Promise.all([
          listen<string>("llm-token", (event) => {
            setStreamingContent((prev) => prev + event.payload);
            setIsThinking(false);
          }),
          listen<string>("llm-done", (event) => {
            const content = event.payload;
            const cards = toolActivitiesToActionCards(toolActivitiesRef.current).map(
              (card) => ({ ...card, expanded: false, status: "completed" as const }),
            );

            if (content) {
              optionsRef.current.onMessages?.((prev) => [
                ...prev,
                {
                  id: `m-${Date.now()}`,
                  role: "agent",
                  content,
                  timestamp: new Date(),
                  actionCards: cards.length > 0 ? cards : undefined,
                },
              ]);
            } else if (cards.length > 0) {
              optionsRef.current.onMessages?.((prev) => [
                ...prev,
                {
                  id: `m-${Date.now()}`,
                  role: "agent",
                  content: "Completed.",
                  timestamp: new Date(),
                  actionCards: cards,
                },
              ]);
            }

            setStreamingContent("");
            toolActivitiesRef.current = [];
            setToolActivities([]);
            setIsThinking(false);
            optionsRef.current.onStatusChange?.("connected");
          }),
          listen<{ code: string; message: string }>("llm-error", (event) => {
            const err = event.payload;
            const cards = toolActivitiesToActionCards(toolActivitiesRef.current).map(
              (card) => ({ ...card, expanded: false, status: "completed" as const }),
            );

            if (err?.message || cards.length > 0) {
              optionsRef.current.onMessages?.((prev) => [
                ...prev,
                {
                  id: `m-${Date.now()}`,
                  role: "agent",
                  content: "",
                  timestamp: new Date(),
                  actionCards: cards.length > 0 ? cards : undefined,
                  error: err?.message || "Completion failed.",
                },
              ]);
            }

            setIsThinking(false);
            setStreamingContent("");
            toolActivitiesRef.current = [];
            setToolActivities([]);
            optionsRef.current.onStatusChange?.("error");

            if (err?.code === "API_KEY_MISSING") {
              optionsRef.current.onErrorToast?.(err.message, {
                label: "Open Settings",
                onClick: optionsRef.current.onOpenSettings ?? (() => {}),
              });
            } else {
              optionsRef.current.onErrorToast?.(err?.message || "Completion failed.", {
                label: "Retry",
                onClick: () => {},
              });
            }
          }),
          listen<{ id: string; name: string; arguments?: unknown }>("llm-tool-call", (event) => {
            setToolActivities((prev) => {
              const next = [
                ...prev,
                {
                  id: event.payload.id,
                  name: event.payload.name,
                  arguments: event.payload.arguments,
                  status: "calling" as const,
                },
              ];
              toolActivitiesRef.current = next;
              return next;
            });
            optionsRef.current.onStatusChange?.("acting");
          }),
          listen<{ id: string; name: string; result: string }>("llm-tool-result", (event) => {
            setToolActivities((prev) => {
              const idx = prev.findIndex((a) => a.id === event.payload.id);
              if (idx >= 0) {
                const updated = [...prev];
                updated[idx] = { ...updated[idx], result: event.payload.result, status: "completed" };
                toolActivitiesRef.current = updated;
                return updated;
              }

              const next = [
                ...prev,
                {
                  id: event.payload.id,
                  name: event.payload.name,
                  result: event.payload.result,
                  status: "completed" as const,
                },
              ];
              toolActivitiesRef.current = next;
              return next;
            });
            optionsRef.current.onStatusChange?.("acting");
          }),
          listen<CorpusAutoBuildComplete>("corpus-auto-build-complete", (event) => {
            if (event.payload.success) {
              optionsRef.current.onSuccessToast?.(`Corpus ready with ${event.payload.symbol_count} symbols.`);
            } else {
              optionsRef.current.onErrorToast?.("Corpus auto-build failed. Use Build Corpus to retry.");
            }
          }),
        ]);
        unlisteners.push(u1, u2, u3, u4, u5, u6);
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
    setStreamingContent("");
    toolActivitiesRef.current = [];
    setToolActivities([]);
    setIsThinking(false);
  }, []);

  return {
    streamingContent,
    toolActivities,
    toolActivitiesRef,
    isThinking,
    setIsThinking,
    startStream,
    resetStream,
  };
}
