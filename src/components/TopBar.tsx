import { useState, useRef, useEffect } from "react";
import type { Workspace, AgentStatus } from "../types";
import { StatusIndicator } from "./StatusIndicator";
import "./TopBar.css";

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

  return (
    <header className="topbar">
      <div className="topbar__left">
        <button
          className={`topbar__session-toggle${sessionsOpen ? " topbar__session-toggle--active" : ""}`}
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
          className="topbar__workspace-btn"
          onClick={onWorkspaceSwitch}
          aria-label="Switch workspace"
        >
          <span className="topbar__workspace-name">{workspace.name}</span>
          <svg
            className="topbar__chevron"
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
        <span className="topbar__separator" aria-hidden="true" />
        {editing ? (
          <input
            ref={inputRef}
            className="topbar__title-input"
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
            className="topbar__title"
            onClick={() => setEditing(true)}
            aria-label="Edit session title"
          >
            {sessionTitle || "New session"}
          </button>
        )}
      </div>
      <div className="topbar__right">
        <span className="topbar__model-badge">{model}</span>
        <StatusIndicator status={status} />
        <button className="topbar__overflow" aria-label="Settings" onClick={onOpenSettings}>
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