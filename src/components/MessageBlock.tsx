import type { Message } from "../types";
import "./MessageBlock.css";

interface MessageBlockProps {
  message: Message;
}

export function MessageBlock({ message }: MessageBlockProps) {
  const isUser = message.role === "user";

  const timeStr = message.timestamp.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });

  return (
    <div className={`message-block ${isUser ? "message-block--user" : "message-block--agent"}`}>
      <div className="message-block__header">
        <div
          className={`message-block__avatar ${
            isUser ? "message-block__avatar--user" : "message-block__avatar--agent"
          }`}
          aria-hidden="true"
        >
          {isUser ? "Y" : "G"}
        </div>
        <span className="message-block__name">
          {isUser ? "You" : "Gospel"}
        </span>
        <time className="message-block__time">{timeStr}</time>
      </div>
      <div className="message-block__body">{message.content}</div>
      <div className="message-block__footer">
        <button className="message-block__action" aria-label="Copy message">
          Copy
        </button>
        {!isUser && (
          <>
            <button className="message-block__action" aria-label="Retry message">
              Retry
            </button>
            <button className="message-block__action" aria-label="Fork conversation">
              Fork
            </button>
          </>
        )}
      </div>
    </div>
  );
}