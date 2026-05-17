import type { Message } from "../types";
import { MessageBlock } from "./MessageBlock";
import { ActionCard } from "./ActionCard";
import "./ChatView.css";

interface ChatViewProps {
  messages: Message[];
  workspacePath: string;
  isThinking: boolean;
  currentAction?: string;
  statusText?: string;
}

export function ChatView({
  messages,
  workspacePath,
  isThinking,
  currentAction,
}: ChatViewProps) {
  const isEmpty = messages.length === 0;

  if (isEmpty) {
    return (
      <div className="chat-view chat-view--empty" role="main">
        <div className="chat-view__empty">
          <p className="chat-view__empty-path">{workspacePath}</p>
          <p className="chat-view__empty-prompt">
            Type a prompt to start a session
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="chat-view" role="main" aria-live="polite">
      <div className="chat-view__thinking-bar">
        {isThinking && (
          <span className="chat-view__thinking-text">
            {currentAction || "Thinking..."}
          </span>
        )}
      </div>
      <div className="chat-view__messages">
        {messages.map((msg) => (
          <div key={msg.id} className="chat-view__message-wrapper">
            <MessageBlock message={msg} />
            {msg.actionCards?.map((card) => (
              <ActionCard key={card.id} card={card} />
            ))}
            {msg.error && (
              <div className="chat-view__error" role="alert">
                <div className="chat-view__error-content">
                  {msg.error}
                </div>
                <div className="chat-view__error-actions">
                  <button className="chat-view__error-btn">Retry</button>
                  <button className="chat-view__error-btn">Copy error</button>
                </div>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}