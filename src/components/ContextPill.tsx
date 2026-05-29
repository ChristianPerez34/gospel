interface ContextPillProps {
  name: string;
  onRemove: () => void;
}

export function ContextPill({ name, onRemove }: ContextPillProps) {
  return (
    <span className="inline-flex items-center gap-1 py-0.5 px-2 bg-surface-overlay rounded-md font-mono text-body-sm text-text-secondary whitespace-nowrap shrink-0 group">
      <span className="max-w-[120px] overflow-hidden text-ellipsis">
        {name}
      </span>
      <button
        className="flex items-center justify-center w-3.5 h-3.5 rounded-full text-text-muted opacity-0 transition-opacity transition-colors duration-150 ease-out-quart group-hover:opacity-100 hover:bg-surface-elevated hover:text-text-primary"
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
