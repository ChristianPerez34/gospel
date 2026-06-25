import { useCallback } from "react";
import { Streamdown } from "streamdown";
import { code } from "@streamdown/code";
import { Button } from "@/components/ui/button";
import type { Message } from "../types";

interface MessageBlockProps {
  message: Message;
  showActions?: boolean;
}

export function MessageBlock({ message, showActions = true }: MessageBlockProps) {
  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(message.content);
  }, [message.content]);
  const isUser = message.role === "user";

  const timeStr = message.timestamp.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });

  const alignClass = isUser ? "self-end" : "self-start";
  const avatarBg = isUser ? "bg-surface-overlay text-text-secondary" : "bg-accent-action text-text-inverse";
  const bodyBg = isUser ? "bg-surface-overlay" : "bg-surface-base border border-surface-overlay";

  return (
    <div className={`flex flex-col gap-2 max-w-[680px] w-full ${alignClass} group`}>
      <div className="flex items-center gap-2">
        <div
          className={`w-[22px] h-[22px] rounded-full flex items-center justify-center font-body text-caption font-semibold shrink-0 ${avatarBg}`}
          aria-hidden="true"
        >
          {isUser ? "Y" : "G"}
        </div>
        <span className="text-body-sm font-medium text-text-secondary">
          {isUser ? "You" : "Gospel"}
        </span>
        <time className="font-mono text-caption text-text-muted tracking-[0.02em]">{timeStr}</time>
      </div>
      <div className={`text-body leading-relaxed text-text-primary rounded-lg break-words ${bodyBg} ${isUser ? "px-3 py-3" : "px-3 py-3 prose"}`}>
        {isUser ? (
          message.content
        ) : (
          <Streamdown animated plugins={{ code }}>
            {message.content}
          </Streamdown>
        )}
      </div>
      {showActions && (
        <div className="flex gap-3 opacity-0 transition-opacity duration-150 ease-out-quart pl-1 group-hover:opacity-100 group-focus-within:opacity-100">
          <Button variant="ghost" size="xs" onClick={handleCopy} aria-label="Copy message">
            Copy
          </Button>
          {!isUser && (
            <>
              <Button variant="ghost" size="xs" aria-label="Retry message">
                Retry
              </Button>
              <Button variant="ghost" size="xs" aria-label="Fork conversation">
                Fork
              </Button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
