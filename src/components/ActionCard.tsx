import { useState } from "react";
import type {
  ActionCard as ActionCardType,
  ActionCardSection,
} from "../types";

interface ActionCardProps {
  card: ActionCardType;
}

const TYPE_ICONS: Record<string, string> = {
  file: "F",
  terminal: ">",
  diff: "±",
  search: "S",
};

const TYPE_ACCENT: Record<string, string> = {
  file: "text-accent-action",
  terminal: "text-accent-data",
  diff: "text-accent-structure",
  search: "text-accent-signal",
};

function renderSection(section: ActionCardSection) {
  if (section.type === "fields") {
    return (
      <section className="grid gap-2" key={`${section.title}-fields`}>
        {section.title && (
          <h4 className="font-mono text-caption font-semibold uppercase tracking-[0.04em] text-text-muted">
            {section.title}
          </h4>
        )}
        <dl className="grid grid-cols-1 gap-2 sm:grid-cols-3">
          {section.fields.map((item) => (
            <div className="min-w-0 rounded-sm bg-surface-overlay p-2" key={`${item.label}-${item.value}`}>
              <dt className="mb-1 font-mono text-caption uppercase tracking-[0.04em] text-text-muted">
                {item.label}
              </dt>
              <dd className="truncate font-mono text-body-sm text-text-primary" title={item.value}>
                {item.value}
              </dd>
            </div>
          ))}
        </dl>
      </section>
    );
  }

  if (section.type === "rows") {
    return (
      <section className="grid gap-2" key={`${section.title}-rows`}>
        {section.title && (
          <h4 className="font-mono text-caption font-semibold uppercase tracking-[0.04em] text-text-muted">
            {section.title}
          </h4>
        )}
        {section.rows.length === 0 ? (
          <p className="m-0 text-body-sm text-text-muted">
            {section.emptyText ?? "No rows returned."}
          </p>
        ) : (
          <ul className="m-0 grid list-none gap-1 p-0">
            {section.rows.map((row, index) => (
              <li
                className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 rounded-sm bg-surface-base px-2 py-1.5 font-mono text-body-sm"
                key={`${row.primary}-${row.meta ?? index}`}
              >
                <span className="min-w-0">
                  <span className="block truncate text-text-primary" title={row.primary}>
                    {row.primary}
                  </span>
                  {row.secondary && (
                    <span className="block truncate text-text-muted" title={row.secondary}>
                      {row.secondary}
                    </span>
                  )}
                </span>
                {row.meta && <span className="text-text-muted">{row.meta}</span>}
              </li>
            ))}
          </ul>
        )}
      </section>
    );
  }

  return (
    <section className="grid gap-2" key={`${section.title}-text`}>
      {section.title && (
        <h4 className="font-mono text-caption font-semibold uppercase tracking-[0.04em] text-text-muted">
          {section.title}
        </h4>
      )}
      <pre
        className={`m-0 max-h-[260px] overflow-auto whitespace-pre-wrap break-words rounded-sm bg-surface-base p-2 text-text-primary ${
          section.monospace ? "font-mono text-mono" : "font-body text-body-sm"
        }`}
      >
        {section.content}
      </pre>
    </section>
  );
}

export function ActionCard({ card }: ActionCardProps) {
  const [expanded, setExpanded] = useState(card.expanded ?? false);
  const [showRaw, setShowRaw] = useState(false);

  const accentClass = TYPE_ACCENT[card.type] || TYPE_ACCENT.file;
  const chevronClass = expanded ? "rotate-180" : "";
  const isRunning = card.status === "calling";
  const hasBody = (card.sections?.length ?? 0) > 0 || card.rawPayload;

  return (
    <section
      className="ml-7 mr-6 overflow-hidden rounded-md border border-surface-overlay bg-surface-elevated animate-fade-slide-in"
      aria-label={card.summary}
    >
      <button
        type="button"
        className="grid min-h-[42px] w-full grid-cols-[auto_minmax(0,1fr)_auto_auto] items-center gap-2 px-3 py-2 text-left text-body-sm text-text-secondary transition-colors duration-150 ease-out-quart hover:bg-surface-overlay disabled:cursor-default"
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
        disabled={!hasBody}
      >
        <span
          className={`flex h-[20px] w-[20px] shrink-0 items-center justify-center rounded-sm bg-surface-overlay font-mono text-caption font-semibold ${accentClass}`}
          aria-hidden="true"
        >
          {TYPE_ICONS[card.type] || TYPE_ICONS.file}
        </span>
        <span className="min-w-0">
          <span className="block truncate font-body text-body-sm font-medium text-text-primary">
            {card.summary}
          </span>
          {card.detail && (
            <span className="block truncate font-mono text-caption text-text-muted" title={card.detail}>
              {card.detail}
            </span>
          )}
        </span>
        {isRunning && (
          <span className="shrink-0 font-mono text-caption text-accent-action">
            Running
          </span>
        )}
        {hasBody && (
          <svg
            className={`shrink-0 text-text-muted transition-transform duration-150 ease-out-quart ${chevronClass}`}
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
        )}
      </button>
      {expanded && hasBody && (
        <div className="grid max-h-[520px] gap-3 overflow-y-auto border-t border-surface-overlay p-3 animate-fade-slide-in-fast">
          {card.sections?.map(renderSection)}
          {card.rawPayload && (
            <section className="grid gap-2">
              <button
                type="button"
                className="justify-self-start rounded-sm border border-surface-overlay px-2 py-1 font-mono text-caption text-text-muted transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary"
                onClick={() => setShowRaw((value) => !value)}
                aria-expanded={showRaw}
              >
                {showRaw ? "Hide raw JSON" : "Show raw JSON"}
              </button>
              {showRaw && (
                <pre className="m-0 max-h-[260px] overflow-auto whitespace-pre-wrap break-words rounded-sm bg-surface-base p-2 font-mono text-mono text-text-primary">
                  {card.rawPayload}
                </pre>
              )}
            </section>
          )}
        </div>
      )}
    </section>
  );
}
