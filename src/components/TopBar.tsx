import {
  ChevronDown,
  ChevronRight,
  Cpu,
  Folder,
  Layers,
  LayoutDashboard,
  PanelLeft,
  Settings2,
} from "lucide-react";
import { type RefObject, useEffect, useRef, useState } from "react";
import { needsYouLabel } from "../sessionActivity";
import type { AgentStatus, Workspace } from "../types";
import { StatusIndicator } from "./StatusIndicator";

interface TopBarProps {
  workspace: Workspace;
  sessionTitle: string;
  model: string;
  status: AgentStatus;
  onWorkspaceSwitch: () => void;
  onSessionTitleChange: (title: string) => void;
  onToggleSessions: () => void;
  onOpenSettings: () => void;
  sessionsOpen: boolean;
  needsYouCount?: number;
  sessionToggleRef?: RefObject<HTMLButtonElement>;
  overviewOpen?: boolean;
  taskSetupOpen?: boolean;
  onOpenOverview?: () => void;
  runningTaskCount?: number;
  showWorkspaceSwitch?: boolean;
}

export function TopBar({
  workspace,
  sessionTitle,
  model,
  status,
  onWorkspaceSwitch,
  onSessionTitleChange,
  onToggleSessions,
  onOpenSettings,
  sessionsOpen,
  needsYouCount = 0,
  sessionToggleRef,
  overviewOpen = false,
  taskSetupOpen = false,
  onOpenOverview,
  runningTaskCount = 0,
  showWorkspaceSwitch = true,
}: TopBarProps) {
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(sessionTitle);
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    setTitle(sessionTitle);
  }, [sessionTitle]);
  useEffect(() => {
    if (editing) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [editing]);
  const handleSubmit = () => {
    const next = title.trim();
    if (next && next !== sessionTitle) onSessionTitleChange(next);
    else setTitle(sessionTitle);
    setEditing(false);
  };
  const needsYouText = needsYouLabel(needsYouCount);
  const taskButtonLabel = needsYouCount > 0 ? `Toggle tasks, ${needsYouText}` : "Toggle tasks";
  const globalView = overviewOpen || taskSetupOpen;

  return (
    <header className="app-topbar spatial-topbar" data-navigation-open={sessionsOpen}>
      <div className="topbar-identity">
        {onOpenOverview && (
          <button
            type="button"
            className="gospel-brand"
            onClick={onOpenOverview}
            aria-label="Gospel command center"
          >
            <span className="gospel-mark">
              <Layers size={16} strokeWidth={1.6} aria-hidden="true" />
            </span>
            <span>gospel</span>
          </button>
        )}
        <button
          type="button"
          ref={sessionToggleRef}
          className="shell-icon-button topbar-navigation-toggle"
          onClick={onToggleSessions}
          aria-label={taskButtonLabel}
          aria-pressed={sessionsOpen}
          title={sessionsOpen ? "Hide task navigation" : "Show task navigation"}
        >
          <PanelLeft size={17} strokeWidth={1.6} aria-hidden="true" />
          {needsYouCount > 0 && (
            <span className="task-rail-toggle-count" aria-hidden="true">
              {needsYouCount > 9 ? "9+" : needsYouCount}
            </span>
          )}
        </button>
      </div>
      <div className="topbar-main">
        <div className="topbar-primary">
          {showWorkspaceSwitch ? (
            <button
              type="button"
              className="topbar-workspace-control"
              onClick={onWorkspaceSwitch}
              aria-label="Switch workspace"
              title={workspace.path || "Choose a workspace"}
            >
              <Folder size={15} strokeWidth={1.6} aria-hidden="true" />
              <span className="topbar-workspace-text">{workspace.name}</span>
              <ChevronDown size={12} aria-hidden="true" />
            </button>
          ) : overviewOpen ? (
            <span className="topbar-scope">
              <LayoutDashboard size={15} strokeWidth={1.6} aria-hidden="true" />
              All workspaces
            </span>
          ) : (
            <span className="topbar-scope" title={workspace.path}>
              <Folder size={15} strokeWidth={1.6} aria-hidden="true" />
              {workspace.name}
            </span>
          )}
          <ChevronRight className="topbar-breadcrumb-divider" size={13} aria-hidden="true" />
          {globalView ? (
            <span className="topbar-overview-label">{taskSetupOpen ? "New task" : "Overview"}</span>
          ) : editing ? (
            <input
              ref={inputRef}
              className="topbar-session-title topbar-title-input"
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              onBlur={handleSubmit}
              onKeyDown={(event) => {
                if (event.key === "Enter") handleSubmit();
                if (event.key === "Escape") {
                  setTitle(sessionTitle);
                  setEditing(false);
                }
              }}
              aria-label="Session title"
            />
          ) : (
            <button
              type="button"
              className="topbar-session-title"
              onClick={() => setEditing(true)}
              aria-label="Edit session title"
              title="Edit session title"
            >
              {sessionTitle || "Untitled task"}
            </button>
          )}
        </div>
        <div className="topbar-actions">
          {globalView ? (
            runningTaskCount > 0 && (
              <span className="topbar-global-status">
                <span className="live-dot is-running" />
                {runningTaskCount} running
              </span>
            )
          ) : (
            <>
              <span className="topbar-model-badge">
                <Cpu size={13} strokeWidth={1.6} aria-hidden="true" />
                <span>{model}</span>
              </span>
              <StatusIndicator status={status} />
            </>
          )}
          <button
            type="button"
            className="shell-icon-button"
            aria-label="Settings"
            title="Settings"
            onClick={onOpenSettings}
          >
            <Settings2 size={17} strokeWidth={1.6} aria-hidden="true" />
          </button>
        </div>
      </div>
    </header>
  );
}
