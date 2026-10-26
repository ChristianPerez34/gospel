import { formatToolActivityLabel } from "./toolActivityCards";
import type { AgentStatus, CurrentTurn, Session } from "./types";

/** What a session is doing, as the user sees it in the task rail. */
export type SessionActivityState = "needs-approval" | "failed" | "new-reply" | "running" | "idle";

export interface SessionActivity {
  state: SessionActivityState;
  /** Waiting on the user: a pending approval, or a reply or failure that
   * arrived while another session was open. */
  needsYou: boolean;
  /** Epoch ms when the current run started; only set while running. */
  runStartedAt?: number;
}

export type SessionActivityMap = Record<string, SessionActivity>;

export interface SessionActivityInput {
  activeSessionId: string | null;
  currentTurn: CurrentTurn | null;
  liveTurnsBySession: Record<string, CurrentTurn | null>;
  statusBySession: Record<string, AgentStatus>;
  streamingSessionIds: string[];
  unreadSessionIds: string[];
  runStartedAtBySession?: Record<string, number>;
}

/** Display label for a session's model: "gpt-4o" or "gpt-4o · fast". */
export function sessionModelLabel(session: Session): string {
  return session.variant ? `${session.model} · ${session.variant}` : session.model;
}

/** "1 session needs you" / "3 sessions need you". */
export function needsYouLabel(count: number): string {
  return `${count} ${count === 1 ? "session needs" : "sessions need"} you`;
}

export const ACTIVITY_LABELS: Record<SessionActivityState, string> = {
  "needs-approval": "Needs approval",
  failed: "Failed",
  "new-reply": "New reply",
  running: "Running",
  idle: "Idle",
};

const IDLE_ACTIVITY: SessionActivity = { state: "idle", needsYou: false };

export function activityFor(activity: SessionActivityMap, sessionId: string): SessionActivity {
  return activity[sessionId] ?? IDLE_ACTIVITY;
}

/** Show useful work without exposing ephemeral reasoning or raw tool arguments. */
export function sessionActivityPreview(session: Session, turn?: CurrentTurn | null): string {
  const approval = turn?.blocks.find(
    (block) => block.kind === "approval" && block.status === "pending"
  );
  if (approval?.kind === "approval") return approval.title.trim().replace(/\s+/g, " ");
  const latest = turn ? [...turn.blocks].reverse() : [];
  const tool = latest.find((block) => block.kind === "tool" && block.status === "calling");
  if (tool?.kind === "tool") return formatToolActivityLabel(tool, true).trim().replace(/\s+/g, " ");
  const text = latest.find((block) => block.kind === "text" && block.text.trim());
  if (text?.kind === "text") return text.text.trim().replace(/\s+/g, " ");
  if (turn) return "Working on your task…";
  return (
    session.messages[session.messages.length - 1]?.content.trim().replace(/\s+/g, " ") ||
    "Open conversation to continue"
  );
}

function hasPendingApproval(turn: CurrentTurn | null | undefined): boolean {
  return (
    turn?.blocks.some((block) => block.kind === "approval" && block.status === "pending") ?? false
  );
}

function stateFor(
  needsApproval: boolean,
  running: boolean,
  failed: boolean,
  unread: boolean
): SessionActivityState {
  if (needsApproval) return "needs-approval";
  if (running) return "running";
  if (failed) return "failed";
  if (unread) return "new-reply";
  return "idle";
}

export function deriveSessionActivity(
  sessions: Session[],
  input: SessionActivityInput
): SessionActivityMap {
  const streaming = new Set(input.streamingSessionIds);
  const unreadIds = new Set(input.unreadSessionIds);
  const activity: SessionActivityMap = {};
  for (const { id } of sessions) {
    // The focused session's live turn is `currentTurn`; its liveTurnsBySession
    // entry can be a stale stash from before it was focused.
    const turn = id === input.activeSessionId ? input.currentTurn : input.liveTurnsBySession[id];
    const needsApproval = hasPendingApproval(turn);
    const running = streaming.has(id);
    const unread = unreadIds.has(id);
    activity[id] = {
      state: stateFor(needsApproval, running, input.statusBySession[id] === "error", unread),
      needsYou: needsApproval || (unread && !running),
      runStartedAt: running ? input.runStartedAtBySession?.[id] : undefined,
    };
  }
  return activity;
}

export interface RailProjectGroup {
  key: string;
  items: Session[];
}

export interface RailLayout {
  needsYou: Session[];
  running: Session[];
  projects: RailProjectGroup[];
}

/**
 * Sessions waiting on the user come first (approvals ahead of replies), then
 * running sessions, then the rest grouped by project. Each section keeps the
 * input order, which callers pass newest first.
 */
export function buildRailLayout(sessions: Session[], activity: SessionActivityMap): RailLayout {
  const approvals: Session[] = [];
  const otherNeedsYou: Session[] = [];
  const running: Session[] = [];
  const projects: RailProjectGroup[] = [];
  const projectByKey = new Map<string, RailProjectGroup>();

  for (const session of sessions) {
    const { state, needsYou } = activityFor(activity, session.id);
    if (needsYou) {
      (state === "needs-approval" ? approvals : otherNeedsYou).push(session);
      continue;
    }
    if (state === "running") {
      running.push(session);
      continue;
    }
    const key = session.workspaceId ?? "";
    let group = projectByKey.get(key);
    if (!group) {
      group = { key, items: [] };
      projectByKey.set(key, group);
      projects.push(group);
    }
    group.items.push(session);
  }

  return { needsYou: [...approvals, ...otherNeedsYou], running, projects };
}

/** Top-to-bottom order of the rail, used for keyboard navigation. */
export function railOrder(layout: RailLayout): Session[] {
  return [
    ...layout.needsYou,
    ...layout.running,
    ...layout.projects.flatMap((group) => group.items),
  ];
}

/**
 * Keep the held layout's order while it still holds exactly the same sessions,
 * swapping in the latest session data. Any added or removed session releases
 * the hold.
 */
export function holdRailLayout(held: RailLayout, next: RailLayout): RailLayout {
  const latest = new Map(railOrder(next).map((session) => [session.id, session]));
  const heldOrder = railOrder(held);
  if (heldOrder.length !== latest.size || heldOrder.some((session) => !latest.has(session.id))) {
    return next;
  }
  const refresh = (items: Session[]) => items.map((session) => latest.get(session.id) ?? session);
  return {
    needsYou: refresh(held.needsYou),
    running: refresh(held.running),
    projects: held.projects.map((group) => ({ key: group.key, items: refresh(group.items) })),
  };
}

/** Sessions that need you, then running sessions, then the rest in input order. */
export function rankSessionsByActivity(
  sessions: Session[],
  activity: SessionActivityMap
): Session[] {
  const { needsYou, running } = buildRailLayout(sessions, activity);
  const ranked = new Set([...needsYou, ...running].map((session) => session.id));
  return [...needsYou, ...running, ...sessions.filter((session) => !ranked.has(session.id))];
}

/** The session one step away from the current one, wrapping at either end. */
export function stepSession(
  order: Session[],
  currentId: string | null | undefined,
  step: 1 | -1
): Session | null {
  if (order.length === 0) return null;
  const index = order.findIndex((session) => session.id === currentId);
  if (index === -1) return step === 1 ? order[0] : order[order.length - 1];
  return order[(index + step + order.length) % order.length];
}

/** The next session after the current one that needs you, wrapping around. */
export function nextSessionNeedingYou(
  order: Session[],
  activity: SessionActivityMap,
  currentId: string | null | undefined
): Session | null {
  const start = order.findIndex((session) => session.id === currentId);
  for (let offset = 1; offset <= order.length; offset += 1) {
    const candidate = order[(start + offset + order.length) % order.length];
    if (candidate.id !== currentId && activityFor(activity, candidate.id).needsYou) {
      return candidate;
    }
  }
  return null;
}

const MINUTE = 60 * 1000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** Compact age for a rail row: "now", "5m", "3h", "2d", then a short date. */
export function formatRelativeTime(timestamp: Date, now: number): string {
  const time = timestamp.getTime();
  if (Number.isNaN(time)) return "";
  const age = Math.max(0, now - time);
  if (age < MINUTE) return "now";
  if (age < HOUR) return `${Math.floor(age / MINUTE)}m`;
  if (age < DAY) return `${Math.floor(age / HOUR)}h`;
  if (age < 7 * DAY) return `${Math.floor(age / DAY)}d`;
  return timestamp.toLocaleDateString([], { month: "short", day: "numeric" });
}

/** Elapsed run time: "12s", "1m 12s", "1h 5m". */
export function formatElapsed(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
}
