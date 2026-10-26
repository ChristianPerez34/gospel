import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { sessionActivityPreview } from "../sessionActivity";
import type { CurrentTurn, Session } from "../types";
import { CommandCenter, LiveTaskMonitor } from "./CommandCenter";

const workspaces = [
  { id: "alpha", name: "Alpha", path: "/alpha", sessionCount: 1 },
  { id: "beta", name: "Beta", path: "/beta", sessionCount: 1 },
];
const sessions: Session[] = workspaces.map((workspace, index) => ({
  id: `task-${index}`,
  title: index === 0 ? "Fix authentication" : "Build billing",
  provider: "openai",
  model: "gpt-4o",
  workspaceId: workspace.id,
  timestamp: new Date(),
  status: "idle",
  messages: [],
}));
const liveTurn: CurrentTurn = {
  id: "turn",
  createdAt: new Date(),
  blocks: [
    { kind: "reasoning", id: "private", text: "Private reasoning", phase: "delta" },
    {
      kind: "tool",
      id: "tool",
      name: "read_file",
      status: "calling",
      arguments: { secret: "private argument" },
    },
  ],
};
const props = {
  sessions,
  workspaces,
  activityBySession: {
    "task-0": { state: "running" as const, needsYou: false },
    "task-1": { state: "new-reply" as const, needsYou: true },
  },
  liveTurnsBySession: { "task-0": liveTurn },
  onSelect: vi.fn(),
  onNewTask: vi.fn(),
  onManageWorkspaces: vi.fn(),
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("CommandCenter", () => {
  it("combines status, workspace, and search filters without changing the workspace", () => {
    render(<CommandCenter {...props} />);
    const list = screen.getByRole("list", { name: "All workspace tasks" });
    expect(within(list).getAllByRole("button")).toHaveLength(2);
    expect(within(list).getByText("Read file...")).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: /Running/ }));
    expect(within(list).getAllByRole("button")).toHaveLength(1);
    fireEvent.change(screen.getByRole("combobox", { name: "Filter by workspace" }), {
      target: { value: "beta" },
    });
    expect(within(list).queryAllByRole("button")).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "Show all tasks" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Search all tasks" }), {
      target: { value: "beta" },
    });
    fireEvent.click(within(list).getByRole("button", { name: "Open Build billing in Beta" }));
    expect(props.onSelect).toHaveBeenCalledWith(sessions[1]);
    expect(props.onManageWorkspaces).not.toHaveBeenCalled();
  });

  it("updates activity and the needs-you filter as background work finishes", () => {
    const { rerender } = render(<CommandCenter {...props} />);
    fireEvent.click(screen.getByRole("button", { name: /Needs you/ }));
    const list = screen.getByRole("list", { name: "All workspace tasks" });
    expect(within(list).getAllByRole("button")).toHaveLength(1);
    rerender(
      <CommandCenter
        {...props}
        activityBySession={{
          ...props.activityBySession,
          "task-0": { state: "new-reply", needsYou: true },
        }}
        liveTurnsBySession={{}}
      />
    );
    expect(within(list).getAllByRole("button")).toHaveLength(2);
    expect(screen.queryByText("Read file...")).toBeNull();
  });

  it("keeps other workspaces visible while a conversation is focused", () => {
    render(<LiveTaskMonitor {...props} activeSessionId="task-1" />);
    const monitor = screen.getByRole("complementary", { name: "Other active tasks" });
    expect(within(monitor).queryByText("Build billing")).toBeNull();
    expect(within(monitor).getByText("Alpha")).toBeDefined();
    expect(within(monitor).getByText("Read file...")).toBeDefined();
    fireEvent.click(within(monitor).getByRole("button", { name: /Fix authentication/ }));
    expect(props.onSelect).toHaveBeenCalledWith(sessions[0]);
  });

  it("shows approval titles first and never previews reasoning or tool arguments", () => {
    expect(sessionActivityPreview(sessions[0], liveTurn)).toBe("Read file...");
    expect(sessionActivityPreview(sessions[0], { ...liveTurn, blocks: [liveTurn.blocks[0]] })).toBe(
      "Working on your task…"
    );
    expect(
      sessionActivityPreview(sessions[0], {
        ...liveTurn,
        blocks: [
          ...liveTurn.blocks,
          {
            kind: "approval",
            id: "approval",
            toolName: "git",
            approvalKind: "command",
            title: "Approve commit",
            summary: "Commit changes",
            reason: "Mutation",
            risk: "mutating",
            status: "pending",
          },
        ],
      })
    ).toBe("Approve commit");
    expect(sessionActivityPreview(sessions[0])).toBe("Open conversation to continue");
  });
});
