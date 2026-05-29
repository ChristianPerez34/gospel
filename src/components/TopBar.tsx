import { useState, useRef, useEffect } from "react";
import type { Workspace, AgentStatus } from "../types";
import { StatusIndicator } from "./StatusIndicator";

interface TopBarProps {
  workspace: Workspace;
  sessionTitle: string;
  model: string;
  status: AgentStatus;
  onSessionTitleChange: (title: string) => void;
  onWorkspaceSwitch: () => void;
  onToggleSessions: () => void;
  onOpenSettings: () => void;
  sessionsOpen: boolean;
}

export function TopBar({
  workspace,
  sessionTitle,
  model,
  status,
  onSessionTitleChange,
  onWorkspaceSwitch,
  onToggleSessions,
  onOpenSettings,
  sessionsOpen,
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
    onSessionTitleChange(title);
  };

  const sessionToggleClass = sessionsOpen
    ? "bg-surface-overlay text-accent-structure"
    : "text-text-muted hover:bg-surface-overlay hover:text-text-secondary";

  return (
    <header className="h-[var(--topbar-height)] flex items-center justify-between px-4 bg-surface-base border-b border-surface-overlay shrink-0">
      <div className="flex items-center gap-2 min-w-0">
        <button
          className={`w-7 h-7 flex items-center justify-center rounded-sm transition-colors duration-150 ease-out-quart ${sessionToggleClass}`}
          onClick={onToggleSessions}
          aria-label="Toggle session history"
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
          className="flex min-w-0 items-center gap-1 py-1 px-2 rounded-sm transition-colors duration-150 ease-out-quart font-body text-text-secondary hover:bg-surface-overlay hover:text-text-primary"
          onClick={onWorkspaceSwitch}
          aria-label="Switch workspace"
        >
          <span className="text-body-sm font-medium truncate max-w-[220px]">{workspace.name}</span>
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
        <span className="w-px h-4 bg-surface-overlay shrink-0" aria-hidden="true" />
        {editing ? (
          <input
            ref={inputRef}
            className="text-body-sm text-text-primary py-1 px-2 rounded-sm max-w-[300px] bg-surface-overlay"
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
            className="text-body-sm text-text-muted py-1 px-2 rounded-sm transition-colors duration-150 ease-out-quart whitespace-nowrap overflow-hidden text-ellipsis max-w-[300px] font-normal hover:bg-surface-overlay hover:text-text-secondary"
            onClick={() => setEditing(true)}
            aria-label="Edit session title"
          >
            {sessionTitle || "New session"}
          </button>
        )}
      </div>
      <div className="flex items-center gap-3 shrink-0">
        <span className="font-mono text-caption tracking-[0.02em] py-0.5 px-2 rounded-sm bg-surface-overlay text-text-muted whitespace-nowrap">
          {model}
        </span>
        <StatusIndicator status={status} />
        <button
          className="w-7 h-7 flex items-center justify-center rounded-sm text-text-muted transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary"
          aria-label="Settings"
          onClick={onOpenSettings}
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 16 16"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.2"
          >
            <circle cx="8" cy="8" r="2.5" />
            <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.1 3.1l1.4 1.4M11.5 11.5l1.4 1.4M3.1 12.9l1.4-1.4M11.5 4.5l1.4-1.4" strokeLinecap="round" />
          </svg>
        </button>
      </div>
    </header>
  );
}
