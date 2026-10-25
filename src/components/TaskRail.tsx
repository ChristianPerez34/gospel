import { History, Plus } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { shortcutLabel } from "../hooks/useSessionShortcuts";
import {
  ACTIVITY_LABELS,
  activityFor,
  buildRailLayout,
  formatElapsed,
  formatRelativeTime,
  holdRailLayout,
  type RailLayout,
  type SessionActivity,
  type SessionActivityMap,
} from "../sessionActivity";
import type { Session, Workspace } from "../types";

interface TaskRailProps {
  sessions: Session[];
  activeSessionId?: string | null;
  activityBySession: SessionActivityMap;
  workspaces: Workspace[];
  workspaceNames: Record<string, string>;
  activeWorkspaceId?: string;
  onSelect: (session: Session) => void;
  onNewTask: () => void;
  onOpenHistory: () => void;
  collapsed?: boolean;
}

/** Re-render on an interval so run timers and relative times stay current. */
function useTicker(intervalMs: number | null) {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (intervalMs === null) return;
    const id = window.setInterval(() => setTick((tick) => tick + 1), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
}

function modelLabel(session: Session): string {
  return session.variant ? `${session.model} · ${session.variant}` : session.model;
}

function stateLine(session: Session, activity: SessionActivity, now: number): string {
  if (activity.state === "idle") return modelLabel(session);
  if (activity.state === "running" && activity.runStartedAt !== undefined) {
    return `Running · ${formatElapsed(now - activity.runStartedAt)}`;
  }
  return ACTIVITY_LABELS[activity.state];
}

function countLabel(count: number, singular: string, plural: string): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

export function TaskRail({
  sessions,
  activeSessionId,
  activityBySession,
  workspaces,
  workspaceNames,
  activeWorkspaceId,
  onSelect,
  onNewTask,
  onOpenHistory,
  collapsed = false,
}: TaskRailProps) {
  const [search, setSearch] = useState("");
  // Rows keep their place while the pointer is over the list, so a session
  // that changes state can't move out from under a click.
  const [heldLayout, setHeldLayout] = useState<RailLayout | null>(null);

  const workspaceById = useMemo(() => {
    const map = new Map<string, Workspace>();
    for (const ws of workspaces) map.set(ws.id, ws);
    return map;
  }, [workspaces]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) => {
      const project = workspaceNames[s.workspaceId ?? ""] ?? s.workspaceId ?? "";
      return (
        s.title.toLowerCase().includes(q) ||
        modelLabel(s).toLowerCase().includes(q) ||
        project.toLowerCase().includes(q)
      );
    });
  }, [sessions, search, workspaceNames]);

  const liveLayout = useMemo(
    () => buildRailLayout(filtered, activityBySession),
    [filtered, activityBySession]
  );
  const layout = heldLayout ? holdRailLayout(heldLayout, liveLayout) : liveLayout;

  let needsYouCount = 0;
  let runningCount = 0;
  for (const session of sessions) {
    const activity = activityFor(activityBySession, session.id);
    if (activity.needsYou) needsYouCount += 1;
    else if (activity.state === "running") runningCount += 1;
  }
  const showProjectInSections = new Set(sessions.map((s) => s.workspaceId ?? "")).size > 1;

  useTicker(collapsed ? null : runningCount > 0 ? 1000 : 30_000);

  useEffect(() => {
    if (collapsed) setHeldLayout(null);
  }, [collapsed]);

  if (collapsed) return null;

  const now = Date.now();

  const projectLabel = (key: string): string => {
    if (!key) return "Unscoped";
    return workspaceNames[key] ?? workspaceById.get(key)?.name ?? key;
  };

  const projectPath = (key: string): string | undefined => {
    if (!key) return undefined;
    return workspaceById.get(key)?.path;
  };

  const summary = [
    needsYouCount > 0 ? `${countLabel(needsYouCount, "session needs", "sessions need")} you` : "",
    runningCount > 0 ? `${runningCount} running` : "",
  ]
    .filter(Boolean)
    .join(", ");

  const renderRow = (session: Session, inStatusSection: boolean) => {
    const activity = activityFor(activityBySession, session.id);
    const isActive = session.id === activeSessionId;
    const title = session.title || "Untitled";
    const project = projectLabel(session.workspaceId ?? "");
    const stateTitle = activity.state === "idle" ? "" : ` — ${ACTIVITY_LABELS[activity.state]}`;
    return (
      <li key={session.id}>
        <button
          type="button"
          className={`task-rail-row is-${activity.state}${isActive ? " is-active" : ""}${activity.needsYou ? " is-needs-you" : ""}`}
          onClick={() => onSelect(session)}
          aria-current={isActive ? "true" : undefined}
          title={`${title} — ${project}${stateTitle}`}
        >
          <span className="task-rail-dot" aria-hidden="true" />
          <span className="task-rail-main">
            <span className="task-rail-heading">
              <span className="task-rail-title">{title}</span>
              {activity.state !== "running" && (
                <span className="task-rail-time">{formatRelativeTime(session.timestamp, now)}</span>
              )}
            </span>
            <span className="task-rail-meta">
              <span className="task-rail-state">{stateLine(session, activity, now)}</span>
              {inStatusSection && showProjectInSections && (
                <>
                  <span aria-hidden="true">·</span>
                  <span className="task-rail-meta-project">{project}</span>
                </>
              )}
            </span>
          </span>
        </button>
      </li>
    );
  };

  const renderStatusSection = (kind: "needs-you" | "running", label: string, items: Session[]) => {
    if (items.length === 0) return null;
    return (
      <section className="task-rail-group">
        <header className={`task-rail-group-head is-${kind}`}>
          <span className="task-rail-group-dot" aria-hidden="true" />
          <span className="task-rail-group-name">{label}</span>
          <span className="task-rail-group-count">{items.length}</span>
        </header>
        <ul className="task-rail-rows" aria-label={label}>
          {items.map((session) => renderRow(session, true))}
        </ul>
      </section>
    );
  };

  return (
    <aside className="task-rail" aria-label="Tasks" data-testid="task-rail">
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
        <button
          type="button"
          className="task-rail-new"
          onClick={onNewTask}
          title={`New task (${shortcutLabel("N")})`}
        >
          <Plus aria-hidden="true" />
          New task
        </button>
        <output className="sr-only" aria-live="polite">
          {summary}
        </output>
      </div>

      <div
        className="task-rail-list"
        onPointerEnter={() => setHeldLayout(liveLayout)}
        onPointerLeave={() => setHeldLayout(null)}
      >
        {filtered.length === 0 && (
          <div className="task-rail-empty">
            {search ? "No tasks match." : "No tasks yet — start one."}
          </div>
        )}
        {renderStatusSection("needs-you", "Needs you", layout.needsYou)}
        {renderStatusSection("running", "Running", layout.running)}
        {layout.projects.map((group) => (
          <section key={group.key || "unscoped"} className="task-rail-group">
            <header
              className="task-rail-group-head"
              title={projectPath(group.key) ?? projectLabel(group.key)}
            >
              <span className="task-rail-group-name">{projectLabel(group.key)}</span>
              {group.key === activeWorkspaceId && (
                <span className="task-rail-project-active">active</span>
              )}
              <span className="task-rail-group-count" title={`${group.items.length} tasks`}>
                {group.items.length}
              </span>
            </header>
            <ul className="task-rail-rows" aria-label={projectLabel(group.key)}>
              {group.items.map((session) => renderRow(session, false))}
            </ul>
          </section>
        ))}
      </div>

      <button type="button" className="task-rail-history" onClick={onOpenHistory}>
        <History aria-hidden="true" />
        History &amp; archive
      </button>
    </aside>
  );
}
