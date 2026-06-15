import { useEffect, useRef, useState, type RefObject } from "react";
import type { Session } from "../types";
import { useFocusTrap } from "../hooks/useFocusTrap";

interface SessionDrawerProps {
  sessions: Session[];
  activeSessionId?: string;
  onSelect: (session: Session) => void;
  onNewSession: () => void;
  onClose: () => void;
  open: boolean;
  triggerRef?: RefObject<HTMLElement>;
  trapPaused?: boolean;
}

function setInert(element: HTMLElement, inert: boolean) {
  if (!(element instanceof HTMLElement)) return;
  (element as HTMLElement & { inert?: boolean }).inert = inert;
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
  triggerRef,
  trapPaused = false,
}: SessionDrawerProps) {
  const [search, setSearch] = useState("");
  const drawerRef = useRef<HTMLElement>(null);
  const groups = groupByDate(sessions);

  useFocusTrap({
    active: open && !trapPaused,
    containerRef: drawerRef,
    onEscape: onClose,
    restoreFocusRef: triggerRef,
    shouldRestoreFocusOnDeactivate: !trapPaused,
  });

  useEffect(() => {
    const drawer = drawerRef.current;
    if (!drawer) return;
    setInert(drawer, !open);
  }, [open]);

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

  return (
    <>
      <div
        className="session-scrim"
        style={{ opacity: open ? 1 : 0, pointerEvents: open ? "auto" : "none" }}
        onClick={onClose}
        aria-hidden="true"
      />
      <aside
        className={`session-drawer ${open ? "is-open" : ""}`}
        ref={drawerRef}
        role="dialog"
        aria-label="Session history"
        aria-modal={open ? "true" : undefined}
        aria-hidden={!open}
        tabIndex={-1}
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
            className="h-11 flex-1 font-body text-body-sm text-text-primary placeholder:text-text-muted bg-transparent"
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
                    className={`session-row ${isActive ? "is-active" : ""}`}
                    onClick={() => onSelect(session)}
                    aria-current={isActive ? "true" : undefined}
                  >
                    <span className="session-row-dot" aria-hidden="true" />
                    <div className="min-w-0">
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
                    </div>
                  </button>
                );
              })}
            </div>
          ))}
        </div>
        <button
          className="flex min-h-11 items-center justify-center gap-2 w-full p-3 border-t border-surface-overlay text-body-sm text-text-muted transition-colors duration-150 ease-out-quart shrink-0 hover:bg-surface-overlay hover:text-text-secondary"
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
