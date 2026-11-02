import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import type { Message } from "../types";

export interface MessageActionCallbacks {
  onRetry?: (messageId: string) => Promise<void>;
  onFork?: (messageId: string) => Promise<void>;
  actionsDisabled?: boolean;
}

export function MessageActions({
  message,
  onRetry,
  onFork,
  actionsDisabled,
  className = "",
}: MessageActionCallbacks & { message: Message; className?: string }) {
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [copied]);

  const copy = async () => {
    setError(null);
    try {
      const text =
        message.blocks
          ?.filter((block) => block.kind === "text")
          .map((block) => block.text)
          .join("\n\n") || message.content;
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      setError("Unable to copy this message. Check clipboard permissions and try again.");
    }
  };

  return (
    <div className={className}>
      <div className="message-actions flex gap-3 opacity-0 transition-opacity duration-150 ease-out-quart pl-1 group-hover:opacity-100 group-focus-within:opacity-100">
        <Button variant="ghost" size="xs" onClick={() => void copy()} aria-label="Copy message">
          {copied ? "Copied" : "Copy"}
        </Button>
        {message.role === "agent" && (
          <>
            <Button
              variant="ghost"
              size="xs"
              aria-label="Retry message"
              disabled={actionsDisabled || !onRetry}
              onClick={() => void onRetry?.(message.id)}
            >
              Retry
            </Button>
            <Button
              variant="ghost"
              size="xs"
              aria-label="Fork conversation"
              disabled={actionsDisabled || !onFork}
              onClick={() => void onFork?.(message.id)}
            >
              Fork
            </Button>
          </>
        )}
      </div>
      {error && (
        <p role="alert" className="text-caption text-status-error">
          {error}
        </p>
      )}
    </div>
  );
}
