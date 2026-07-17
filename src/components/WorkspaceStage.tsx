import { Activity, ExternalLink, FolderOpen, GitBranch, RefreshCw, Terminal } from "lucide-react";
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
    <section className="workspace-stage-shell" aria-label="Evidence rail">
      <header className="workspace-stage-header">
        <div className="workspace-stage-title">
          <FolderOpen aria-hidden="true" />
          <div>
            <span>Evidence Rail</span>
            <small>Review, browser, and runtime artifacts</small>
          </div>
        </div>
        <div className="workspace-stage-actions">
          <button
            className="stage-icon-button"
            type="button"
            disabled
            aria-label="Refresh workspace view"
            title="Refresh (coming soon)"
          >
            <RefreshCw aria-hidden="true" />
          </button>
          <button
            className="stage-icon-button"
            type="button"
            disabled
            aria-label="Open workspace externally"
            title="Open externally (coming soon)"
          >
            <ExternalLink aria-hidden="true" />
          </button>
        </div>
      </header>

      <div className="workspace-stage-content">
        <div className="workspace-stage-preview">
          <div className="stage-focus-mark" aria-hidden="true" />
          <div className="workspace-stage-overview">
            <div className="stage-status-panel">
              <div className="stage-status-kicker">
                <span className={`stage-status-dot ${STATUS_CLASS[status]}`} aria-hidden="true" />
                <span>{STATUS_COPY[status]}</span>
              </div>
              <h2>Workspace Evidence</h2>
              <p>{trimPath(workspacePath)}</p>
              <div className="stage-stat-grid">
                <div className="stage-stat">
                  <span>Workspace</span>
                  <strong>{workspaceName}</strong>
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
                  <strong>{reviewOpen ? "Inspector open" : "Packet ready"}</strong>
                </div>
              </div>
            </div>

            <section className="evidence-module-grid" aria-label="Evidence modules">
              <article className="evidence-module">
                <div className="evidence-module-header">
                  <Activity aria-hidden="true" />
                  <span>Run ledger</span>
                </div>
                <p>{sessionTitle || "New session"}</p>
              </article>
              <article className="evidence-module">
                <div className="evidence-module-header">
                  <GitBranch aria-hidden="true" />
                  <span>Review packet</span>
                </div>
                <p>{reviewOpen ? "Inspector is open" : "Findings attach to this run"}</p>
              </article>
              <article className="evidence-module">
                <div className="evidence-module-header">
                  <ExternalLink aria-hidden="true" />
                  <span>Browser artifacts</span>
                </div>
                <p>Preview, console, network, and screenshots will dock here.</p>
              </article>
              <article className="evidence-module">
                <div className="evidence-module-header">
                  <Terminal aria-hidden="true" />
                  <span>Verification</span>
                </div>
                <p>Tests and command output stay tied to the current run.</p>
              </article>
            </section>
          </div>
        </div>

        <section className="workspace-terminal-panel" aria-label="Runtime telemetry summary">
          <div className="workspace-terminal-title">
            <Activity aria-hidden="true" />
            <span>Runtime Telemetry</span>
          </div>
          <div className="workspace-terminal-lines">
            <div>
              <span className="term-prompt">gospel</span>
              <span className="term-dir">{workspaceName}</span>
              <span className="term-muted">run.inspect</span>
            </div>
            <div>
              <span className="term-muted">status</span>
              <span className={status === "error" ? "term-error" : "term-success"}>
                {STATUS_COPY[status]}
              </span>
            </div>
            <div>
              <GitBranch aria-hidden="true" />
              <span className="term-muted">mode</span>
              <span>{sessionMode === "Build" ? "Build enabled" : "Plan only"}</span>
            </div>
            <div>
              <ExternalLink aria-hidden="true" />
              <span className="term-muted">browser</span>
              <span>Artifacts pending integration</span>
            </div>
          </div>
        </section>
      </div>
    </section>
  );
}
