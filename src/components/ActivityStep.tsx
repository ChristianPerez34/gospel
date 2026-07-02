import { useState } from "react";
import { Button } from "@/components/ui/button";
import type {
  ActionCard as ActionCardType,
  ActionCardSection,
} from "../types";

interface ActivityStepProps {
  card: ActionCardType;
  className?: string;
}

function classNames(...classes: (string | false | null | undefined)[]) {
  return classes.filter(Boolean).join(" ");
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

const MAX_PREVIEW_LINES = 6;

function PreviewText({
  content,
  monospace,
}: {
  content: string;
  monospace?: boolean;
}) {
  const [showAll, setShowAll] = useState(false);
  const lines = content.split("\n");
  const overflowing = lines.length > MAX_PREVIEW_LINES;
  const visible =
    showAll || !overflowing
      ? content
      : lines.slice(0, MAX_PREVIEW_LINES).join("\n");
  const hiddenCount = lines.length - MAX_PREVIEW_LINES;

  return (
    <div className="grid gap-1">
      <pre
        className={classNames(
          "m-0 max-h-[320px] overflow-auto whitespace-pre-wrap break-words rounded-sm bg-surface-base p-2 text-text-primary",
          monospace ? "font-mono text-mono" : "font-body text-body-sm",
        )}
      >
        {visible}
      </pre>
      {overflowing && (
        <Button
          variant="ghost"
          size="xs"
          className="justify-self-start"
          onClick={() => setShowAll((value) => !value)}
          aria-expanded={showAll}
        >
          {showAll ? "Show less" : `Show ${hiddenCount} more lines`}
        </Button>
      )}
    </div>
  );
}

function renderSection(section: ActionCardSection, keyPrefix = "") {
  if (section.type === "fields") {
    return (
      <section className="grid gap-2" key={`${keyPrefix}${section.title}-fields`}>
        {section.title && (
          <h4 className="font-mono text-caption font-semibold uppercase tracking-[0.04em] text-text-muted">
            {section.title}
          </h4>
        )}
        <dl className="grid grid-cols-1 gap-2 sm:grid-cols-2">
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
      <section className="grid gap-2" key={`${keyPrefix}${section.title}-rows`}>
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
    <section className="grid gap-2" key={`${keyPrefix}${section.title}-text`}>
      {section.title && (
        <h4 className="font-mono text-caption font-semibold uppercase tracking-[0.04em] text-text-muted">
          {section.title}
        </h4>
      )}
      <PreviewText content={section.content} monospace={section.monospace} />
    </section>
  );
}

/**
 * A single node on the tool-activity timeline. The header is the collapsed
 * state (status dot + type glyph + label + target); clicking expands the
 * detail inline below. Merged consecutive calls render each pass in sequence.
 */
export function ActivityStep({ card, className }: ActivityStepProps) {
  const [expanded, setExpanded] = useState(card.expanded ?? false);
  const [showRaw, setShowRaw] = useState(false);

  const accentClass = TYPE_ACCENT[card.type] || TYPE_ACCENT.file;
  const isRunning = card.status === "calling";
  const groupCount = card.groupCount ?? 0;
  const hasBody =
    (card.passes?.length ?? 0) > 0 ||
    (card.sections?.length ?? 0) > 0 ||
    Boolean(card.rawPayload);
  const chevronClass = expanded ? "rotate-180" : "";
  const ariaLabel = classNames(
    card.summary,
    card.target,
    groupCount > 1 && `${groupCount} passes`,
    isRunning ? "Running" : "Done",
  );

  return (
    <li className={classNames("activity-step relative", className)}>
      <button
        type="button"
        className="activity-step-trigger grid min-h-9 w-full grid-cols-[auto_auto_minmax(0,1fr)_auto] items-center gap-2 rounded-sm py-1.5 pl-3 pr-2 text-left text-body-sm text-text-secondary transition-colors duration-150 ease-out-quart hover:bg-surface-overlay focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-accent-action disabled:cursor-default motion-reduce:transition-none"
        onClick={() => setExpanded((value) => !value)}
        aria-expanded={expanded}
        aria-label={ariaLabel}
        disabled={!hasBody}
      >
        <span
          className={classNames(
            "activity-step-dot h-2 w-2 shrink-0 rounded-full",
            isRunning
              ? "bg-accent-action animate-pulse motion-reduce:animate-none"
              : "bg-text-muted",
          )}
          aria-hidden="true"
        />
        <span
          className={`flex h-5 w-4 shrink-0 items-center justify-center font-mono text-caption font-semibold ${accentClass}`}
          aria-hidden="true"
        >
          {TYPE_ICONS[card.type] || TYPE_ICONS.file}
        </span>
        <span className="flex min-w-0 items-baseline gap-2">
          <span
            className="shrink-0 font-mono text-body-sm font-medium text-text-primary"
            data-testid="tool-row-label"
          >
            {card.summary}
          </span>
          {card.target && (
            <span
              className="min-w-0 truncate font-mono text-caption text-text-muted"
              title={card.target}
            >
              {card.target}
            </span>
          )}
          {groupCount > 1 && (
            <span className="shrink-0 rounded-sm bg-surface-overlay px-1 font-mono text-caption text-text-muted">
              {groupCount}×
            </span>
          )}
        </span>
        {hasBody && (
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
        )}
      </button>
      {expanded && hasBody && (
        <div className="activity-step-body ml-6 grid max-h-[520px] gap-3 overflow-y-auto rounded-sm p-3 animate-fade-slide-in-fast motion-reduce:animate-none">
          {card.passes
            ? card.passes.map((pass, index) => (
                <div className="grid gap-2 border-l border-surface-overlay pl-3" key={pass.id}>
                  <h4 className="font-mono text-caption font-semibold uppercase tracking-[0.04em] text-text-muted">
                    Pass {index + 1}
                    {pass.detail ? ` · ${pass.detail}` : ""}
                  </h4>
                  {pass.sections?.map((section) => renderSection(section, `${pass.id}-`))}
                </div>
              ))
            : card.sections?.map((section) => renderSection(section))}
          {!card.passes && card.rawPayload && (
            <section className="grid gap-2">
              <Button
                variant="ghost"
                size="xs"
                className="justify-self-start"
                onClick={() => setShowRaw((value) => !value)}
                aria-expanded={showRaw}
              >
                {showRaw ? "Hide raw JSON" : "Show raw JSON"}
              </Button>
              {showRaw && (
                <pre className="m-0 max-h-[260px] overflow-auto whitespace-pre-wrap break-words rounded-sm bg-surface-base p-2 font-mono text-mono text-text-primary">
                  {card.rawPayload}
                </pre>
              )}
            </section>
          )}
        </div>
      )}
    </li>
  );
}
