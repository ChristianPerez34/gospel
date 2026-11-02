import { Streamdown } from "streamdown";
import { codePlugin } from "@/lib/markdown";
import type { Message } from "../types";
import { MessageActions } from "./MessageActions";

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
  const avatarBg = isUser
    ? "user-avatar bg-surface-overlay text-text-secondary"
    : "agent-avatar bg-accent-action text-text-inverse";
  const bodyBg = isUser ? "bg-surface-overlay" : "bg-surface-base border border-surface-overlay";
  const bodyRole = isUser ? "message-body-user" : "message-body-agent";

  return (
    <div className={`message-block flex flex-col gap-2 max-w-[680px] w-full ${alignClass} group`}>
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
      <div
        className={`message-body ${bodyRole} text-body leading-relaxed text-text-primary rounded-lg break-words ${bodyBg} ${isUser ? "px-3 py-3" : "px-3 py-3 prose"}`}
      >
        {isUser ? (
          message.content
        ) : (
          <Streamdown plugins={{ code: codePlugin }}>{message.content}</Streamdown>
        )}
      </div>
      {showActions && <MessageActions message={message} />}
    </div>
  );
}
