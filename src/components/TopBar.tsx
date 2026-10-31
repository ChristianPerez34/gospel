import {
  ChevronDown,
  ChevronRight,
  Cpu,
  Folder,
  Layers,
  LayoutDashboard,
  PanelLeft,
  Settings2,
  Shield,
  Sparkles,
  Zap,
} from "lucide-react";
import { type RefObject, useEffect, useRef, useState } from "react";
import { needsYouLabel } from "../sessionActivity";
import type { AgentStatus, AutonomyMode, Workspace, WorkspaceGitContext } from "../types";
import { GitContextBadge } from "./GitContextBadge";
import { StatusIndicator } from "./StatusIndicator";

interface TopBarProps {
  workspace: Workspace;
  gitContext?: WorkspaceGitContext | null;
  sessionTitle: string;
  model: string;
  status: AgentStatus;
  autonomyMode?: AutonomyMode;
  onToggleAutonomy?: () => void;
  allowedRuleCount?: number;
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
  onOpenCommandPalette?: () => void;
}

function SessionTitleControl({
  globalView,
  taskSetupOpen,
  sessionTitle,
  onSessionTitleChange,
}: {
  globalView: boolean;
  taskSetupOpen: boolean;
  sessionTitle: string;
  onSessionTitleChange: (title: string) => void;
}) {
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

  if (globalView) {
    return <span className="topbar-overview-label">{taskSetupOpen ? "New task" : "Overview"}</span>;
  }

  if (editing) {
    return (
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
    );
  }

  return (
    <button
      type="button"
      className="topbar-session-title"
      onClick={() => setEditing(true)}
      aria-label="Edit session title"
      title="Edit session title"
    >
      {sessionTitle || "Untitled task"}
    </button>
  );
}

function WorkspaceScopeControl({
  showWorkspaceSwitch,
  overviewOpen,
  workspace,
  onWorkspaceSwitch,
}: {
  showWorkspaceSwitch: boolean;
  overviewOpen: boolean;
  workspace: Workspace;
  onWorkspaceSwitch: () => void;
}) {
  if (showWorkspaceSwitch) {
    return (
      <button
        type="button"
        className="topbar-workspace-control"
        onClick={onWorkspaceSwitch}
        aria-label="Switch workspace"
        title={workspace.path || "Choose a workspace"}
      >
        <Folder size={14} strokeWidth={1.6} aria-hidden="true" />
        <span className="topbar-workspace-text">{workspace.name}</span>
        <ChevronDown size={12} aria-hidden="true" />
      </button>
    );
  }
  if (overviewOpen) {
    return (
      <span className="topbar-scope">
        <LayoutDashboard size={14} strokeWidth={1.6} aria-hidden="true" />
        All workspaces
      </span>
    );
  }
  return (
    <span className="topbar-scope" title={workspace.path}>
      <Folder size={14} strokeWidth={1.6} aria-hidden="true" />
      {workspace.name}
    </span>
  );
}

export function TopBar({
  workspace,
  gitContext,
  sessionTitle,
  model,
  status,
  autonomyMode = "ask",
  onToggleAutonomy,
  allowedRuleCount = 0,
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
  onOpenCommandPalette,
}: TopBarProps) {
  const needsYouText = needsYouLabel(needsYouCount);
  const taskButtonLabel = needsYouCount > 0 ? `Toggle tasks, ${needsYouText}` : "Toggle tasks";
  const globalView = overviewOpen || taskSetupOpen;
  return (
    <header className="app-topbar inset-topbar" data-navigation-open={sessionsOpen}>
      <div className="inset-topbar-left">
        <button
          type="button"
          ref={sessionToggleRef}
          className="shell-icon-button topbar-navigation-toggle"
          onClick={onToggleSessions}
          aria-label={taskButtonLabel}
          aria-pressed={sessionsOpen}
          title={sessionsOpen ? "Hide task navigation" : "Show task navigation"}
        >
          <PanelLeft size={16} strokeWidth={1.6} aria-hidden="true" />
          {needsYouCount > 0 && (
            <span className="task-rail-toggle-count" aria-hidden="true">
              {needsYouCount > 9 ? "9+" : needsYouCount}
            </span>
          )}
        </button>

        {onOpenOverview && (
          <div className={`inset-brand-slot${sessionsOpen ? " is-rail-open" : ""}`}>
            <button
              type="button"
              className="gospel-brand"
              onClick={onOpenOverview}
              aria-label="Gospel command center"
            >
              <span className="gospel-mark">
                <Layers size={15} strokeWidth={1.7} aria-hidden="true" />
              </span>
              <span className="gospel-brand-text">gospel</span>
            </button>
          </div>
        )}

        <div className="topbar-primary">
          <WorkspaceScopeControl
            showWorkspaceSwitch={showWorkspaceSwitch}
            overviewOpen={overviewOpen}
            workspace={workspace}
            onWorkspaceSwitch={onWorkspaceSwitch}
          />
          {!globalView && gitContext && <GitContextBadge gitContext={gitContext} />}
          <ChevronRight className="topbar-breadcrumb-divider" size={13} aria-hidden="true" />
          <SessionTitleControl
            globalView={globalView}
            taskSetupOpen={taskSetupOpen}
            sessionTitle={sessionTitle}
            onSessionTitleChange={onSessionTitleChange}
          />
        </div>
      </div>

      <div className="topbar-actions inset-topbar-right">
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
            {onToggleAutonomy && (
              <button
                type="button"
                className={`topbar-autonomy-badge transition-transform duration-150 ease-out active:scale-[0.97] ${
                  autonomyMode === "auto" ? "is-autonomous" : ""
                }`}
                onClick={onToggleAutonomy}
                data-testid="topbar-autonomy-toggle"
                title={
                  autonomyMode === "auto"
                    ? "Autonomous execution active: Safe commands auto-execute; destructive actions always prompt (Click to switch to Ask mode)"
                    : `Ask mode active: Approvals required for mutating commands${
                        allowedRuleCount > 0
                          ? ` (${allowedRuleCount} always-allowed rule${allowedRuleCount > 1 ? "s" : ""})`
                          : ""
                      } (Click to switch to Autonomous mode)`
                }
                aria-label={`Command autonomy: ${autonomyMode === "auto" ? "Autonomous" : "Ask mode"}`}
              >
                {autonomyMode === "auto" ? (
                  <>
                    <Zap size={12} strokeWidth={2} className="text-status-warning" aria-hidden="true" />
                    <span>Auto</span>
                  </>
                ) : (
                  <>
                    <Shield size={12} strokeWidth={1.8} className="text-text-muted" aria-hidden="true" />
                    <span>Ask</span>
                  </>
                )}
              </button>
            )}
            <StatusIndicator status={status} />
          </>
        )}
        {onOpenCommandPalette && (
          <button
            type="button"
            className="inset-cmd-trigger"
            onClick={onOpenCommandPalette}
            title="Command palette (⌘K)"
          >
            <Sparkles size={12} aria-hidden="true" />
            <span>Commands</span>
            <kbd>⌘K</kbd>
          </button>
        )}
        <button
          type="button"
          className="shell-icon-button"
          aria-label="Settings"
          title="Settings"
          onClick={onOpenSettings}
        >
          <Settings2 size={16} strokeWidth={1.6} aria-hidden="true" />
        </button>
      </div>
    </header>
  );
}
