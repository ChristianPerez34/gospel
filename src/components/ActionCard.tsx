import { useState } from "react";
import type { ActionCard as ActionCardType } from "../types";
import "./ActionCard.css";

interface ActionCardProps {
  card: ActionCardType;
}

const TYPE_ICONS: Record<string, string> = {
  file: "F",
  terminal: "▶",
  diff: "±",
  search: "?",
};

const TYPE_ACCENT: Record<string, string> = {
  file: "var(--accent-action)",
  terminal: "var(--accent-data)",
  diff: "var(--accent-structure)",
  search: "var(--accent-signal)",
};

export function ActionCard({ card }: ActionCardProps) {
  const [expanded, setExpanded] = useState(card.expanded ?? false);

  return (
    <div className="action-card" role="region" aria-label={card.summary}>
      <button
        className="action-card__header"
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
        style={{ borderLeftColor: TYPE_ACCENT[card.type] || TYPE_ACCENT.file }}
      >
        <span className="action-card__icon" aria-hidden="true">
          {TYPE_ICONS[card.type] || TYPE_ICONS.file}
        </span>
        <span className="action-card__summary">{card.summary}</span>
        <svg
          className={`action-card__chevron ${expanded ? "action-card__chevron--open" : ""}`}
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
        <div className="action-card__content">
          <pre className="action-card__code">{card.content}</pre>
        </div>
      )}
    </div>
  );
}