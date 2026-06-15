import type { AgentStatus } from "../types";

interface StatusIndicatorProps {
  status: AgentStatus;
}

const STATUS_LABELS: Record<AgentStatus, string> = {
  idle: "Idle",
  thinking: "Thinking",
  acting: "Acting",
  error: "Error",
  connected: "Connected",
};

const DOT_CLASSES: Record<AgentStatus, string> = {
  idle: "bg-text-muted",
  thinking: "bg-accent-action animate-pulse",
  acting: "bg-accent-structure animate-pulse",
  error: "bg-status-error",
  connected: "bg-status-success",
};

export function StatusIndicator({ status }: StatusIndicatorProps) {
  return (
    <div className="flex items-center gap-1" title={STATUS_LABELS[status]}>
      <span
        className={`w-2 h-2 rounded-full shrink-0 ${DOT_CLASSES[status]}`}
        aria-hidden="true"
      />
      {status !== "idle" && status !== "connected" && (
        <span className="topbar-status-label text-caption text-text-muted tracking-[0.02em] whitespace-nowrap">
          {STATUS_LABELS[status]}
        </span>
      )}
    </div>
  );
}
