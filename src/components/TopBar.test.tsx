import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TopBar } from "./TopBar";

const workspace = {
  id: "workspace-main",
  name: "Main workspace",
  path: "/tmp/main-workspace",
  sessionCount: 1,
};

function renderTopBar(overrides: Partial<ComponentProps<typeof TopBar>> = {}) {
  return render(
    <TopBar
      workspace={workspace}
      sessionTitle="Current session"
      sessionMode="Build"
      model="gpt-5"
      status="idle"
      onWorkspaceSwitch={vi.fn()}
      onSessionModeChange={vi.fn().mockResolvedValue(undefined)}
      onToggleSessions={vi.fn()}
      onOpenSettings={vi.fn()}
      sessionsOpen={false}
      {...overrides}
    />
  );
}

describe("TopBar", () => {
  afterEach(() => {
    cleanup();
  });

  it("requests the next session mode immediately when the mode button is clicked", () => {
    const onSessionModeChange = vi.fn().mockResolvedValue(undefined);
    renderTopBar({ onSessionModeChange });

    fireEvent.click(screen.getByRole("button", { name: /Session mode: Build/ }));

    expect(onSessionModeChange).toHaveBeenCalledTimes(1);
    expect(onSessionModeChange).toHaveBeenCalledWith("ReadOnly");
    expect(screen.queryByRole("button", { name: /Confirm/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /Cancel session mode change/ })).toBeNull();
  });

  it("labels read-only session mode as Plan and toggles back to Build", () => {
    const onSessionModeChange = vi.fn().mockResolvedValue(undefined);
    renderTopBar({ sessionMode: "ReadOnly", onSessionModeChange });

    fireEvent.click(screen.getByRole("button", { name: /Session mode: Plan/ }));

    expect(onSessionModeChange).toHaveBeenCalledTimes(1);
    expect(onSessionModeChange).toHaveBeenCalledWith("Build");
  });
});
