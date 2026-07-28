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
      onSessionTitleChange={vi.fn()}
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

  it("disables the workspace switch button while the agent is active", () => {
    renderTopBar({ status: "thinking" });

    const switchButton = screen.getByRole("button", {
      name: "Switch workspace",
    }) as HTMLButtonElement;
    expect(switchButton.disabled).toBe(true);
    expect(switchButton.className).not.toContain("hover:bg-surface-overlay");
    expect(switchButton.className).toContain("cursor-not-allowed");
  });

  it("calls onSessionTitleChange with the trimmed title on Enter", () => {
    const onSessionTitleChange = vi.fn();
    renderTopBar({ sessionTitle: "Old", onSessionTitleChange });

    fireEvent.click(screen.getByLabelText("Edit session title"));
    const input = screen.getByRole("textbox", { name: "Session title" }) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "New Title  " } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onSessionTitleChange).toHaveBeenCalledTimes(1);
    expect(onSessionTitleChange).toHaveBeenCalledWith("New Title");
  });

  it("does not call onSessionTitleChange when the trimmed title equals the current title", () => {
    const onSessionTitleChange = vi.fn();
    renderTopBar({ sessionTitle: "Same", onSessionTitleChange });

    fireEvent.click(screen.getByLabelText("Edit session title"));
    const input = screen.getByRole("textbox", { name: "Session title" }) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "  Same  " } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onSessionTitleChange).not.toHaveBeenCalled();

    fireEvent.click(screen.getByLabelText("Edit session title"));
    expect((screen.getByRole("textbox", { name: "Session title" }) as HTMLInputElement).value).toBe(
      "Same"
    );
  });

  it("restores the current title when the trimmed title is empty", () => {
    const onSessionTitleChange = vi.fn();
    renderTopBar({ sessionTitle: "Current", onSessionTitleChange });

    fireEvent.click(screen.getByLabelText("Edit session title"));
    const input = screen.getByRole("textbox", { name: "Session title" }) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onSessionTitleChange).not.toHaveBeenCalled();

    fireEvent.click(screen.getByLabelText("Edit session title"));
    expect((screen.getByRole("textbox", { name: "Session title" }) as HTMLInputElement).value).toBe(
      "Current"
    );
  });

  it("disables the session title editor while streaming", () => {
    const onSessionTitleChange = vi.fn();
    renderTopBar({ sessionTitle: "Streaming session", status: "thinking", onSessionTitleChange });

    const editButton = screen.getByRole("button", { name: "Edit session title" });
    expect(editButton.hasAttribute("disabled")).toBe(false);
    expect(editButton.getAttribute("aria-disabled")).toBe("true");
    expect(editButton.getAttribute("title")).toBe("Session title can't be edited while streaming");

    // Clicking a disabled button does not enter edit mode.
    fireEvent.click(editButton);
    expect(screen.queryByRole("textbox", { name: "Session title" })).toBeNull();
    expect(onSessionTitleChange).not.toHaveBeenCalled();
  });
});
