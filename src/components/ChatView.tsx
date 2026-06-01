import { Streamdown } from "streamdown";
import { code } from "@streamdown/code";
import type { Message, ToolCallActivity } from "../types";
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

function ToolActivityIndicator({ activity }: { activity: ToolCallActivity }) {
  const toolLabels: Record<string, string> = {
    read_file: "Reading file",
    search_code: "Searching code",
    find_files: "Finding files",
    list_directory: "Listing directory",
    delegate_exploration: "Exploration Agent investigating",
  };
  const displayName = activity.name
    .replace(/_/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase());
  const isCalling = activity.status === "calling";
  const baseLabel = toolLabels[activity.name] ?? displayName;
  const label = `${baseLabel}${isCalling ? "..." : ""}`;

  const statusColor = isCalling ? "text-accent-action" : "text-text-muted";
  const iconAnim = isCalling ? "animate-spin" : "";

  return (
    <div className={`flex items-center gap-2 py-1 px-2 rounded-sm text-caption bg-surface-elevated animate-fade-slide-in-fast ${statusColor}`}>
      <span className={`shrink-0 w-3.5 text-center ${iconAnim}`}>
        {isCalling ? "⟳" : "✓"}
      </span>
      <span className="font-mono">
        {label}
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
      <div className="flex-1 overflow-y-auto overflow-x-hidden flex flex-col items-center justify-center" role="main">
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
    <div className="flex-1 overflow-y-auto overflow-x-hidden flex flex-col relative" role="main" aria-live="polite">
      {(isThinking || hasToolActivities) && (
        <div className="overflow-hidden shrink-0 py-2 px-4">
          {hasToolActivities && (
            <div className="flex flex-col gap-1">
              {toolActivities!.map((activity, i) => (
                <ToolActivityIndicator key={`${activity.name}-${i}`} activity={activity} />
              ))}
            </div>
          )}
          {isThinking && !hasToolActivities && (
            <div className="prose text-body leading-relaxed text-text-primary py-3 px-4 rounded-md bg-surface-elevated border-l-2 border-l-accent-action max-h-[200px] overflow-y-auto">
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
      <div className="flex-1 flex flex-col gap-6 py-6 px-4 max-w-full">
        {messages.map((msg) => (
          <div key={msg.id} className="flex flex-col gap-2 animate-fade-slide-in-fast">
            <MessageBlock message={msg} />
            {msg.actionCards?.map((card) => (
              <ActionCard key={card.id} card={card} />
            ))}
            {msg.error && (
              <div className="ml-7 border-l-2 border-l-status-error py-3 px-4 rounded-r-md bg-surface-elevated" role="alert">
                <div className="text-body-sm text-text-primary mb-2">
                  {msg.error}
                </div>
                <div className="flex gap-2">
                  {/* TODO: wire up handlers when retry/copy functionality is implemented */}
                  {/* <button className="text-caption text-text-muted py-0.5 px-2 rounded-sm transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary">Retry</button>
                  <button className="text-caption text-text-muted py-0.5 px-2 rounded-sm transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary">Copy error</button> */}
                </div>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
