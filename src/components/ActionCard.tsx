import { useState } from "react";
import type { ActionCard as ActionCardType } from "../types";

interface ActionCardProps {
  card: ActionCardType;
}

const TYPE_ICONS: Record<string, string> = {
  file: "F",
  terminal: "▶",
  diff: "±",
  search: "?",
};

const TYPE_BORDER: Record<string, string> = {
  file: "border-l-accent-action",
  terminal: "border-l-accent-data",
  diff: "border-l-accent-structure",
  search: "border-l-accent-signal",
};

export function ActionCard({ card }: ActionCardProps) {
  const [expanded, setExpanded] = useState(card.expanded ?? false);

  const borderClass = TYPE_BORDER[card.type] || TYPE_BORDER.file;
  const chevronClass = expanded ? "rotate-180" : "";
  const isRunning = card.status === "calling";

  return (
    <div className="ml-7 mr-6 rounded-md overflow-hidden bg-surface-elevated animate-fade-slide-in" role="region" aria-label={card.summary}>
      <button
        className={`flex items-center gap-2 w-full py-2 px-3 border-l-2 ${borderClass} text-left text-body-sm text-text-secondary transition-colors duration-150 ease-out-quart min-h-[36px] hover:bg-surface-overlay`}
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
      >
        <span className="font-mono text-caption font-semibold text-text-muted w-[18px] h-[18px] flex items-center justify-center rounded-sm bg-surface-overlay shrink-0" aria-hidden="true">
          {TYPE_ICONS[card.type] || TYPE_ICONS.file}
        </span>
        <span className="flex-1 min-w-0 overflow-hidden text-ellipsis whitespace-nowrap font-mono text-body-sm">{card.summary}</span>
        {isRunning && (
          <span className="shrink-0 font-mono text-caption text-accent-action">
            Running
          </span>
        )}
        <svg
          className={`text-text-muted transition-transform duration-150 ease-out-quart shrink-0 ${chevronClass}`}
          width="12"
          height="12"
          viewBox="0 0 12 12"
          fill="none"
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
      {expanded && card.content && (
        <div className="px-3 pb-3 max-h-[400px] overflow-y-auto animate-fade-slide-in-fast">
          <pre className="font-mono text-mono-lg leading-relaxed text-text-primary whitespace-pre-wrap break-all m-0">{card.content}</pre>
        </div>
      )}
    </div>
  );
}
