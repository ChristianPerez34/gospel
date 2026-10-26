import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { NewTaskForm } from "./NewTaskForm";

const props = {
  workspaces: [
    { id: "alpha", name: "Alpha", path: "/projects/alpha", sessionCount: 0 },
    { id: "beta", name: "Beta", path: "/projects/beta", sessionCount: 0 },
  ],
  activeWorkspaceId: "alpha",
  hasModel: true,
  onContinue: vi.fn(),
  onCancel: vi.fn(),
  onManageWorkspaces: vi.fn(),
  onOpenSettings: vi.fn(),
  creating: false,
  error: null,
};
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("NewTaskForm", () => {
  it("asks only for the workspace before opening a chat", () => {
    render(<NewTaskForm {...props} />);
    expect(screen.getByText("/projects/alpha")).toBeDefined();
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(document.activeElement).toBe(screen.getByLabelText("Workspace"));
    fireEvent.change(screen.getByLabelText("Workspace"), { target: { value: "beta" } });
    expect(screen.getByText("/projects/beta")).toBeDefined();
    expect(props.onContinue).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Continue to chat" }));
    expect(props.onContinue).toHaveBeenCalledWith("beta");
  });

  it("retains the selected workspace after failure and offers missing setup actions", () => {
    const { rerender } = render(<NewTaskForm {...props} />);
    fireEvent.change(screen.getByLabelText("Workspace"), { target: { value: "beta" } });
    rerender(<NewTaskForm {...props} error="Storage unavailable" />);
    expect(screen.getByRole("alert").textContent).toBe("Storage unavailable");
    expect((screen.getByLabelText("Workspace") as HTMLSelectElement).value).toBe("beta");
    rerender(<NewTaskForm {...props} workspaces={[]} hasModel={false} />);
    expect(
      (screen.getByRole("button", { name: "Continue to chat" }) as HTMLButtonElement).disabled
    ).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Add a workspace" }));
    fireEvent.click(screen.getByRole("button", { name: "Model settings" }));
    expect(props.onManageWorkspaces).toHaveBeenCalledOnce();
    expect(props.onOpenSettings).toHaveBeenCalledOnce();
  });
});
