import { Streamdown } from "streamdown";
import { code } from "@streamdown/code";
import type { Message, ToolCallActivity } from "../types";
import { MessageBlock } from "./MessageBlock";
import { ActionCard } from "./ActionCard";
import "./ChatView.css";

interface StreamingAction {
  type: "streaming";
  content: string;
}

interface ChatViewProps {
  messages: Message[];
  workspacePath: string;
  isThinking: boolean;
  currentAction?: string | StreamingAction;
  statusText?: string;
  toolActivities?: ToolCallActivity[];
}

function ToolActivityIndicator({ activity }: { activity: ToolCallActivity }) {
  const displayName = activity.name
    .replace(/_/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase());
  const isCalling = activity.status === "calling";

  return (
    <div className={`tool-activity tool-activity--${activity.status}`}>
      <span className="tool-activity__icon">{isCalling ? "⟳" : "✓"}</span>
      <span className="tool-activity__label">
        {isCalling ? `Searching ${displayName}...` : displayName}
      </span>
    </div>
  );
}

export function ChatView({
  messages,
  workspacePath,
  isThinking,
  currentAction,
  toolActivities,
}: ChatViewProps) {
  const isEmpty = messages.length === 0;
  const hasToolActivities = toolActivities && toolActivities.length > 0;

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
      {(isThinking || hasToolActivities) && (
        <div className="chat-view__thinking-bar">
          {hasToolActivities && (
            <div className="chat-view__tool-activities">
              {toolActivities!.map((activity, i) => (
                <ToolActivityIndicator key={`${activity.name}-${i}`} activity={activity} />
              ))}
            </div>
          )}
          {isThinking && !hasToolActivities && (
            <div className="chat-view__thinking-text prose">
              <Streamdown animated isAnimating={isThinking} plugins={{ code }}>
                {typeof currentAction === "object" && currentAction?.type === "streaming"
                  ? currentAction.content
                  : typeof currentAction === "string"
                  ? currentAction
                  : "Thinking..."}
              </Streamdown>
            </div>
          )}
        </div>
      )}
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