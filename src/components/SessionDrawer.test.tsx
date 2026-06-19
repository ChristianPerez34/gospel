import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SessionDrawer } from "./SessionDrawer";
import type { Session } from "../types";

const baseDate = new Date("2026-06-19T12:00:00Z");

const sessions: Session[] = [
  {
    id: "session-main",
    title: "Current workspace",
    provider: "openai",
    model: "gpt-4o",
    timestamp: baseDate,
    messages: [],
    status: "idle",
    workspaceId: "workspace-main",
  },
  {
    id: "session-other",
    title: "Other workspace",
    provider: "openai",
    model: "gpt-4o",
    timestamp: new Date(baseDate.getTime() - 60 * 60 * 1000),
    messages: [],
    status: "idle",
    workspaceId: "workspace-other",
  },
];

function renderDrawer(overrides: Partial<ComponentProps<typeof SessionDrawer>> = {}) {
  return render(
    <SessionDrawer
      open
      sessions={sessions}
      activeSessionId="session-main"
      activeWorkspaceId="workspace-main"
      workspaceNames={{ "workspace-main": "Main", "workspace-other": "Other" }}
      onSelect={vi.fn()}
      onNewSession={vi.fn()}
      onClose={vi.fn()}
      {...overrides}
    />,
  );
}

describe("SessionDrawer", () => {
  afterEach(() => {
    cleanup();
  });

  it("shows a workspace badge for sessions outside the active workspace", () => {
    renderDrawer();
    const badge = screen.getAllByText("Other");
    expect(badge.length).toBe(1);
    expect(screen.queryByText("Main")).toBeNull();
  });

  it("calls onSelect for clicked session", () => {
    const onSelect = vi.fn();
    renderDrawer({ onSelect });

    fireEvent.click(screen.getByRole("button", { name: /Current workspace/ }));

    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith(sessions[0]);
  });
});
