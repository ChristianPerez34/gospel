import type { AgentStatus } from "../types";
import "./StatusIndicator.css";

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

export function StatusIndicator({ status }: StatusIndicatorProps) {
  return (
    <div className="status-indicator" title={STATUS_LABELS[status]}>
      <span
        className={`status-indicator__dot status-indicator__dot--${status}`}
        aria-hidden="true"
      />
      {status !== "idle" && status !== "connected" && (
        <span className="status-indicator__label">{STATUS_LABELS[status]}</span>
      )}
    </div>
  );
}