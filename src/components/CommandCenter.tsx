import { ArrowUpRight, CircleDot, Folder, Plus, Search, SlidersHorizontal } from "lucide-react";
import { useEffect, useState } from "react";
import {
  ACTIVITY_LABELS,
  activityFor,
  buildRailLayout,
  formatElapsed,
  formatRelativeTime,
  holdRailLayout,
  type RailLayout,
  railOrder,
  type SessionActivityMap,
  sessionActivityPreview,
  sessionModelLabel,
} from "../sessionActivity";
import type { CurrentTurn, Session, Workspace } from "../types";
import { Button } from "./ui/button";

interface CommandCenterProps {
  sessions: Session[];
  activityBySession: SessionActivityMap;
  liveTurnsBySession: Record<string, CurrentTurn | null>;
  workspaces: Workspace[];
  onSelect: (session: Session) => void;
  onNewTask: () => void;
  onManageWorkspaces: () => void;
}

type TaskFilter = "all" | "running" | "needs-you";

export function CommandCenter({
  sessions,
  activityBySession,
  liveTurnsBySession,
  workspaces,
  onSelect,
  onNewTask,
  onManageWorkspaces,
}: CommandCenterProps) {
  const [filter, setFilter] = useState<TaskFilter>("all");
  const [workspaceId, setWorkspaceId] = useState("");
  const [search, setSearch] = useState("");
  const [heldLayout, setHeldLayout] = useState<RailLayout | null>(null);
  const [now, setNow] = useState(Date.now);
  const running = sessions.filter(
    (session) => activityFor(activityBySession, session.id).state === "running"
  ).length;
  const needsYou = sessions.filter(
    (session) => activityFor(activityBySession, session.id).needsYou
  ).length;
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), running > 0 ? 1000 : 30_000);
    return () => window.clearInterval(timer);
  }, [running]);

  const projectName = (session: Session) =>
    workspaces.find((workspace) => workspace.id === session.workspaceId)?.name ??
    session.workspaceId ??
    "Unscoped";
  const query = search.trim().toLowerCase();
  const visible = sessions.filter((session) => {
    const activity = activityFor(activityBySession, session.id);
    return (
      (!workspaceId || session.workspaceId === workspaceId) &&
      (filter === "all" ||
        (filter === "running" ? activity.state === "running" : activity.needsYou)) &&
      (!query ||
        `${session.title} ${projectName(session)} ${sessionModelLabel(session)}`
          .toLowerCase()
          .includes(query))
    );
  });
  const liveLayout = buildRailLayout(visible, activityBySession);
  const ordered = railOrder(heldLayout ? holdRailLayout(heldLayout, liveLayout) : liveLayout);

  return (
    <main className="command-center" aria-label="Command center">
      <header className="command-center-heading">
        <div>
          <div className="command-center-eyebrow">
            <span className="live-dot" /> YOUR WORK, IN VIEW
          </div>
          <h1>
            Command center<span className="heading-period">.</span>
          </h1>
          <p>One place for every task. Keep your projects moving.</p>
        </div>
        <Button className="command-primary" onClick={onNewTask}>
          <Plus size={15} aria-hidden="true" /> New task
        </Button>
      </header>

      <section className="command-center-pulse" aria-label="Across all workspaces">
        <span>
          <span className={`live-dot${running ? " is-running" : ""}`} />{" "}
          {running ? `${running} running` : "No tasks running"}
        </span>
        <span className={needsYou ? "attention-text" : ""}>
          <CircleDot size={13} aria-hidden="true" />{" "}
          {needsYou ? `${needsYou} need${needsYou === 1 ? "s" : ""} you` : "You're all caught up"}
        </span>
        <button type="button" onClick={onManageWorkspaces}>
          <Folder size={13} aria-hidden="true" /> {workspaces.length}{" "}
          {workspaces.length === 1 ? "workspace" : "workspaces"}
          <ArrowUpRight size={12} aria-hidden="true" />
        </button>
      </section>

      <section className="command-task-section" aria-label="Task overview">
        <div className="command-task-toolbar">
          <fieldset className="command-filters" aria-label="Filter tasks">
            {(
              [
                ["all", "All tasks", sessions.length],
                ["running", "Running", running],
                ["needs-you", "Needs you", needsYou],
              ] as const
            ).map(([value, label, count]) => (
              <button
                key={value}
                type="button"
                aria-pressed={filter === value}
                onClick={() => setFilter(value)}
              >
                {label}
                <span>{count}</span>
              </button>
            ))}
          </fieldset>
          <div className="command-task-tools">
            <label className="command-search">
              <Search size={14} aria-hidden="true" />
              <input
                aria-label="Search all tasks"
                placeholder="Search tasks…"
                value={search}
                onChange={(event) => setSearch(event.target.value)}
              />
            </label>
            <label className="command-workspace-filter">
              <SlidersHorizontal size={13} aria-hidden="true" />
              <select
                aria-label="Filter by workspace"
                value={workspaceId}
                onChange={(event) => setWorkspaceId(event.target.value)}
              >
                <option value="">All workspaces</option>
                {workspaces.map((workspace) => (
                  <option key={workspace.id} value={workspace.id}>
                    {workspace.name}
                  </option>
                ))}
              </select>
            </label>
          </div>
        </div>
        <div className="command-task-columns" aria-hidden="true">
          <span>Task / latest activity</span>
          <span>Workspace</span>
          <span>Status</span>
          <span>Updated</span>
        </div>
        <ul
          className="command-task-list"
          aria-label="All workspace tasks"
          onPointerEnter={() => setHeldLayout(liveLayout)}
          onPointerLeave={() => setHeldLayout(null)}
        >
          {ordered.map((session) => {
            const activity = activityFor(activityBySession, session.id);
            const preview = sessionActivityPreview(session, liveTurnsBySession[session.id]);
            return (
              <li key={session.id}>
                <button
                  type="button"
                  className={`command-task-row is-${activity.state}`}
                  onClick={() => onSelect(session)}
                  aria-label={`Open ${session.title || "Untitled"} in ${projectName(session)}`}
                >
                  <span className="command-task-content">
                    <span className="command-task-icon">
                      <CircleDot size={16} aria-hidden="true" />
                    </span>
                    <span className="command-task-copy">
                      <strong>{session.title || "Untitled"}</strong>
                      <span className="command-task-preview">
                        {preview.replace(/\s+/g, " ").slice(0, 180)}
                      </span>
                      <span className="command-task-model">{sessionModelLabel(session)}</span>
                    </span>
                  </span>
                  <span className="command-task-project" title={projectName(session)}>
                    <Folder size={13} aria-hidden="true" />
                    <span>{projectName(session)}</span>
                  </span>
                  <span className={`task-status is-${activity.state}`}>
                    <span className="live-dot" />
                    {ACTIVITY_LABELS[activity.state]}
                  </span>
                  <span className="command-task-age">
                    {activity.state === "running" && activity.runStartedAt !== undefined
                      ? formatElapsed(now - activity.runStartedAt)
                      : formatRelativeTime(session.timestamp, now)}
                    <ArrowUpRight size={13} aria-hidden="true" />
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
        {visible.length === 0 && (
          <div className="command-empty">
            <CircleDot size={28} strokeWidth={1} aria-hidden="true" />
            <h2>
              {sessions.length === 0 ? "A clear space. A fresh start." : "No tasks in this view"}
            </h2>
            <p>
              {sessions.length === 0
                ? "Start a new task to choose a workspace and give Gospel an instruction."
                : "Try another workspace, status, or search."}
            </p>
            {sessions.length > 0 && (
              <button
                type="button"
                onClick={() => {
                  setSearch("");
                  setFilter("all");
                  setWorkspaceId("");
                }}
              >
                Show all tasks
              </button>
            )}
          </div>
        )}
        <footer className="command-task-footer">
          <span>
            {visible.length} {visible.length === 1 ? "task" : "tasks"} in view
          </span>
          <span>Sessions keep running when you switch.</span>
        </footer>
      </section>
    </main>
  );
}

/** A compact view of other tasks stays visible beside the focused conversation. */
export function LiveTaskMonitor({
  sessions,
  activityBySession,
  liveTurnsBySession,
  workspaces,
  onSelect,
  onNewTask,
  onManageWorkspaces,
  activeSessionId,
}: CommandCenterProps & { activeSessionId: string | null }) {
  const [heldLayout, setHeldLayout] = useState<RailLayout | null>(null);
  const live = sessions.filter(
    (session) =>
      session.id !== activeSessionId &&
      (activityFor(activityBySession, session.id).needsYou ||
        activityFor(activityBySession, session.id).state === "running")
  );
  const liveLayout = buildRailLayout(live, activityBySession);
  const ordered = railOrder(heldLayout ? holdRailLayout(heldLayout, liveLayout) : liveLayout);
  if (live.length === 0) return null;
  return (
    <aside
      className="live-task-monitor"
      aria-label="Other active tasks"
      onPointerEnter={() => setHeldLayout(liveLayout)}
      onPointerLeave={() => setHeldLayout(null)}
    >
      <header>
        <span className="live-dot" />
        <h2>Happening elsewhere</h2>
        <span>{live.length}</span>
      </header>
      <p>Your other projects are still moving.</p>
      {ordered.map((session) => {
        const activity = activityFor(activityBySession, session.id);
        return (
          <button
            type="button"
            key={session.id}
            onClick={() => onSelect(session)}
            className="live-task-item"
          >
            <span className="live-task-project">
              <Folder size={12} aria-hidden="true" />
              {workspaces.find((workspace) => workspace.id === session.workspaceId)?.name ??
                session.workspaceId ??
                "Unscoped"}
            </span>
            <strong>{session.title || "Untitled"}</strong>
            <span className={`task-status is-${activity.state}`}>
              <span className="live-dot" />
              {ACTIVITY_LABELS[activity.state]}
            </span>
            <span className="live-task-preview">
              {sessionActivityPreview(session, liveTurnsBySession[session.id])
                .replace(/\s+/g, " ")
                .slice(0, 160)}
            </span>
            <span className="live-task-open">
              Open task <ArrowUpRight size={12} aria-hidden="true" />
            </span>
          </button>
        );
      })}
      <button type="button" className="live-task-new" onClick={onNewTask}>
        <Plus size={14} aria-hidden="true" /> Start another task
      </button>
      <button type="button" className="live-task-new" onClick={onManageWorkspaces}>
        <Folder size={14} aria-hidden="true" /> Manage workspaces
      </button>
    </aside>
  );
}
