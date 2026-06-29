import { useState, useRef, useEffect, type RefObject } from "react";
import { Check, Hammer, Lock, X } from "lucide-react";
import type { Workspace, AgentStatus, SessionMode } from "../types";
import { StatusIndicator } from "./StatusIndicator";

interface TopBarProps {
  workspace: Workspace;
  sessionTitle: string;
  sessionMode: SessionMode;
  model: string;
  status: AgentStatus;
  onWorkspaceSwitch: () => void;
  onSessionModeChange: (mode: SessionMode) => Promise<void>;
  onToggleSessions: () => void;
  onOpenReview: () => void;
  onOpenSettings: () => void;
  sessionsOpen: boolean;
  reviewOpen?: boolean;
  sessionToggleRef?: RefObject<HTMLButtonElement>;
  reviewToggleRef?: RefObject<HTMLButtonElement>;
}

export function TopBar({
  workspace,
  sessionTitle,
  sessionMode,
  model,
  status,
  onWorkspaceSwitch,
  onSessionModeChange,
  onToggleSessions,
  onOpenReview,
  onOpenSettings,
  sessionsOpen,
  reviewOpen = false,
  sessionToggleRef,
  reviewToggleRef,
}: TopBarProps) {
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(sessionTitle);
  const [pendingMode, setPendingMode] = useState<SessionMode | null>(null);
  const [savingMode, setSavingMode] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setTitle(sessionTitle);
  }, [sessionTitle]);

  useEffect(() => {
    setPendingMode(null);
  }, [sessionMode, sessionTitle]);

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
  const modeLabel = sessionMode === "Build" ? "Build" : "Read-only";
  const nextModeLabel = nextMode === "Build" ? "Build" : "Read-only";
  const pendingCopy =
    pendingMode === "ReadOnly"
      ? "Future turns will refuse workspace edits. Prior edits remain."
      : "Future turns can edit the workspace again.";

  const confirmModeChange = async () => {
    if (!pendingMode) return;
    setSavingMode(true);
    try {
      await onSessionModeChange(pendingMode);
      setPendingMode(null);
    } finally {
      setSavingMode(false);
    }
  };

  const sessionToggleClass = sessionsOpen
    ? "bg-surface-overlay text-accent-structure"
    : "text-text-muted hover:bg-surface-overlay hover:text-text-secondary";
  const reviewToggleClass = reviewOpen
    ? "bg-surface-overlay text-accent-action"
    : "text-text-muted hover:bg-surface-overlay hover:text-text-secondary";

  return (
    <header className="app-topbar flex items-center justify-between px-4 bg-surface-base shrink-0">
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
            onClick={() => setPendingMode(nextMode)}
            aria-label={`Session mode: ${modeLabel}. Change to ${nextModeLabel}.`}
            aria-pressed={sessionMode === "ReadOnly"}
            title={`Session mode: ${modeLabel}`}
          >
            {sessionMode === "ReadOnly" ? <Lock aria-hidden="true" /> : <Hammer aria-hidden="true" />}
            <span>{modeLabel}</span>
          </button>
          {pendingMode && (
            <div className="topbar-mode-confirm" role="status" aria-live="polite">
              <span>{pendingCopy}</span>
              <button
                type="button"
                className="topbar-mode-confirm-action"
                onClick={() => void confirmModeChange()}
                aria-label={`Confirm ${nextModeLabel} mode`}
                title="Apply"
                disabled={savingMode}
              >
                <Check aria-hidden="true" />
              </button>
              <button
                type="button"
                className="topbar-mode-confirm-action"
                onClick={() => setPendingMode(null)}
                aria-label="Cancel session mode change"
                title="Cancel"
                disabled={savingMode}
              >
                <X aria-hidden="true" />
              </button>
            </div>
          )}
        </div>
      </div>
      <div className="topbar-actions flex items-center gap-3 shrink-0">
        <span className="topbar-model-badge font-mono text-caption tracking-[0.02em] py-0.5 px-2 rounded-sm bg-surface-overlay text-text-muted whitespace-nowrap">
          {model}
        </span>
        <StatusIndicator status={status} />
        <button
          ref={reviewToggleRef}
          className={`hit-target w-8 h-8 flex items-center justify-center rounded-sm transition-colors duration-150 ease-out-quart ${reviewToggleClass}`}
          aria-label="Open security review"
          aria-pressed={reviewOpen}
          title="Security review"
          onClick={onOpenReview}
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 16 16"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.2"
            aria-hidden="true"
          >
            <path d="M8 2 13 4v3.4c0 3.1-1.9 5.2-5 6.6-3.1-1.4-5-3.5-5-6.6V4l5-2Z" strokeLinejoin="round" />
            <path d="m5.7 8.1 1.4 1.4 3.2-3.1" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </button>
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
