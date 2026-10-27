import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Session } from "../types";
import { TaskRail } from "./TaskRail";

const NOW = new Date("2026-01-01T10:05:00").getTime();

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

function rail(overrides: Partial<ComponentProps<typeof TaskRail>> = {}) {
  return (
    <TaskRail
      sessions={sessions}
      activeSessionId="s-1"
      activityBySession={{}}
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

function railList(name: string) {
  return screen.getByRole("list", { name });
}

function railListNames() {
  return screen.getAllByRole("list").map((list) => list.getAttribute("aria-label"));
}

describe("TaskRail", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("groups idle tasks under their project with their model and age", () => {
    render(rail());

    expect(railListNames()).toEqual(["Alpha", "Beta"]);
    expect(within(railList("Alpha")).getByText("Fix auth loop")).toBeDefined();
    expect(within(railList("Alpha")).getByText("Refactor inbox")).toBeDefined();
    const beta = railList("Beta");
    expect(within(beta).getByText("Add billing page")).toBeDefined();
    expect(within(beta).getByText("gpt-4o")).toBeDefined();
    expect(within(beta).getByText("5m")).toBeDefined();
  });

  it("lifts sessions that need you and running sessions above the projects", () => {
    render(
      rail({
        activityBySession: {
          "s-2": { state: "running", needsYou: false, runStartedAt: NOW - 72_000 },
          "s-3": { state: "new-reply", needsYou: true },
        },
      })
    );

    expect(railListNames()).toEqual(["Needs you", "Running", "Alpha"]);
    const needsYou = railList("Needs you");
    expect(within(needsYou).getByText("Refactor inbox")).toBeDefined();
    expect(within(needsYou).getByText("New reply")).toBeDefined();
    // Status sections mix projects, so their rows name the project.
    expect(within(needsYou).getByText("Alpha")).toBeDefined();
    const running = railList("Running");
    expect(within(running).getByText("Add billing page")).toBeDefined();
    expect(within(running).getByText("Running · 1m 12s")).toBeDefined();
    expect(within(running).getByText("Beta")).toBeDefined();
    expect(screen.getByTitle("Refactor inbox — Alpha — New reply")).toBeDefined();
    expect(screen.getByTitle("Add billing page — Beta — Running")).toBeDefined();
  });

  it("advances the run timer while a session is running", () => {
    render(
      rail({
        activityBySession: {
          "s-2": { state: "running", needsYou: false, runStartedAt: NOW - 5_000 },
        },
      })
    );
    expect(screen.getByText("Running · 5s")).toBeDefined();

    act(() => {
      vi.advanceTimersByTime(2_000);
    });

    expect(screen.getByText("Running · 7s")).toBeDefined();
  });

  it("announces how many sessions need you and how many are running", () => {
    render(
      rail({
        activityBySession: {
          "s-2": { state: "running", needsYou: false },
          "s-3": { state: "needs-approval", needsYou: true },
        },
      })
    );

    expect(screen.getByRole("status").textContent).toBe("1 session needs you, 1 running");
    expect(within(railList("Needs you")).getByText("Needs approval")).toBeDefined();
  });

  it("keeps rows in place while the pointer is over the list", () => {
    const { rerender } = render(rail());
    const row = screen.getByTitle("Refactor inbox — Alpha");
    fireEvent.pointerEnter(row);

    rerender(rail({ activityBySession: { "s-3": { state: "new-reply", needsYou: true } } }));

    expect(railListNames()).toEqual(["Alpha", "Beta"]);
    expect(within(railList("Alpha")).getByText("New reply")).toBeDefined();

    fireEvent.pointerLeave(row);

    expect(railListNames()).toEqual(["Needs you", "Alpha", "Beta"]);
    expect(within(railList("Needs you")).getByText("Refactor inbox")).toBeDefined();
  });

  it("marks the active task current", () => {
    render(rail());
    expect(screen.getByTitle("Fix auth loop — Alpha").getAttribute("aria-current")).toBe("true");
    expect(screen.getByTitle("Refactor inbox — Alpha").getAttribute("aria-current")).toBeNull();
  });

  it("filters by task title and project name", () => {
    render(rail());
    fireEvent.change(screen.getByLabelText("Search tasks"), {
      target: { value: "billing" },
    });
    expect(screen.getByText("Add billing page")).toBeDefined();
    expect(screen.queryByText("Fix auth loop")).toBeNull();

    fireEvent.change(screen.getByLabelText("Search tasks"), {
      target: { value: "alpha" },
    });
    expect(screen.getByText("Fix auth loop")).toBeDefined();
    expect(screen.queryByText("Add billing page")).toBeNull();
  });

  it("reports its displayed order so session shortcuts can match it", () => {
    const onDisplayedOrderChange = vi.fn();
    render(rail({ onDisplayedOrderChange }));
    expect(onDisplayedOrderChange).toHaveBeenLastCalledWith([
      sessions[0],
      sessions[2],
      sessions[1],
    ]);

    fireEvent.change(screen.getByLabelText("Search tasks"), {
      target: { value: "billing" },
    });
    expect(onDisplayedOrderChange).toHaveBeenLastCalledWith([sessions[1]]);
  });

  it("calls onSelect, onNewTask, and onOpenHistory", () => {
    const onSelect = vi.fn();
    const onNewTask = vi.fn();
    const onOpenHistory = vi.fn();
    render(rail({ onSelect, onNewTask, onOpenHistory }));

    fireEvent.click(screen.getByTitle("Add billing page — Beta"));
    expect(onSelect).toHaveBeenCalledWith(sessions[1]);

    fireEvent.click(screen.getByRole("button", { name: "New task" }));
    expect(onNewTask).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: /History/ }));
    expect(onOpenHistory).toHaveBeenCalledTimes(1);
  });

  it("renders nothing when collapsed", () => {
    const { container } = render(rail({ collapsed: true }));
    expect(container.firstChild).toBeNull();
  });

  it("keeps the default workspace and directory visible in navigation", () => {
    const onWorkspaceSwitch = vi.fn();
    render(rail({ onWorkspaceSwitch }));
    const picker = screen.getByRole("button", { name: "Switch workspace" });
    expect(picker.textContent).toContain("Alpha");
    expect(picker.textContent).toContain("/tmp/alpha");
    fireEvent.click(picker);
    expect(onWorkspaceSwitch).toHaveBeenCalledTimes(1);
  });

  it("exposes compact navigation as a dialog and closes it with Escape", () => {
    const onClose = vi.fn();
    render(rail({ trapFocus: true, onClose }));
    expect(screen.getByRole("dialog", { name: "Tasks" }).getAttribute("aria-modal")).toBe("true");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
