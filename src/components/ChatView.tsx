import { useMemo, useState } from "react";
import type {
  CurrentTurn,
  FinalizedToolActivity,
  Message,
  ToolCallActivity,
} from "../types";
import {
  summarizeLiveToolActivity,
  toolActivitiesToActionCards,
} from "../toolActivityCards";
import { MessageBlock } from "./MessageBlock";
import { ActionCard } from "./ActionCard";

interface ChatViewProps {
  messages: Message[];
  workspacePath: string;
  isThinking: boolean;
  currentTurn?: CurrentTurn | null;
  finalizedToolActivities?: FinalizedToolActivity[];
}

interface AgentTurnBlockProps {
  message?: Message;
  currentTurn?: CurrentTurn;
  finalizedActivities?: ToolCallActivity[];
  isThinking: boolean;
}

function toolStatus(activity: ToolCallActivity) {
  return activity.status === "calling" ? "Running" : "Done";
}

function ErrorBlock({ message }: { message: string }) {
  return (
    <div
      className="ml-7 mr-6 rounded-md border border-status-error bg-surface-elevated px-4 py-3"
      role="alert"
    >
      <div className="flex items-start gap-2 text-body-sm text-text-primary">
        <span className="mt-1 h-2 w-2 shrink-0 rounded-full bg-status-error" aria-hidden="true" />
        <span>{message}</span>
      </div>
    </div>
  );
}

function LiveToolActivityList({
  activities,
  isThinking,
}: {
  activities: ToolCallActivity[];
  isThinking: boolean;
}) {
  const [expandedIds, setExpandedIds] = useState<Set<string>>(() => new Set());
  const cardsById = useMemo(() => {
    const cards = toolActivitiesToActionCards(activities);
    return new Map(cards.map((card) => [card.id, card]));
  }, [activities]);
  const liveStatus = summarizeLiveToolActivity(activities, isThinking);

  if (activities.length === 0) return null;

  return (
    <div
      className="ml-7 flex w-[calc(100%-3.25rem)] max-w-[720px] flex-col gap-2"
      data-testid="live-tool-activity-list"
    >
      {liveStatus && (
        <div className="sticky top-2 z-10 inline-flex max-w-full self-start rounded-full border border-surface-overlay bg-surface-base px-2 py-1 text-caption text-text-muted animate-fade-slide-in-fast motion-reduce:animate-none">
          <span className="mr-2 mt-[5px] h-1.5 w-1.5 shrink-0 rounded-full bg-accent-action animate-pulse motion-reduce:animate-none" aria-hidden="true" />
          <span className="truncate font-mono">{liveStatus}</span>
        </div>
      )}
      <ol className="m-0 flex list-none flex-col gap-2 p-0">
        {activities.map((activity) => {
          const card = cardsById.get(activity.id);
          if (!card) return null;
          const expanded = expandedIds.has(activity.id);

          return (
            <li className="grid gap-2" key={activity.id}>
              <button
                type="button"
                className="grid min-h-11 w-full grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 rounded-md border border-surface-overlay bg-surface-elevated px-3 py-2 text-left text-body-sm text-text-secondary transition-colors duration-150 ease-out-quart hover:bg-surface-overlay focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-accent-action motion-reduce:transition-none"
                onClick={() => {
                  setExpandedIds((prev) => {
                    const next = new Set(prev);
                    if (next.has(activity.id)) {
                      next.delete(activity.id);
                    } else {
                      next.add(activity.id);
                    }
                    return next;
                  });
                }}
                aria-expanded={expanded}
                aria-label={`${card.summary} ${toolStatus(activity)}`}
              >
                <span
                  className={`h-2 w-2 rounded-full ${
                    activity.status === "calling" ? "bg-accent-action" : "bg-text-muted"
                  }`}
                  aria-hidden="true"
                />
                <span
                  className="min-w-0 truncate font-body font-medium text-text-primary"
                  data-testid="live-tool-row-label"
                  title={card.summary}
                >
                  {card.summary}
                </span>
                <span className="font-mono text-caption text-text-muted">
                  {toolStatus(activity)}
                </span>
              </button>
              {expanded && (
                <ActionCard
                  card={{ ...card, expanded: true }}
                  className="w-full max-w-[960px]"
                />
              )}
            </li>
          );
        })}
      </ol>
    </div>
  );
}

function FinalizedToolActivityDisclosure({
  activities,
}: {
  activities: ToolCallActivity[];
}) {
  const [expanded, setExpanded] = useState(false);
  const cards = useMemo(() => toolActivitiesToActionCards(activities), [activities]);

  if (activities.length === 0) return null;

  return (
    <div
      className="ml-7 flex w-[calc(100%-3.25rem)] max-w-[720px] flex-col gap-2"
      data-testid="finalized-tool-activity"
    >
      <button
        type="button"
        className="grid min-h-11 w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-3 rounded-md border border-surface-overlay bg-surface-base px-3 py-2 text-left transition-colors duration-150 ease-out-quart hover:bg-surface-overlay focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-accent-action motion-reduce:transition-none"
        aria-expanded={expanded}
        onClick={() => setExpanded((value) => !value)}
      >
        <span className="truncate font-body text-body-sm font-medium text-text-secondary">
          Tool activity ({activities.length})
        </span>
        <span className="font-mono text-caption text-text-muted">
          {expanded ? "Hide" : "Show"}
        </span>
      </button>
      {expanded && (
        <div
          className="grid w-full max-w-[960px] gap-2 animate-fade-slide-in-fast motion-reduce:animate-none"
          data-testid="finalized-tool-activity-cards"
        >
          {cards.map((card) => (
            <ActionCard key={card.id} card={card} className="w-full" />
          ))}
        </div>
      )}
    </div>
  );
}

function AgentTurnBlock({
  message,
  currentTurn,
  finalizedActivities = [],
  isThinking,
}: AgentTurnBlockProps) {
  const turnId = currentTurn?.id ?? message?.id ?? "agent-turn";
  const hasLiveContent = Boolean(currentTurn && (currentTurn.content || currentTurn.toolActivities.length > 0));
  const liveMessage: Message = currentTurn
    ? {
        id: currentTurn.id,
        role: "agent",
        content: currentTurn.content || (isThinking ? "Thinking..." : "Working..."),
        timestamp: new Date(),
      }
    : {
        id: turnId,
        role: "agent",
        content: isThinking ? "Thinking..." : "Working...",
        timestamp: new Date(),
      };

  return (
    <div
      className="flex flex-col gap-2 animate-fade-slide-in-fast motion-reduce:animate-none"
      data-testid={`agent-turn-${turnId}`}
    >
      {currentTurn ? (
        <>
          <LiveToolActivityList
            activities={currentTurn.toolActivities}
            isThinking={isThinking}
          />
          <MessageBlock message={liveMessage} showActions={false} />
        </>
      ) : (
        <>
          {message?.content && <MessageBlock message={message} />}
          {message?.error && <ErrorBlock message={message.error} />}
          <FinalizedToolActivityDisclosure activities={finalizedActivities} />
          {!hasLiveContent && isThinking && <MessageBlock message={liveMessage} showActions={false} />}
        </>
      )}
    </div>
  );
}

export function ChatView({
  messages,
  workspacePath,
  isThinking,
  currentTurn,
  finalizedToolActivities = [],
}: ChatViewProps) {
  const isEmpty = messages.length === 0 && !currentTurn;
  const visibleTurns = useMemo(() => {
    const turns: Array<
      | { type: "message"; message: Message }
      | { type: "currentTurn"; currentTurn: CurrentTurn }
    > = messages.map((message) => ({ type: "message", message }));
    if (currentTurn) {
      turns.push({ type: "currentTurn", currentTurn });
    }
    return turns;
  }, [messages, currentTurn]);
  const finalizedByMessageId = useMemo(() => {
    return new Map(
      finalizedToolActivities.map((item) => [item.messageId, item.activities]),
    );
  }, [finalizedToolActivities]);

  if (isEmpty) {
    return (
      <div
        className="flex-1 overflow-y-auto overflow-x-hidden flex flex-col items-center justify-center"
        role="main"
      >
        <div className="flex flex-col items-center gap-3 animate-fade-slide-in motion-reduce:animate-none">
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
      <div className="flex-1 flex flex-col gap-6 py-6 px-4 max-w-full">
        {visibleTurns.map((turn) => {
          if (turn.type === "currentTurn") {
            return (
              <AgentTurnBlock
                key={turn.currentTurn.id}
                currentTurn={turn.currentTurn}
                isThinking={isThinking}
              />
            );
          }

          const msg = turn.message;
          return msg.role === "agent" ? (
            <AgentTurnBlock
              key={msg.id}
              message={msg}
              finalizedActivities={finalizedByMessageId.get(msg.id) ?? []}
              isThinking={isThinking}
            />
          ) : (
            <div key={msg.id} className="flex flex-col gap-2 animate-fade-slide-in-fast motion-reduce:animate-none">
              <MessageBlock message={msg} />
            </div>
          );
        })}
      </div>
    </div>
  );
}
