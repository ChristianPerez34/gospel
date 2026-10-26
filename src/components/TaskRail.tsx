import { ChevronDown, Folder, History, LayoutDashboard, Plus, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useFocusTrap } from "../hooks/useFocusTrap";
import { shortcutLabel } from "../hooks/useSessionShortcuts";
import {
  ACTIVITY_LABELS,
  activityFor,
  buildRailLayout,
  formatElapsed,
  formatRelativeTime,
  holdRailLayout,
  needsYouLabel,
  type RailLayout,
  railOrder,
  type SessionActivity,
  type SessionActivityMap,
  sessionActivityPreview,
  sessionModelLabel,
} from "../sessionActivity";
import type { CurrentTurn, Session, Workspace } from "../types";

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
  overviewOpen?: boolean;
  onOpenOverview?: () => void;
  liveTurnsBySession?: Record<string, CurrentTurn | null>;
  trapFocus?: boolean;
  onClose?: () => void;
  onWorkspaceSwitch?: () => void;
  /** Reports the sessions in the order the rail currently shows them, so
   * session-switching shortcuts match what is on screen. */
  onDisplayedOrderChange?: (order: Session[]) => void;
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

function stateLine(session: Session, activity: SessionActivity, now: number): string {
  if (activity.state === "idle") return sessionModelLabel(session);
  if (activity.state === "running" && activity.runStartedAt !== undefined) {
    return `Running · ${formatElapsed(now - activity.runStartedAt)}`;
  }
  return ACTIVITY_LABELS[activity.state];
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
  onDisplayedOrderChange,
  overviewOpen = false,
  onOpenOverview,
  liveTurnsBySession = {},
  trapFocus = false,
  onClose = () => {},
  onWorkspaceSwitch,
}: TaskRailProps) {
  const railRef = useRef<HTMLElement>(null);
  useFocusTrap({ active: trapFocus, containerRef: railRef, onEscape: onClose });
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
        sessionModelLabel(s).toLowerCase().includes(q) ||
        project.toLowerCase().includes(q)
      );
    });
  }, [sessions, search, workspaceNames]);

  const liveLayout = useMemo(
    () => buildRailLayout(filtered, activityBySession),
    [filtered, activityBySession]
  );
  const layout = heldLayout ? holdRailLayout(heldLayout, liveLayout) : liveLayout;
  // Counts come from the displayed layout so the live-region summary matches
  // what the rail shows, including while a pointer hold freezes the order.
  const needsYouCount = layout.needsYou.length;
  const runningCount = layout.running.length;
  const showProjectInSections = new Set(sessions.map((s) => s.workspaceId ?? "")).size > 1;

  useTicker(collapsed ? null : runningCount > 0 ? 1000 : 30_000);

  useEffect(() => {
    onDisplayedOrderChange?.(railOrder(layout));
  });

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
    needsYouCount > 0 ? needsYouLabel(needsYouCount) : "",
    runningCount > 0 ? `${runningCount} running` : "",
  ]
    .filter(Boolean)
    .join(", ");

  const renderRow = (session: Session, inStatusSection: boolean) => {
    const activity = activityFor(activityBySession, session.id);
    const isActive = !overviewOpen && session.id === activeSessionId;
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
            {activity.state === "running" && liveTurnsBySession[session.id] && (
              <span className="task-rail-preview">
                {sessionActivityPreview(session, liveTurnsBySession[session.id])}
              </span>
            )}
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
    <aside
      ref={railRef}
      className="task-rail"
      aria-label="Tasks"
      data-testid="task-rail"
      {...(trapFocus ? { role: "dialog", "aria-modal": true } : {})}
    >
      {onOpenOverview && (
        <div className="task-rail-home">
          <button
            type="button"
            onClick={onOpenOverview}
            aria-current={overviewOpen ? "page" : undefined}
          >
            <LayoutDashboard size={16} aria-hidden="true" />
            Command center<span className="task-rail-home-count">{sessions.length}</span>
          </button>
        </div>
      )}
      <div className="task-rail-head">
        <button
          type="button"
          className="task-rail-new"
          aria-label="New task"
          onClick={onNewTask}
          title={`New task (${shortcutLabel("N")})`}
        >
          <Plus size={15} strokeWidth={1.6} aria-hidden="true" />
          <span>New task</span>
          <kbd>{shortcutLabel("N")}</kbd>
        </button>
        <div className="task-rail-search-row">
          <Search size={15} strokeWidth={1.6} aria-hidden="true" />
          <input
            className="task-rail-search"
            type="text"
            placeholder="Search tasks…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-label="Search tasks"
          />
        </div>
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
            {search ? "No tasks match." : "Your tasks will appear here."}
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

      <div className="task-rail-footer">
        {onWorkspaceSwitch && (
          <div className="task-rail-workspace">
            <span className="task-rail-workspace-label">Workspace for new tasks</span>
            <button
              type="button"
              className="task-rail-workspace-control"
              onClick={onWorkspaceSwitch}
              aria-label="Switch workspace"
              title={projectPath(activeWorkspaceId ?? "") ?? "Choose a workspace"}
            >
              <Folder size={16} strokeWidth={1.6} aria-hidden="true" />
              <span>
                {activeWorkspaceId ? projectLabel(activeWorkspaceId) : "Choose a workspace"}
                <span className="task-rail-workspace-path">
                  {projectPath(activeWorkspaceId ?? "") ?? "Open a project directory"}
                </span>
              </span>
              <ChevronDown size={12} aria-hidden="true" />
            </button>
          </div>
        )}
        <button type="button" className="task-rail-history" onClick={onOpenHistory}>
          <History aria-hidden="true" />
          History &amp; archive
        </button>
      </div>
    </aside>
  );
}
