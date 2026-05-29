import { useState, useEffect, useRef } from "react";
import type { Session } from "../types";

interface SessionDrawerProps {
  sessions: Session[];
  activeSessionId?: string;
  onSelect: (session: Session) => void;
  onNewSession: () => void;
  onClose: () => void;
  open: boolean;
}

function groupByDate(sessions: Session[]): Record<string, Session[]> {
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today.getTime() - 86400000);
  const weekAgo = new Date(today.getTime() - 7 * 86400000);

  const groups: Record<string, Session[]> = {
    Today: [],
    Yesterday: [],
    "This Week": [],
    Older: [],
  };

  for (const session of sessions) {
    const d = new Date(session.timestamp);
    const sessionDate = new Date(d.getFullYear(), d.getMonth(), d.getDate());
    if (sessionDate.getTime() === today.getTime()) {
      groups["Today"].push(session);
    } else if (sessionDate.getTime() === yesterday.getTime()) {
      groups["Yesterday"].push(session);
    } else if (sessionDate.getTime() >= weekAgo.getTime()) {
      groups["This Week"].push(session);
    } else {
      groups["Older"].push(session);
    }
  }

  return groups;
}

export function SessionDrawer({
  sessions,
  activeSessionId,
  onSelect,
  onNewSession,
  onClose,
  open,
}: SessionDrawerProps) {
  const [search, setSearch] = useState("");
  const groups = groupByDate(sessions);

  const filtered: Record<string, Session[]> = {};
  for (const [label, items] of Object.entries(groups)) {
    const matching = items.filter(
      (s) =>
        s.title.toLowerCase().includes(search.toLowerCase()) ||
        s.model.toLowerCase().includes(search.toLowerCase())
    );
    if (matching.length > 0) {
      filtered[label] = matching;
    }
  }

  const [scrimVisible, setScrimVisible] = useState(false);
  const scrimRef = useRef<HTMLDivElement>(null);

  const drawerClass = open
    ? "translate-x-0"
    : "-translate-x-full";

  useEffect(() => {
    if (open) {
      setScrimVisible(true);
    }
  }, [open]);

  const handleScrimTransitionEnd = () => {
    if (!open) {
      setScrimVisible(false);
    }
  };

  return (
    <>
      <div
        ref={scrimRef}
        className="fixed inset-0 bg-scrim z-[calc(var(--z-drawers)_-_1)] transition-opacity duration-[250ms] ease-out-quart"
        style={{ opacity: scrimVisible ? 1 : 0, pointerEvents: open ? "auto" : "none" }}
        onClick={onClose}
        aria-hidden="true"
        onTransitionEnd={handleScrimTransitionEnd}
      />
      <aside
        className={`fixed left-0 top-0 bottom-0 w-[var(--sidebar-width)] bg-surface-elevated border-r border-surface-overlay flex flex-col z-[var(--z-drawers)] transition-transform duration-[250ms] ease-out-quart ${drawerClass}`}
        role="navigation"
        aria-label="Session history"
        aria-hidden={!open}
        tabIndex={open ? undefined : -1}
      >
        <div className="flex items-center gap-2 p-3 border-b border-surface-overlay shrink-0">
          <svg
            className="text-text-muted shrink-0"
            width="14"
            height="14"
            viewBox="0 0 14 14"
            fill="none"
          >
            <circle cx="5.5" cy="5.5" r="4" stroke="currentColor" strokeWidth="1.5" />
            <path d="M9 9L12.5 12.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
          <input
            className="flex-1 font-body text-body-sm text-text-primary placeholder:text-text-muted"
            type="text"
            placeholder="Search sessions..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <div className="flex-1 overflow-y-auto py-2">
          {Object.entries(filtered).map(([label, items]) => (
            <div key={label} className="mb-2">
              <div className="text-caption font-medium text-text-muted tracking-[0.02em] uppercase py-2 px-3">{label}</div>
              {items.map((session) => {
                const isActive = session.id === activeSessionId;
                return (
                  <button
                    key={session.id}
                    className={`flex flex-col gap-0.5 w-full py-2 px-3 text-left transition-colors duration-150 ease-out-quart border-l-2 ${
                      isActive
                        ? "border-l-accent-structure bg-surface-overlay"
                        : "border-l-transparent hover:bg-surface-overlay"
                    }`}
                    onClick={() => onSelect(session)}
                  >
                    <div className="text-body-sm text-text-primary overflow-hidden text-ellipsis whitespace-nowrap">
                      {session.title || "Untitled"}
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-caption text-text-muted">
                        {session.model}
                      </span>
                      <time className="font-mono text-caption text-text-muted">
                        {session.timestamp.toLocaleTimeString([], {
                          hour: "2-digit",
                          minute: "2-digit",
                        })}
                      </time>
                    </div>
                  </button>
                );
              })}
            </div>
          ))}
        </div>
        <button
          className="flex items-center justify-center gap-2 w-full p-3 border-t border-surface-overlay text-body-sm text-text-muted transition-colors duration-150 ease-out-quart shrink-0 hover:bg-surface-overlay hover:text-text-secondary"
          onClick={onNewSession}
        >
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
            <path d="M7 3V11M3 7H11" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
          New session
        </button>
      </aside>
    </>
  );
}
