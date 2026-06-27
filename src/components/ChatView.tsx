import { useMemo, useRef, useState } from "react";
import { Streamdown } from "streamdown";
import { code } from "@streamdown/code";
import { Button } from "@/components/ui/button";
import type {
  ActionCardType,
  CurrentTurn,
  Message,
  ToolCallActivity,
  TurnBlock,
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
}

interface AgentTurnBlockProps {
  message?: Message;
  currentTurn?: CurrentTurn;
  isThinking: boolean;
}

type TextTurnBlock = Extract<TurnBlock, { kind: "text" }>;
type ToolTurnBlock = Extract<TurnBlock, { kind: "tool" }>;

const TOOL_TYPE_ICONS: Record<ActionCardType, string> = {
  file: "F",
  terminal: ">",
  diff: "+/-",
  search: "S",
};

function classNames(...classes: (string | false | null | undefined)[]) {
  return classes.filter(Boolean).join(" ");
}

function toolBlockToActivity(block: ToolTurnBlock): ToolCallActivity {
  return {
    id: block.id,
    name: block.name,
    arguments: block.arguments,
    result: block.result,
    status: block.status,
  };
}

function toolStatus(block: ToolTurnBlock) {
  return block.status === "calling" ? "Running" : "Done";
}

function ErrorBlock({ message }: { message: string }) {
  return (
    <div
      className="agent-error-card ml-16 rounded-lg border border-status-error bg-surface-elevated px-3 py-3"
      role="alert"
    >
      <div className="flex items-start gap-2 text-body-sm text-text-primary">
        <span className="mt-1.5 h-2 w-2 shrink-0 rounded-full bg-status-error" aria-hidden="true" />
        <span>{message}</span>
      </div>
    </div>
  );
}

function RunningPill({
  toolBlocks,
}: {
  toolBlocks: ToolTurnBlock[];
}) {
  const activities = useMemo(
    () => toolBlocks.map(toolBlockToActivity),
    [toolBlocks],
  );
  const hasRunningTool = activities.some((activity) => activity.status === "calling");
  const liveStatus = hasRunningTool ? summarizeLiveToolActivity(activities, false) : null;

  if (!liveStatus) return null;

  return (
    <div className="running-pill sticky top-2 z-10 inline-flex max-w-full self-start rounded-full border border-surface-overlay bg-surface-base px-2 py-1 text-caption text-text-muted animate-fade-slide-in-fast motion-reduce:animate-none">
      <span className="mr-2 mt-[5px] h-1.5 w-1.5 shrink-0 rounded-full bg-accent-action animate-pulse motion-reduce:animate-none" aria-hidden="true" />
      <span className="truncate font-mono">{liveStatus}</span>
    </div>
  );
}

function ToolRow({
  block,
  defaultExpanded = false,
}: {
  block: ToolTurnBlock;
  defaultExpanded?: boolean;
}) {
  const [expanded, setExpanded] = useState(defaultExpanded);
  const activity = useMemo(() => toolBlockToActivity(block), [block]);
  const card = useMemo(
    () => toolActivitiesToActionCards([activity])[0],
    [activity],
  );

  if (!card) return null;

  const isRunning = block.status === "calling";
  const icon = TOOL_TYPE_ICONS[card.type] ?? TOOL_TYPE_ICONS.file;
  const chevronClass = expanded ? "rotate-180" : "";

  return (
    <li className="grid gap-2" key={block.id}>
      <button
        type="button"
        className="tool-row-button grid min-h-11 w-full max-w-[720px] grid-cols-[auto_auto_minmax(0,1fr)_auto_auto] items-center gap-2 rounded-md border border-surface-overlay bg-surface-elevated px-3 py-2 text-left text-body-sm text-text-secondary transition-colors duration-150 ease-out-quart hover:bg-surface-overlay focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-accent-action motion-reduce:transition-none"
        onClick={() => setExpanded((value) => !value)}
        aria-expanded={expanded}
        aria-label={`${card.summary}${card.detail ? `: ${card.detail}` : ""} ${toolStatus(block)}`}
      >
        <span
          className={classNames(
            "h-2 w-2 rounded-full",
            isRunning
              ? "bg-accent-action animate-pulse motion-reduce:animate-none"
              : "bg-text-muted",
          )}
          aria-hidden="true"
        />
        <span
          className="flex h-5 min-w-5 shrink-0 items-center justify-center font-mono text-caption font-semibold text-text-muted"
          aria-hidden="true"
        >
          {icon}
        </span>
        <span
          className="min-w-0 truncate font-body font-medium text-text-primary"
          data-testid="tool-row-label"
          title={card.summary}
        >
          {card.summary}
        </span>
        {isRunning && (
          <span className="shrink-0 font-mono text-caption text-accent-action">
            Running
          </span>
        )}
        <svg
          className={`shrink-0 text-text-muted transition-transform duration-150 ease-out-quart motion-reduce:transition-none ${chevronClass}`}
          width="12"
          height="12"
          viewBox="0 0 12 12"
          fill="none"
          aria-hidden="true"
        >
          <path
            d="M4 4.5L6 6.5L8 4.5"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </button>
      {expanded && (
        <ActionCard
          card={{ ...card, expanded: true }}
          className="w-full max-w-[960px]"
        />
      )}
    </li>
  );
}

function ToolTimeline({ blocks }: { blocks: ToolTurnBlock[] }) {
  if (blocks.length === 0) return null;

  return (
    <div
      className="tool-timeline ml-16 flex w-[calc(100%-3.25rem)] max-w-[960px] flex-col gap-2"
      data-testid="inline-tool-activity-list"
    >
      <ol className="m-0 flex list-none flex-col gap-2 p-0">
        {blocks.map((block) => (
          <ToolRow block={block} key={block.id} />
        ))}
      </ol>
    </div>
  );
}

function AgentTextBlock({ block }: { block: TextTurnBlock }) {
  if (!block.text) return null;

  return (
    <div className="agent-text-block ml-16 w-[calc(100%-3.25rem)] max-w-[680px] rounded-lg border border-surface-overlay bg-surface-base px-3 py-3 text-body leading-relaxed text-text-primary break-words prose">
      <Streamdown animated plugins={{ code }}>
        {block.text}
      </Streamdown>
    </div>
  );
}

function AgentHeader({ timestamp }: { timestamp: Date }) {
  const timeStr = timestamp.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });

  return (
    <div className="flex items-center gap-2">
      <div
        className="agent-avatar w-[22px] h-[22px] rounded-full flex items-center justify-center font-body text-caption font-semibold shrink-0 bg-accent-action text-text-inverse"
        aria-hidden="true"
      >
        G
      </div>
      <span className="text-body-sm font-medium text-text-secondary">Gospel</span>
      <time className="font-mono text-caption text-text-muted tracking-[0.02em]">{timeStr}</time>
    </div>
  );
}

function AgentActions() {
  return (
    <div className="message-actions ml-16 flex gap-3 opacity-0 transition-opacity duration-150 ease-out-quart pl-1 group-hover:opacity-100 group-focus-within:opacity-100">
      <Button variant="ghost" size="xs" aria-label="Copy message">
        Copy
      </Button>
      <Button variant="ghost" size="xs" aria-label="Retry message">
        Retry
      </Button>
      <Button variant="ghost" size="xs" aria-label="Fork conversation">
        Fork
      </Button>
    </div>
  );
}

function AgentTurnBlock({ message, currentTurn, isThinking }: AgentTurnBlockProps) {
  const turnId = currentTurn?.id ?? message?.id ?? "agent-turn";
  const fallbackTimestampRef = useRef<Date | null>(null);
  if (!fallbackTimestampRef.current) {
    fallbackTimestampRef.current = new Date();
  }
  const timestamp =
    currentTurn?.createdAt ?? message?.timestamp ?? fallbackTimestampRef.current;
  const blocks =
    currentTurn?.blocks ??
    message?.blocks ??
    (message?.content
      ? [{ kind: "text" as const, id: `${turnId}-legacy-text`, text: message.content }]
      : []);
  const visibleBlocks =
    blocks.length > 0 || !isThinking
      ? blocks
      : [{ kind: "text" as const, id: `${turnId}-thinking`, text: "Thinking..." }];
  const isLive = Boolean(currentTurn);
  const toolBlocks = visibleBlocks.filter(
    (block): block is ToolTurnBlock => block.kind === "tool",
  );
  const showActions = Boolean(message && !currentTurn);

  return (
    <div
      className="agent-turn group flex flex-col gap-2 animate-fade-slide-in-fast motion-reduce:animate-none"
      data-testid={`agent-turn-${turnId}`}
    >
      <AgentHeader timestamp={timestamp} />
      {isLive && toolBlocks.some((block) => block.status === "calling") && (
        <div className="ml-16 flex w-[calc(100%-3.25rem)] max-w-[960px] flex-col">
          <RunningPill toolBlocks={toolBlocks} />
        </div>
      )}
      {visibleBlocks.map((block) =>
        block.kind === "text" ? (
          <AgentTextBlock block={block} key={block.id} />
        ) : (
          <ToolTimeline blocks={[block]} key={block.id} />
        ),
      )}
      {message?.error && <ErrorBlock message={message.error} />}
      {showActions && <AgentActions />}
    </div>
  );
}

export function ChatView({
  messages,
  workspacePath,
  isThinking,
  currentTurn,
}: ChatViewProps) {
  const isEmpty = messages.length === 0 && !currentTurn && !isThinking;
  const visibleTurns = useMemo(() => {
    const turns: Array<
      | { type: "message"; message: Message }
      | { type: "currentTurn"; currentTurn: CurrentTurn }
      | { type: "thinking" }
    > = messages.map((message) => ({ type: "message", message }));
    if (currentTurn) {
      turns.push({ type: "currentTurn", currentTurn });
    } else if (isThinking) {
      const lastMessage = messages[messages.length - 1];
      if (!lastMessage || lastMessage.role !== "agent") {
        turns.push({ type: "thinking" });
      }
    }
    return turns;
  }, [messages, currentTurn, isThinking]);

  if (isEmpty) {
    return (
      <div
        className="chat-view chat-empty-state flex-1 overflow-y-auto overflow-x-hidden flex flex-col items-center justify-center"
        role="main"
      >
        <div className="flex flex-col items-center gap-2 animate-fade-slide-in motion-reduce:animate-none">
          <p className="text-heading text-text-primary font-medium">New session ready</p>
          <p className="font-mono text-mono text-text-muted">{workspacePath}</p>
        </div>
      </div>
    );
  }

  return (
    <div
      className="chat-view flex-1 overflow-y-auto overflow-x-hidden flex flex-col relative"
      role="main"
      aria-live="polite"
    >
      <div className="chat-feed flex-1 flex flex-col gap-5 py-5 px-8 max-w-full">
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

          if (turn.type === "thinking") {
            return (
              <AgentTurnBlock
                key="thinking-placeholder"
                message={{
                  id: "thinking-placeholder",
                  role: "agent",
                  content: "",
                  timestamp: new Date(0),
                }}
                isThinking={isThinking}
              />
            );
          }

          const msg = turn.message;
          return msg.role === "agent" ? (
            <AgentTurnBlock
              key={msg.id}
              message={msg}
              isThinking={isThinking}
            />
          ) : (
            <div key={msg.id} className="flex flex-col gap-1.5 animate-fade-slide-in-fast motion-reduce:animate-none">
              <MessageBlock message={msg} />
            </div>
          );
        })}
      </div>
    </div>
  );
}
