import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Session } from "../types";
import { TaskRail } from "./TaskRail";

const workspaces = [
  { id: "ws-1", name: "Alpha", path: "/tmp/alpha", sessionCount: 2 },
  { id: "ws-2", name: "Beta", path: "/tmp/beta", sessionCount: 1 },
];

const workspaceNames = { "ws-1": "Alpha", "ws-2": "Beta" };

function makeSession(overrides: Partial<Session> & { id: string }): Session {
  return {
    title: "Task",
    provider: "openai",
    model: "gpt-4o",
    variant: null,
    timestamp: new Date("2026-01-01T10:00:00"),
    messages: [],
    status: "idle",
    ...overrides,
  };
}

const sessions: Session[] = [
  makeSession({ id: "s-1", title: "Fix auth loop", workspaceId: "ws-1" }),
  makeSession({ id: "s-2", title: "Add billing page", workspaceId: "ws-2" }),
  makeSession({ id: "s-3", title: "Refactor inbox", workspaceId: "ws-1" }),
];

function renderRail(overrides = {}) {
  return render(
    <TaskRail
      sessions={sessions}
      activeSessionId="s-1"
      streamingSessionIds={["s-2"]}
      statusBySession={{ "s-2": "acting" }}
      workspaces={workspaces}
      workspaceNames={workspaceNames}
      activeWorkspaceId="ws-1"
      onSelect={vi.fn()}
      onNewTask={vi.fn()}
      onOpenHistory={vi.fn()}
      {...overrides}
    />
  );
}

describe("TaskRail", () => {
  afterEach(() => {
    cleanup();
  });

  it("groups tasks under their project with counts", () => {
    renderRail();
    expect(screen.getByText("Alpha")).toBeDefined();
    expect(screen.getByText("Beta")).toBeDefined();
    expect(screen.getByText("Fix auth loop")).toBeDefined();
    expect(screen.getByText("Add billing page")).toBeDefined();
  });

  it("marks the running task live and the active task current", () => {
    renderRail();
    const liveBadges = screen.getAllByText("live");
    expect(liveBadges.length).toBeGreaterThan(0);
    const activeRow = screen.getByTitle("Fix auth loop — Alpha");
    expect(activeRow.getAttribute("aria-current")).toBe("true");
  });

  it("filters by task title and project name", () => {
    renderRail();
    fireEvent.change(screen.getByLabelText("Search tasks"), {
      target: { value: "billing" },
    });
    expect(screen.getByText("Add billing page")).toBeDefined();
    expect(screen.queryByText("Fix auth loop")).toBeNull();
  });

  it("calls onSelect, onNewTask, and onOpenHistory", () => {
    const onSelect = vi.fn();
    const onNewTask = vi.fn();
    const onOpenHistory = vi.fn();
    renderRail({ onSelect, onNewTask, onOpenHistory });

    fireEvent.click(screen.getByTitle("Add billing page — Beta"));
    expect(onSelect).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "New task" }));
    expect(onNewTask).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: /History/ }));
    expect(onOpenHistory).toHaveBeenCalledTimes(1);
  });

  it("renders nothing when collapsed", () => {
    const { container } = renderRail({ collapsed: true });
    expect(container.firstChild).toBeNull();
  });
});
