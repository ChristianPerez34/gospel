import { Streamdown } from "streamdown";
import { code } from "@streamdown/code";
import type { Message } from "../types";

interface MessageBlockProps {
  message: Message;
  showActions?: boolean;
}

export function MessageBlock({ message, showActions = true }: MessageBlockProps) {
  const isUser = message.role === "user";

  const timeStr = message.timestamp.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });

  const alignClass = isUser ? "self-end" : "self-start";
  const avatarBg = isUser ? "bg-surface-overlay text-text-secondary" : "bg-accent-action text-text-inverse";
  const bodyBg = isUser ? "bg-surface-overlay" : "bg-surface-elevated";

  return (
    <div className={`flex flex-col gap-1 max-w-[720px] w-full ${alignClass} group`}>
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
      <div className={`text-body leading-relaxed text-text-primary py-3 px-4 rounded-md break-words ${bodyBg} ${isUser ? "" : "prose"}`}>
        {isUser ? (
          message.content
        ) : (
          <Streamdown animated plugins={{ code }}>
            {message.content}
          </Streamdown>
        )}
      </div>
      {showActions && (
        <div className="flex gap-1 opacity-0 transition-opacity duration-150 ease-out-quart pl-1 group-hover:opacity-100 group-focus-within:opacity-100">
          <button className="min-h-11 text-caption text-text-muted px-2 rounded-sm transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-accent-action" aria-label="Copy message">
            Copy
          </button>
          {!isUser && (
            <>
              <button className="min-h-11 text-caption text-text-muted px-2 rounded-sm transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-accent-action" aria-label="Retry message">
                Retry
              </button>
              <button className="min-h-11 text-caption text-text-muted px-2 rounded-sm transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-accent-action" aria-label="Fork conversation">
                Fork
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
