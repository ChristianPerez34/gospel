import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CommandPalette } from "./CommandPalette";
import { modelOptionId, type Session, type Workspace } from "../types";

const now = new Date("2026-06-19T12:00:00Z");
const sessions: Session[] = [
  {
    id: "session-main",
    title: "Session in active workspace",
    provider: "openai",
    model: "gpt-4o",
    timestamp: now,
    messages: [],
    status: "idle",
    workspaceId: "workspace-main",
  },
  {
    id: "session-other",
    title: "Session in other workspace",
    provider: "openai",
    model: "gpt-4o",
    timestamp: new Date(now.getTime() - 60 * 60 * 1000),
    messages: [],
    status: "idle",
    workspaceId: "workspace-other",
  },
];

const workspace: Workspace = {
  id: "workspace-main",
  name: "Main",
  path: "/tmp/main",
  sessionCount: 2,
};

const models = [
  { id: modelOptionId("openai", "gpt-4o"), name: "gpt-4o", provider: "openai", configured: true },
];

function renderPalette(overrides = {}) {
  return render(
    <CommandPalette
      open
      sessions={sessions}
      activeSessionId="session-main"
      workspace={workspace}
      models={models}
      selectedModelId={modelOptionId("openai", "gpt-4o")}
      workspaceNames={{ "workspace-main": "Main", "workspace-other": "Other" }}
      onClose={vi.fn()}
      onSelectSession={vi.fn()}
      onNewSession={vi.fn()}
      onOpenSettings={vi.fn()}
      onOpenWorkspaceSwitcher={vi.fn()}
      onToggleSessions={vi.fn()}
      onToggleReview={vi.fn()}
      onSelectModel={vi.fn()}
      {...overrides}
    />,
  );
}

describe("CommandPalette", () => {
  afterEach(() => {
    cleanup();
  });

  it("shows workspace names in session search details", () => {
    renderPalette();

    expect(screen.getByText(/gpt-4o · Main/)).toBeTruthy();
    expect(screen.getByText(/gpt-4o · Other/)).toBeTruthy();
  });

  it("searches by workspace name across sessions", () => {
    renderPalette();
    fireEvent.change(screen.getByLabelText("Search commands"), {
      target: { value: "other" },
    });

    expect(screen.getByRole("button", { name: /Session in other workspace/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Session in active workspace/ })).toBeNull();
  });
});
