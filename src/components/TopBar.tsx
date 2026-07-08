import { useState, useRef, useEffect, type RefObject } from "react";
import {
  Columns2,
  Cpu,
  Frame,
  GitPullRequest,
  Hammer,
  Lock,
  Shield,
} from "lucide-react";
import type { Workspace, AgentStatus, SessionMode } from "../types";
import { StatusIndicator } from "./StatusIndicator";

export type WorkspaceLayoutMode = "focus" | "pairing" | "review" | "pipeline";

interface TopBarProps {
  workspace: Workspace;
  sessionTitle: string;
  sessionMode: SessionMode;
  workspaceLayout?: WorkspaceLayoutMode;
  model: string;
  status: AgentStatus;
  onWorkspaceSwitch: () => void;
  onSessionModeChange: (mode: SessionMode) => Promise<void>;
  onWorkspaceLayoutChange?: (mode: WorkspaceLayoutMode) => void;
  onToggleSessions: () => void;
  onOpenSettings: () => void;
  sessionsOpen: boolean;
  sessionToggleRef?: RefObject<HTMLButtonElement>;
}

const LAYOUT_OPTIONS = [
  { value: "focus", label: "Focus", Icon: Frame },
  { value: "pairing", label: "Pair", Icon: Columns2 },
  { value: "review", label: "Review", Icon: Shield },
  { value: "pipeline", label: "Pipeline", Icon: GitPullRequest },
] as const;

export function TopBar({
  workspace,
  sessionTitle,
  sessionMode,
  workspaceLayout = "pairing",
  model,
  status,
  onWorkspaceSwitch,
  onSessionModeChange,
  onWorkspaceLayoutChange,
  onToggleSessions,
  onOpenSettings,
  sessionsOpen,
  sessionToggleRef,
}: TopBarProps) {
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(sessionTitle);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setTitle(sessionTitle);
  }, [sessionTitle]);

  useEffect(() => {
    if (editing && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [editing]);

  const handleSubmit = () => {
    setEditing(false);
  };

  const nextMode: SessionMode = sessionMode === "Build" ? "ReadOnly" : "Build";
  const modeLabel = sessionMode === "Build" ? "Build" : "Plan";
  const nextModeLabel = nextMode === "Build" ? "Build" : "Plan";

  const sessionToggleClass = sessionsOpen
    ? "bg-surface-overlay text-accent-structure"
    : "text-text-muted hover:bg-surface-overlay hover:text-text-secondary";
  const computeActive = status === "thinking" || status === "acting";

  return (
    <header className="app-topbar spatial-topbar px-4 bg-surface-base shrink-0">
      <div className="topbar-primary flex items-center gap-2 min-w-0">
        <button
          ref={sessionToggleRef}
          className={`hit-target w-8 h-8 flex items-center justify-center rounded-sm transition-colors duration-150 ease-out-quart ${sessionToggleClass}`}
          onClick={onToggleSessions}
          aria-label="Toggle session history"
          aria-pressed={sessionsOpen}
          title="Sessions"
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
            <rect x="2" y="2" width="12" height="12" rx="2" stroke="currentColor" strokeWidth="1.2" />
            <line x1="5" y1="6" x2="11" y2="6" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
            <line x1="5" y1="8.5" x2="9" y2="8.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
            <line x1="5" y1="11" x2="7" y2="11" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
          </svg>
        </button>
        <button
          className="hit-target flex min-h-11 min-w-11 items-center gap-1 rounded-sm px-2 transition-colors duration-150 ease-out-quart font-body text-text-secondary hover:bg-surface-overlay hover:text-text-primary"
          onClick={onWorkspaceSwitch}
          aria-label="Switch workspace"
          title="Switch workspace"
        >
          <span className="topbar-workspace-text text-body-sm font-medium truncate max-w-[220px]">{workspace.name}</span>
          <svg
            className="shrink-0 opacity-50"
            width="14"
            height="14"
            viewBox="0 0 14 14"
            fill="none"
          >
            <path
              d="M4.5 5.5L7 8L9.5 5.5"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </button>
        <span className="topbar-divider w-px h-4 bg-surface-overlay shrink-0" aria-hidden="true" />
        {editing ? (
          <input
            ref={inputRef}
            className="topbar-session-title min-h-11 text-body-sm text-text-primary px-2 rounded-sm max-w-[300px] bg-surface-overlay"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            onBlur={handleSubmit}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleSubmit();
              if (e.key === "Escape") {
                setTitle(sessionTitle);
                setEditing(false);
              }
            }}
            aria-label="Session title"
          />
        ) : (
          <button
            className="topbar-session-title hit-target min-h-11 text-body-sm text-text-muted px-2 rounded-sm transition-colors duration-150 ease-out-quart whitespace-nowrap overflow-hidden text-ellipsis max-w-[300px] font-normal hover:bg-surface-overlay hover:text-text-secondary"
            onClick={() => setEditing(true)}
            aria-label="Edit session title"
          >
            {sessionTitle || "New session"}
          </button>
        )}
        <div className="topbar-session-mode-wrap">
          <button
            type="button"
            className={`topbar-session-mode hit-target ${sessionMode === "ReadOnly" ? "is-readonly" : ""}`}
            onClick={() => void onSessionModeChange(nextMode)}
            aria-label={`Session mode: ${modeLabel}. Change to ${nextModeLabel}.`}
            aria-pressed={sessionMode === "ReadOnly"}
            title={`Session mode: ${modeLabel}`}
          >
            {sessionMode === "ReadOnly" ? <Lock aria-hidden="true" /> : <Hammer aria-hidden="true" />}
            <span>{modeLabel}</span>
          </button>
        </div>
      </div>
      <div className="topbar-layout-controls" role="group" aria-label="Workspace layout">
        {LAYOUT_OPTIONS.map(({ value, label, Icon }) => {
          const active = workspaceLayout === value;
          return (
            <button
              type="button"
              className={`topbar-layout-button ${active ? "is-active" : ""}`}
              key={value}
              onClick={() => onWorkspaceLayoutChange?.(value)}
              aria-pressed={active}
              title={`${label} layout`}
            >
              <Icon aria-hidden="true" />
              <span>{label}</span>
            </button>
          );
        })}
      </div>
      <div className="topbar-actions flex items-center gap-3 shrink-0">
        <div
          className={`topbar-compute-graph ${computeActive ? "is-active" : ""}`}
          title={computeActive ? "Gospel is running" : "Gospel is idle"}
          aria-hidden="true"
        >
          <span />
          <span />
          <span />
          <span />
          <span />
        </div>
        <span className="topbar-model-badge inline-flex items-center gap-1.5 font-mono text-caption tracking-[0.02em] py-0.5 px-2 rounded-sm bg-surface-overlay text-text-muted whitespace-nowrap">
          <Cpu className="size-3 text-text-muted" aria-hidden="true" />
          {model}
        </span>
        <StatusIndicator status={status} />
        <button
          className="hit-target w-8 h-8 flex items-center justify-center rounded-sm text-text-muted transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary"
          aria-label="Settings"
          title="Settings"
          onClick={onOpenSettings}
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 16 16"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.2"
            strokeLinecap="round"
            aria-hidden="true"
          >
            <line x1="2" y1="4.5" x2="14" y2="4.5" />
            <line x1="2" y1="8" x2="14" y2="8" />
            <line x1="2" y1="11.5" x2="14" y2="11.5" />
            <circle cx="9" cy="4.5" r="1.7" fill="currentColor" stroke="none" />
            <circle cx="5" cy="8" r="1.7" fill="currentColor" stroke="none" />
            <circle cx="11" cy="11.5" r="1.7" fill="currentColor" stroke="none" />
          </svg>
        </button>
      </div>
    </header>
  );
}
