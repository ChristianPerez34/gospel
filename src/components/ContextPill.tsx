interface ContextPillProps {
  name: string;
  onRemove: () => void;
}

export function ContextPill({ name, onRemove }: ContextPillProps) {
  return (
    <span className="inline-flex items-center gap-1 py-1 px-2 bg-surface-elevated border border-surface-overlay rounded-lg font-mono text-body-sm text-text-secondary whitespace-nowrap shrink-0 group">
      <svg
        className="shrink-0 text-text-muted"
        width="12"
        height="12"
        viewBox="0 0 12 12"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
      >
        <path d="M6 1v10M2 5l4-4 4 4" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      <span className="max-w-[140px] overflow-hidden text-ellipsis">{name}</span>
      <button
        className="hit-target flex items-center justify-center w-3.5 h-3.5 rounded-full text-text-muted opacity-0 transition-opacity transition-colors duration-150 ease-out-quart group-hover:opacity-100 group-focus-within:opacity-100 hover:bg-surface-overlay hover:text-text-primary focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-accent-action"
        onClick={onRemove}
        aria-label={`Remove ${name}`}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
          <path
            d="M2.5 2.5L7.5 7.5M7.5 2.5L2.5 7.5"
            stroke="currentColor"
            strokeWidth="1.2"
            strokeLinecap="round"
          />
        </svg>
      </button>
    </span>
  );
}
