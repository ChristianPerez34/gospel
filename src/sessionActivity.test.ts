import { describe, expect, it } from "vitest";
import {
  buildRailLayout,
  deriveSessionActivity,
  formatElapsed,
  formatRelativeTime,
  holdRailLayout,
  nextSessionNeedingYou,
  railOrder,
  rankSessionsByActivity,
  type SessionActivityInput,
  type SessionActivityMap,
  sessionActivityPreview,
  stepSession,
} from "./sessionActivity";
import type { CurrentTurn, Session } from "./types";

function makeSession(id: string, workspaceId = "ws-1"): Session {
  return {
    id,
    title: id,
    provider: "openai",
    model: "gpt-4o",
    timestamp: new Date("2026-01-01T10:00:00Z"),
    messages: [],
    status: "idle",
    workspaceId,
  };
}

function turnWithApproval(status: "pending" | "approved"): CurrentTurn {
  return {
    id: "turn-1",
    createdAt: new Date("2026-01-01T10:00:00Z"),
    blocks: [
      {
        kind: "approval",
        id: "approval-1",
        toolName: "run_git_command",
        approvalKind: "command",
        title: "git push",
        summary: "Push the branch",
        reason: "Mutating command",
        risk: "mutating",
        status,
      },
    ],
  };
}

const baseInput: SessionActivityInput = {
  activeSessionId: null,
  currentTurn: null,
  liveTurnsBySession: {},
  statusBySession: {},
  streamingSessionIds: [],
  unreadSessionIds: [],
};

function ids(sessions: Session[]) {
  return sessions.map((session) => session.id);
}

describe("deriveSessionActivity", () => {
  it("marks a background session with a pending approval as needing you", () => {
    const activity = deriveSessionActivity([makeSession("a")], {
      ...baseInput,
      streamingSessionIds: ["a"],
      liveTurnsBySession: { a: turnWithApproval("pending") },
    });

    expect(activity.a).toMatchObject({ state: "needs-approval", needsYou: true });
  });

  it("reads the focused session's approvals from the current turn, not its stale stash", () => {
    const activity = deriveSessionActivity([makeSession("a")], {
      ...baseInput,
      activeSessionId: "a",
      streamingSessionIds: ["a"],
      currentTurn: turnWithApproval("approved"),
      liveTurnsBySession: { a: turnWithApproval("pending") },
      runStartedAtBySession: { a: 1_000 },
    });

    expect(activity.a).toEqual({ state: "running", needsYou: false, runStartedAt: 1_000 });
  });

  it("keeps a failure that arrived in the background in needs you until it is opened", () => {
    const sessions = [makeSession("a")];
    const failed = { ...baseInput, statusBySession: { a: "error" as const } };

    expect(deriveSessionActivity(sessions, { ...failed, unreadSessionIds: ["a"] }).a).toMatchObject(
      { state: "failed", needsYou: true }
    );
    expect(deriveSessionActivity(sessions, failed).a).toMatchObject({
      state: "failed",
      needsYou: false,
    });
  });

  it("marks unopened replies and leaves other sessions idle", () => {
    const activity = deriveSessionActivity([makeSession("a"), makeSession("b")], {
      ...baseInput,
      statusBySession: { a: "connected", b: "connected" },
      unreadSessionIds: ["a"],
    });

    expect(activity.a).toMatchObject({ state: "new-reply", needsYou: true });
    expect(activity.b).toMatchObject({ state: "idle", needsYou: false });
  });
});

describe("buildRailLayout", () => {
  const sessions = [
    makeSession("reply", "ws-1"),
    makeSession("run", "ws-2"),
    makeSession("idle-2", "ws-2"),
    makeSession("approval", "ws-1"),
    makeSession("idle-1", "ws-1"),
    makeSession("idle-3", "ws-2"),
  ];
  const activity: SessionActivityMap = {
    reply: { state: "new-reply", needsYou: true },
    run: { state: "running", needsYou: false },
    approval: { state: "needs-approval", needsYou: true },
  };

  it("puts approvals, then replies, then running sessions above the project groups", () => {
    const layout = buildRailLayout(sessions, activity);

    expect(ids(layout.needsYou)).toEqual(["approval", "reply"]);
    expect(ids(layout.running)).toEqual(["run"]);
    expect(layout.projects.map((group) => [group.key, ids(group.items)])).toEqual([
      ["ws-2", ["idle-2", "idle-3"]],
      ["ws-1", ["idle-1"]],
    ]);
    expect(ids(railOrder(layout))).toEqual([
      "approval",
      "reply",
      "run",
      "idle-2",
      "idle-3",
      "idle-1",
    ]);
  });

  it("ranks sessions for the palette without grouping the rest by project", () => {
    expect(ids(rankSessionsByActivity(sessions, activity))).toEqual([
      "approval",
      "reply",
      "run",
      "idle-2",
      "idle-1",
      "idle-3",
    ]);
  });
});

describe("holdRailLayout", () => {
  it("keeps the held order while the same sessions are present", () => {
    const a = makeSession("a");
    const b = makeSession("b");
    const held = buildRailLayout([a, b], { a: { state: "running", needsYou: false } });
    const next = buildRailLayout([{ ...a, title: "Renamed" }, b], {
      a: { state: "new-reply", needsYou: true },
    });

    const kept = holdRailLayout(held, next);

    expect(ids(kept.needsYou)).toEqual([]);
    expect(ids(kept.running)).toEqual(["a"]);
    expect(kept.running[0]?.title).toBe("Renamed");
  });

  it("releases the hold when a session is added or removed", () => {
    const held = buildRailLayout([makeSession("a")], {});
    const next = buildRailLayout([makeSession("b"), makeSession("a")], {});

    expect(holdRailLayout(held, next)).toBe(next);
  });
});

describe("session navigation", () => {
  const order = [makeSession("a"), makeSession("b"), makeSession("c")];

  it("steps forward and back, wrapping at either end", () => {
    expect(stepSession(order, "a", 1)?.id).toBe("b");
    expect(stepSession(order, "c", 1)?.id).toBe("a");
    expect(stepSession(order, "a", -1)?.id).toBe("c");
    expect(stepSession(order, null, 1)?.id).toBe("a");
    expect(stepSession(order, null, -1)?.id).toBe("c");
    expect(stepSession([], "a", 1)).toBeNull();
  });

  it("finds the next session that needs you after the current one", () => {
    const activity: SessionActivityMap = {
      a: { state: "new-reply", needsYou: true },
      c: { state: "needs-approval", needsYou: true },
    };

    expect(nextSessionNeedingYou(order, activity, "a")?.id).toBe("c");
    expect(nextSessionNeedingYou(order, activity, "c")?.id).toBe("a");
    expect(nextSessionNeedingYou(order, activity, null)?.id).toBe("a");
    expect(nextSessionNeedingYou(order, { a: { state: "new-reply", needsYou: true } }, "a")).toBe(
      null
    );
  });
});

describe("time formatting", () => {
  const now = new Date("2026-01-10T12:00:00Z").getTime();
  const minutesAgo = (minutes: number) => new Date(now - minutes * 60 * 1000);

  it("formats compact relative times", () => {
    expect(formatRelativeTime(minutesAgo(0.5), now)).toBe("now");
    expect(formatRelativeTime(minutesAgo(5), now)).toBe("5m");
    expect(formatRelativeTime(minutesAgo(3 * 60), now)).toBe("3h");
    expect(formatRelativeTime(minutesAgo(2 * 24 * 60), now)).toBe("2d");
    const old = minutesAgo(10 * 24 * 60);
    expect(formatRelativeTime(old, now)).toBe(
      old.toLocaleDateString([], { month: "short", day: "numeric" })
    );
    expect(formatRelativeTime(new Date("not a date"), now)).toBe("");
  });

  it("formats elapsed run time", () => {
    expect(formatElapsed(12_400)).toBe("12s");
    expect(formatElapsed(72_000)).toBe("1m 12s");
    expect(formatElapsed(3_900_000)).toBe("1h 5m");
    expect(formatElapsed(-500)).toBe("0s");
  });
});

describe("sessionActivityPreview", () => {
  it("normalizes multiline whitespace in text and message previews", () => {
    const session: Session = {
      ...makeSession("s1"),
      messages: [
        {
          id: "m1",
          role: "user",
          content: "First line\n  Second line\n\nThird line",
          timestamp: new Date(),
        },
      ],
    };
    expect(sessionActivityPreview(session)).toBe("First line Second line Third line");

    const turn: CurrentTurn = {
      id: "t1",
      createdAt: new Date(),
      blocks: [{ kind: "text", id: "b1", text: "Streaming\n\nmultiline\tresponse" }],
    };
    expect(sessionActivityPreview(session, turn)).toBe("Streaming multiline response");
  });
});
