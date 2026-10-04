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
      model="gpt-5"
      status="idle"
      onWorkspaceSwitch={vi.fn()}
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

  it("keeps the workspace switch button enabled while the agent is active", () => {
    renderTopBar({ status: "thinking" });

    const switchButton = screen.getByRole("button", {
      name: "Switch workspace",
    }) as HTMLButtonElement;
    expect(switchButton.disabled).toBe(false);
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

  it("keeps the session title editor enabled while streaming", () => {
    const onSessionTitleChange = vi.fn();
    renderTopBar({ sessionTitle: "Streaming session", status: "thinking", onSessionTitleChange });

    const editButton = screen.getByRole("button", { name: "Edit session title" });
    expect(editButton.hasAttribute("disabled")).toBe(false);
    expect(editButton.getAttribute("aria-disabled")).toBe("false");

    // Clicking enters edit mode even while streaming (control plane).
    fireEvent.click(editButton);
    expect(screen.queryByRole("textbox", { name: "Session title" })).not.toBeNull();
  });
});
