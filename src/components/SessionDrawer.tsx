import { useState } from "react";
import type { Session } from "../types";
import "./SessionDrawer.css";

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

  return (
    <>
      {open && (
        <div
          className="session-drawer__scrim"
          onClick={onClose}
          aria-hidden="true"
        />
      )}
      <aside
        className={`session-drawer ${open ? "session-drawer--open" : ""}`}
        role="navigation"
        aria-label="Session history"
      >
        <div className="session-drawer__search">
          <svg
            className="session-drawer__search-icon"
            width="14"
            height="14"
            viewBox="0 0 14 14"
            fill="none"
          >
            <circle
              cx="5.5"
              cy="5.5"
              r="4"
              stroke="currentColor"
              strokeWidth="1.5"
            />
            <path
              d="M9 9L12.5 12.5"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
            />
          </svg>
          <input
            className="session-drawer__search-input"
            type="text"
            placeholder="Search sessions..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <div className="session-drawer__list">
          {Object.entries(filtered).map(([label, items]) => (
            <div key={label} className="session-drawer__group">
              <div className="session-drawer__group-label">{label}</div>
              {items.map((session) => (
                <button
                  key={session.id}
                  className={`session-drawer__item${
                    session.id === activeSessionId
                      ? " session-drawer__item--active"
                      : ""
                  }`}
                  onClick={() => onSelect(session)}
                >
                  <div className="session-drawer__item-title">
                    {session.title || "Untitled"}
                  </div>
                  <div className="session-drawer__item-meta">
                    <span className="session-drawer__item-model">
                      {session.model}
                    </span>
                    <time className="session-drawer__item-time">
                      {session.timestamp.toLocaleTimeString([], {
                        hour: "2-digit",
                        minute: "2-digit",
                      })}
                    </time>
                  </div>
                </button>
              ))}
            </div>
          ))}
        </div>
        <button className="session-drawer__new" onClick={onNewSession}>
          <svg
            width="14"
            height="14"
            viewBox="0 0 14 14"
            fill="none"
          >
            <path
              d="M7 3V11M3 7H11"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
            />
          </svg>
          New session
        </button>
      </aside>
    </>
  );
}