import "./ContextPill.css";

interface ContextPillProps {
  name: string;
  onRemove: () => void;
}

export function ContextPill({ name, onRemove }: ContextPillProps) {
  return (
    <span className="context-pill">
      <span className="context-pill__name">{name}</span>
      <button
        className="context-pill__remove"
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