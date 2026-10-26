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

  it("shows global scope and activity on the homepage without session controls", () => {
    renderTopBar({
      overviewOpen: true,
      showWorkspaceSwitch: false,
      runningTaskCount: 2,
      status: "acting",
    });
    expect(screen.getByText("All workspaces")).toBeDefined();
    expect(screen.getByText("Overview")).toBeDefined();
    expect(screen.getByText("2 running")).toBeDefined();
    expect(screen.queryByText("gpt-5")).toBeNull();
    expect(screen.queryByText("Acting")).toBeNull();
    expect(screen.queryByRole("button", { name: "Edit session title" })).toBeNull();
  });

  it("keeps conversation status specific to the selected task", () => {
    renderTopBar({ runningTaskCount: 2, status: "connected" });
    expect(screen.getByText("gpt-5")).toBeDefined();
    expect(screen.getByText("Connected")).toBeDefined();
    expect(screen.queryByText("2 running")).toBeNull();
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

  it("counts sessions that need you on the tasks button", () => {
    renderTopBar({ needsYouCount: 2 });
    expect(
      screen.getByRole("button", { name: "Toggle tasks, 2 sessions need you" }).textContent
    ).toBe("2");

    cleanup();
    renderTopBar({ needsYouCount: 1 });
    expect(screen.getByRole("button", { name: "Toggle tasks, 1 session needs you" })).toBeDefined();

    cleanup();
    renderTopBar({ needsYouCount: 12 });
    expect(
      screen.getByRole("button", { name: "Toggle tasks, 12 sessions need you" }).textContent
    ).toBe("9+");

    cleanup();
    renderTopBar();
    expect(screen.getByRole("button", { name: "Toggle tasks" }).textContent).toBe("");
  });
});
