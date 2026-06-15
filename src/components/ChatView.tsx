import type { Message, ToolCallActivity } from "../types";
import {
  summarizeLiveToolActivity,
  toolActivitiesToActionCards,
} from "../toolActivityCards";
import { MessageBlock } from "./MessageBlock";
import { ActionCard } from "./ActionCard";

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

export function ChatView({
  messages,
  workspacePath,
  isThinking,
  currentAction,
  toolActivities,
}: ChatViewProps) {
  const isEmpty = messages.length === 0;
  const activities = toolActivities ?? [];
  const liveActionCards = toolActivitiesToActionCards(activities);
  const hasToolActivities = liveActionCards.length > 0;
  const liveStatus = summarizeLiveToolActivity(activities, isThinking);
  const showLiveTurn =
    isThinking ||
    hasToolActivities ||
    typeof currentAction === "string" ||
    typeof currentAction === "object";

  const liveContent =
    typeof currentAction === "object" && currentAction?.type === "streaming"
      ? currentAction.content
      : typeof currentAction === "string"
        ? currentAction
        : hasToolActivities
          ? "Working..."
          : "Thinking...";

  const liveMessage: Message = {
    id: "live-agent-turn",
    role: "agent",
    content: liveContent,
    timestamp: new Date(),
  };

  if (isEmpty) {
    return (
      <div
        className="flex-1 overflow-y-auto overflow-x-hidden flex flex-col items-center justify-center"
        role="main"
      >
        <div className="flex flex-col items-center gap-3 animate-fade-slide-in">
          <p className="font-mono text-body-sm text-text-muted">{workspacePath}</p>
          <p className="text-body text-text-secondary">
            Type a prompt to start a session
          </p>
        </div>
      </div>
    );
  }

  return (
    <div
      className="flex-1 overflow-y-auto overflow-x-hidden flex flex-col relative"
      role="main"
      aria-live="polite"
    >
      {liveStatus && (
        <div className="sticky top-0 z-10 px-4 pt-2 shrink-0">
          <div className="inline-flex max-w-full items-center gap-2 rounded-full border border-surface-overlay bg-surface-base py-1 px-2 text-caption text-text-muted animate-fade-slide-in-fast">
            <span
              className={`h-1.5 w-1.5 rounded-full shrink-0 ${hasToolActivities ? "bg-accent-action animate-pulse" : "bg-text-muted"}`}
              aria-hidden="true"
            />
            <span className="font-mono truncate">{liveStatus}</span>
          </div>
        </div>
      )}
      <div className="flex-1 flex flex-col gap-6 py-6 px-4 max-w-full">
        {messages.map((msg) => (
          <div key={msg.id} className="flex flex-col gap-2 animate-fade-slide-in-fast">
            {msg.content && <MessageBlock message={msg} />}
            {msg.actionCards?.map((card) => (
              <ActionCard key={card.id} card={card} />
            ))}
            {msg.error && (
              <div
                className="ml-7 mr-6 rounded-md border border-status-error bg-surface-elevated px-4 py-3"
                role="alert"
              >
                <div className="flex items-start gap-2 text-body-sm text-text-primary">
                  <span className="mt-1 h-2 w-2 shrink-0 rounded-full bg-status-error" aria-hidden="true" />
                  <span>{msg.error}</span>
                </div>
                <div className="flex gap-2">
                  {/* TODO: wire up handlers when retry/copy functionality is implemented */}
                  {/* <button className="min-h-11 text-caption text-text-muted px-2 rounded-sm transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary">Retry</button>
                  <button className="min-h-11 text-caption text-text-muted px-2 rounded-sm transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary">Copy error</button> */}
                </div>
              </div>
            )}
          </div>
        ))}
        {showLiveTurn && (
          <div className="flex flex-col gap-2 animate-fade-slide-in-fast">
            {liveActionCards.map((card) => (
              <ActionCard key={card.id} card={card} />
            ))}
            <MessageBlock message={liveMessage} showActions={false} />
          </div>
        )}
      </div>
    </div>
  );
}
