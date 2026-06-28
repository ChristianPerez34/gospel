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

    fireEvent.click(screen.getByRole("button", { name: /^Current workspace/ }));

    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith(sessions[0]);
  });

  it("calls onArchiveSession for the row archive action", () => {
    const onArchiveSession = vi.fn();
    renderDrawer({ onArchiveSession });

    fireEvent.click(screen.getByRole("button", { name: "Archive Current workspace" }));

    expect(onArchiveSession).toHaveBeenCalledTimes(1);
    expect(onArchiveSession).toHaveBeenCalledWith(sessions[0]);
  });

  it("shows archived sessions with restore and permanent delete actions", () => {
    const onRestoreSession = vi.fn();
    const onDeleteArchivedSession = vi.fn();
    renderDrawer({
      sessions: [],
      archivedSessions: [sessions[1]],
      showArchived: true,
      onShowArchivedChange: vi.fn(),
      onRestoreSession,
      onDeleteArchivedSession,
    });

    fireEvent.click(screen.getByRole("button", { name: "Restore Other workspace" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete Other workspace permanently" }));

    expect(onRestoreSession).toHaveBeenCalledWith(sessions[1]);
    expect(onDeleteArchivedSession).toHaveBeenCalledWith(sessions[1]);
  });

  it("archives selected visible sessions", () => {
    const onArchiveSessions = vi.fn();
    renderDrawer({ onArchiveSessions });

    fireEvent.click(screen.getByRole("button", { name: "Select visible" }));
    fireEvent.click(screen.getByRole("button", { name: "Archive selected" }));

    expect(onArchiveSessions).toHaveBeenCalledTimes(1);
    expect(onArchiveSessions).toHaveBeenCalledWith(sessions);
  });

  it("imports archived session JSON from the archive panel", () => {
    const onImportArchivedSessions = vi.fn();
    renderDrawer({
      sessions: [],
      archivedSessions: [sessions[1]],
      showArchived: true,
      onShowArchivedChange: vi.fn(),
      onImportArchivedSessions,
    });

    fireEvent.click(screen.getByRole("button", { name: /Import archive/ }));
    fireEvent.change(screen.getByLabelText("Archive import JSON"), {
      target: { value: "{\"version\":1,\"sessions\":[]}" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Import" }));

    expect(onImportArchivedSessions).toHaveBeenCalledWith("{\"version\":1,\"sessions\":[]}");
  });
});
