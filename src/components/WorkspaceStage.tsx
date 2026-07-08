import {
  Activity,
  ExternalLink,
  FolderOpen,
  GitBranch,
  RefreshCw,
  Terminal,
} from "lucide-react";
import type { AgentStatus, SessionMode } from "../types";

interface WorkspaceStageProps {
  workspaceName: string;
  workspacePath?: string;
  sessionTitle: string;
  sessionMode: SessionMode;
  status: AgentStatus;
  messageCount: number;
  reviewOpen: boolean;
}

const STATUS_COPY: Record<AgentStatus, string> = {
  idle: "Idle",
  thinking: "Thinking",
  acting: "Acting",
  error: "Needs attention",
  connected: "Connected",
};

const STATUS_CLASS: Record<AgentStatus, string> = {
  idle: "is-muted",
  thinking: "is-active",
  acting: "is-active",
  error: "is-error",
  connected: "is-success",
};

function trimPath(path?: string) {
  if (!path) return "No workspace path";
  const parts = path.split("/").filter(Boolean);
  if (parts.length <= 3) return path;
  return `.../${parts.slice(-3).join("/")}`;
}

export function WorkspaceStage({
  workspaceName,
  workspacePath,
  sessionTitle,
  sessionMode,
  status,
  messageCount,
  reviewOpen,
}: WorkspaceStageProps) {
  return (
    <section className="workspace-stage-shell" aria-label="Workspace stage">
      <header className="workspace-stage-header">
        <div className="workspace-stage-tabs" role="tablist" aria-label="Workspace surfaces">
          <button className="workspace-stage-tab is-active" type="button" role="tab" aria-selected="true">
            <FolderOpen aria-hidden="true" />
            <span>Live Workspace</span>
          </button>
          <button className="workspace-stage-tab" type="button" role="tab" aria-selected="false">
            <Terminal aria-hidden="true" />
            <span>Run Log</span>
          </button>
        </div>
        <div className="workspace-stage-actions">
          <button className="stage-icon-button" type="button" aria-label="Refresh workspace view" title="Refresh">
            <RefreshCw aria-hidden="true" />
          </button>
          <button className="stage-icon-button" type="button" aria-label="Open workspace externally" title="Open externally">
            <ExternalLink aria-hidden="true" />
          </button>
        </div>
      </header>

      <div className="workspace-stage-content">
        <div className="workspace-stage-preview">
          <div className="stage-grid-mark" aria-hidden="true" />
          <div className="stage-status-panel">
            <div className="stage-status-kicker">
              <span className={`stage-status-dot ${STATUS_CLASS[status]}`} aria-hidden="true" />
              <span>{STATUS_COPY[status]}</span>
            </div>
            <h2>{workspaceName}</h2>
            <p>{trimPath(workspacePath)}</p>
            <div className="stage-stat-grid">
              <div className="stage-stat">
                <span>Session</span>
                <strong>{sessionTitle || "New session"}</strong>
              </div>
              <div className="stage-stat">
                <span>Mode</span>
                <strong>{sessionMode === "Build" ? "Build" : "Plan"}</strong>
              </div>
              <div className="stage-stat">
                <span>Turns</span>
                <strong>{messageCount}</strong>
              </div>
              <div className="stage-stat">
                <span>Review</span>
                <strong>{reviewOpen ? "Open" : "Closed"}</strong>
              </div>
            </div>
          </div>
        </div>

        <div className="workspace-terminal-panel" aria-label="Workspace terminal summary">
          <div className="workspace-terminal-title">
            <Activity aria-hidden="true" />
            <span>Workspace Telemetry</span>
          </div>
          <div className="workspace-terminal-lines">
            <div>
              <span className="term-prompt">gospel</span>
              <span className="term-dir">{workspaceName}</span>
              <span className="term-muted">session.inspect</span>
            </div>
            <div>
              <span className="term-muted">status</span>
              <span className={status === "error" ? "term-error" : "term-success"}>{STATUS_COPY[status]}</span>
            </div>
            <div>
              <GitBranch aria-hidden="true" />
              <span className="term-muted">mode</span>
              <span>{sessionMode === "Build" ? "Build enabled" : "Plan only"}</span>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
