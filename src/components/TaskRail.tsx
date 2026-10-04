import { History, Plus } from "lucide-react";
import { useMemo, useState } from "react";
import type { AgentStatus, Session, Workspace } from "../types";

interface TaskRailProps {
  sessions: Session[];
  activeSessionId?: string | null;
  streamingSessionIds: string[];
  statusBySession: Record<string, AgentStatus>;
  workspaces: Workspace[];
  workspaceNames: Record<string, string>;
  activeWorkspaceId?: string;
  onSelect: (session: Session) => void;
  onNewTask: () => void;
  onOpenHistory: () => void;
  collapsed?: boolean;
}

function statusForSession(
  session: Session,
  streamingSessionIds: string[],
  statusBySession: Record<string, AgentStatus>
): AgentStatus {
  if (streamingSessionIds.includes(session.id)) return statusBySession[session.id] ?? "acting";
  return statusBySession[session.id] ?? "idle";
}

function formatTime(timestamp: Date): string {
  return timestamp.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function TaskRail({
  sessions,
  activeSessionId,
  streamingSessionIds,
  statusBySession,
  workspaces,
  workspaceNames,
  activeWorkspaceId,
  onSelect,
  onNewTask,
  onOpenHistory,
  collapsed = false,
}: TaskRailProps) {
  const [search, setSearch] = useState("");

  const workspaceById = useMemo(() => {
    const map = new Map<string, Workspace>();
    for (const ws of workspaces) map.set(ws.id, ws);
    return map;
  }, [workspaces]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) => {
      const modelLabel = s.variant ? `${s.model} ${s.variant}` : s.model;
      const project = workspaceNames[s.workspaceId ?? ""] ?? s.workspaceId ?? "";
      return (
        s.title.toLowerCase().includes(q) ||
        modelLabel.toLowerCase().includes(q) ||
        project.toLowerCase().includes(q)
      );
    });
  }, [sessions, search, workspaceNames]);

  const groups = useMemo(() => {
    const order: string[] = [];
    const byKey = new Map<string, Session[]>();
    for (const s of filtered) {
      const key = s.workspaceId ?? "";
      if (!byKey.has(key)) {
        byKey.set(key, []);
        order.push(key);
      }
      byKey.get(key)!.push(s);
    }
    return order.map((key) => ({ key, items: byKey.get(key)! }));
  }, [filtered]);

  const projectLabel = (key: string): string => {
    if (!key) return "Unscoped";
    return workspaceNames[key] ?? workspaceById.get(key)?.name ?? key;
  };

  const projectPath = (key: string): string | undefined => {
    if (!key) return undefined;
    return workspaceById.get(key)?.path;
  };

  if (collapsed) return null;

  return (
    <aside
      className="task-rail"
      aria-label="Tasks"
      data-testid="task-rail"
    >
      <div className="task-rail-head">
        <div className="task-rail-search-row">
          <svg
            className="text-text-muted shrink-0"
            width="14"
            height="14"
            viewBox="0 0 14 14"
            fill="none"
            aria-hidden="true"
          >
            <circle cx="5.5" cy="5.5" r="4" stroke="currentColor" strokeWidth="1.5" />
            <path
              d="M9 9L12.5 12.5"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
            />
          </svg>
          <input
            className="task-rail-search"
            type="text"
            placeholder="Search tasks or projects..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-label="Search tasks"
          />
        </div>
        <button type="button" className="task-rail-new" onClick={onNewTask}>
          <Plus aria-hidden="true" />
          New task
        </button>
      </div>

      <div className="task-rail-list">
        {groups.length === 0 && (
          <div className="task-rail-empty">
            {search ? "No tasks match." : "No tasks yet — start one."}
          </div>
        )}
        {groups.map((group) => {
          const runningInGroup = group.items.filter((s) =>
            streamingSessionIds.includes(s.id)
          ).length;
          return (
            <section key={group.key || "unscoped"} className="task-rail-group">
              <header
                className="task-rail-project"
                title={projectPath(group.key) ?? projectLabel(group.key)}
              >
                <span
                  className={`task-rail-project-dot${runningInGroup > 0 ? " is-live" : ""}`}
                  aria-hidden="true"
                />
                <span className="task-rail-project-name">{projectLabel(group.key)}</span>
                {group.key === activeWorkspaceId && (
                  <span className="task-rail-project-active">active</span>
                )}
                <span className="task-rail-project-count" title={`${group.items.length} tasks`}>
                  {group.items.length}
                </span>
                {runningInGroup > 0 && (
                  <span className="task-rail-project-live">{runningInGroup} live</span>
                )}
              </header>
              <ul className="task-rail-rows">
                {group.items.map((session) => {
                  const isActive = session.id === activeSessionId;
                  const live = streamingSessionIds.includes(session.id);
                  const st = statusForSession(session, streamingSessionIds, statusBySession);
                  const title = session.title || "Untitled";
                  return (
                    <li key={session.id}>
                      <button
                        type="button"
                        className={`task-rail-row${isActive ? " is-active" : ""}${live ? " is-live" : ""}`}
                        onClick={() => onSelect(session)}
                        aria-current={isActive ? "true" : undefined}
                        title={`${title} — ${projectLabel(group.key)}`}
                      >
                        <span
                          className={`task-rail-dot is-${st}`}
                          aria-hidden="true"
                        />
                        <span className="task-rail-main">
                          <span className="task-rail-title">{title}</span>
                          <span className="task-rail-meta">
                            <span className="task-rail-model">
                              {session.variant
                                ? `${session.model} · ${session.variant}`
                                : session.model}
                            </span>
                            {session.mode === "ReadOnly" && (
                              <span className="task-rail-plan">Plan</span>
                            )}
                            {live && <span className="task-rail-live">live</span>}
                            <time>{formatTime(session.timestamp)}</time>
                          </span>
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </section>
          );
        })}
      </div>

      <button type="button" className="task-rail-history" onClick={onOpenHistory}>
        <History aria-hidden="true" />
        History &amp; archive
      </button>
    </aside>
  );
}
