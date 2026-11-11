import { describe, expect, it } from "vitest";
import {
  applySubagentStep,
  applySubagentStepToTurn,
  applyVerificationStep,
  completeVerification,
  updateMessageById,
} from "./subagentActivity";
import {
  formatLiveToolActivity,
  subagentCardView,
  subagentCurrentLabel,
} from "./toolActivityCards";
import type { CurrentTurn, Message, SubagentActivity, ToolCallActivity } from "./types";

function exploration(steps: Parameters<typeof applySubagentStep>[2][]): SubagentActivity {
  return steps.reduce<SubagentActivity | undefined>(
    (activity, step) => applySubagentStep(activity, "exploration", step),
    undefined
  ) as SubagentActivity;
}

describe("applySubagentStep", () => {
  it("tracks a call until its result settles it", () => {
    const activity = exploration([
      { type: "toolCall", id: "c1", name: "read_file", arguments: { path: "src/llm.rs" } },
      { type: "toolResult", id: "c1", name: "read_file", result: "contents" },
    ]);
    expect(activity.steps).toEqual([
      {
        id: "c1",
        name: "read_file",
        arguments: { path: "src/llm.rs" },
        result: "contents",
        status: "completed",
      },
    ]);
  });

  it("ignores a repeated call id and records a result without a call", () => {
    const activity = exploration([
      { type: "toolCall", id: "c1", name: "grep", arguments: {} },
      { type: "toolCall", id: "c1", name: "grep", arguments: {} },
      { type: "toolResult", id: "orphan", name: "glob", result: "x" },
    ]);
    expect(activity.steps.map((step) => [step.id, step.status])).toEqual([
      ["c1", "calling"],
      ["orphan", "completed"],
    ]);
  });

  it("appends report text and replaces it on a reset", () => {
    const activity = exploration([
      { type: "text", text: "Found " },
      { type: "text", text: "two callers" },
    ]);
    expect(activity.text).toBe("Found two callers");
    expect(
      applySubagentStep(activity, "exploration", { type: "textReset", text: "Kept" }).text
    ).toBe("Kept");
  });

  it("only touches the matching tool block of a turn", () => {
    const turn: CurrentTurn = {
      id: "turn-1",
      blocks: [
        { kind: "tool", id: "parent", name: "delegate_exploration", status: "calling" },
        { kind: "tool", id: "other", name: "read_file", status: "calling" },
      ],
    } as CurrentTurn;
    const step = { type: "phase", label: "Reading diff" } as const;
    const next = applySubagentStepToTurn(turn, "parent", "review", step);
    expect(next.blocks[0]).toMatchObject({ subagent: { agent: "review", phase: "Reading diff" } });
    expect(next.blocks[1]).toBe(turn.blocks[1]);
    expect(applySubagentStepToTurn(turn, "missing", "review", step)).toBe(turn);
  });
});

describe("verification activity", () => {
  it("settles every step on a verdict and ignores steps after it", () => {
    let verification = applyVerificationStep(undefined, {
      type: "toolCall",
      id: "v1",
      name: "read_file",
      arguments: { path: "a.rs" },
    });
    expect(verification.status).toBe("running");
    verification = completeVerification(verification, {
      status: "concerns",
      summary: "  One gap  ",
      concerns: ["Missing test"],
    });
    expect(verification).toMatchObject({
      status: "concerns",
      summary: "One gap",
      concerns: ["Missing test"],
    });
    expect(verification.activity.steps[0].status).toBe("completed");
    const late = applyVerificationStep(verification, { type: "phase", label: "late" });
    expect(late).toBe(verification);
  });

  it("updates only the named message and keeps the array when none match", () => {
    const messages = [
      { id: "m1", role: "agent", content: "a", timestamp: new Date(0) },
      { id: "m2", role: "agent", content: "b", timestamp: new Date(0) },
    ] as Message[];
    const next = updateMessageById(messages, "m2", (m) => ({ ...m, content: "c" }));
    expect(next.map((m) => m.content)).toEqual(["a", "c"]);
    expect(next[0]).toBe(messages[0]);
    expect(updateMessageById(messages, "nope", (m) => m)).toBe(messages);
  });
});

describe("sub-agent labels", () => {
  const parent = (subagent: SubagentActivity): ToolCallActivity => ({
    id: "parent",
    name: "delegate_exploration",
    status: "calling",
    subagent,
  });

  it("names the running sub-agent call with its target", () => {
    const activity = exploration([
      { type: "toolCall", id: "c1", name: "read_file", arguments: { path: "src/llm.rs" } },
    ]);
    expect(formatLiveToolActivity(parent(activity))).toBe(
      "Delegate exploration: Read file src/llm.rs"
    );
    expect(formatLiveToolActivity(parent(activity), false)).toBe("Delegate exploration: Read file");
  });

  it("prefers report writing, then phase, then the last finished step", () => {
    const done = exploration([
      { type: "toolCall", id: "c1", name: "read_file", arguments: { path: "a.rs" } },
      { type: "toolResult", id: "c1", name: "read_file", result: "ok" },
    ]);
    expect(subagentCurrentLabel(done)).toBe("Read file a.rs");
    expect(
      subagentCurrentLabel(applySubagentStep(done, "exploration", { type: "text", text: "Report" }))
    ).toBe("Writing report");
    expect(
      subagentCurrentLabel(applySubagentStep(done, "review", { type: "phase", label: "Verifying" }))
    ).toBe("Verifying");
  });

  it("drops the sub-agent suffix once the parent call completes", () => {
    const activity = exploration([{ type: "toolCall", id: "c1", name: "grep", arguments: {} }]);
    expect(formatLiveToolActivity({ ...parent(activity), status: "completed" })).toBe(
      "Delegate exploration"
    );
  });

  it("builds a card view with nested steps", () => {
    const view = subagentCardView(
      exploration([
        { type: "toolCall", id: "c1", name: "read_file", arguments: { path: "a.rs" } },
        { type: "text", text: "Notes" },
      ])
    );
    expect(view.stepCount).toBe(1);
    expect(view.steps[0].status).toBe("calling");
    expect(view.text).toBe("Notes");
    expect(view.liveLabel).toBe("Read file a.rs");
  });
});
