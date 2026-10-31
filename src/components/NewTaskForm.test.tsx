import { invoke } from "@tauri-apps/api/core";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  onAddWorkspace: vi.fn().mockResolvedValue(null),
  onOpenSettings: vi.fn(),
  creating: false,
  error: null,
};
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("NewTaskForm", () => {
  it("asks only for the workspace before opening a chat", async () => {
    render(<NewTaskForm {...props} />);
    expect(screen.getByText("/projects/alpha")).toBeDefined();
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(document.activeElement).toBe(screen.getByLabelText("Workspace"));
    fireEvent.click(screen.getByRole("combobox", { name: "Workspace" }));
    fireEvent.click(await screen.findByRole("option", { name: /Beta/ }));
    expect(screen.getByText("/projects/beta")).toBeDefined();
    expect(props.onContinue).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Continue to chat" }));
    expect(props.onContinue).toHaveBeenCalledWith("beta");
  });

  it("retains the selected workspace after failure and offers missing setup actions", async () => {
    const { rerender } = render(<NewTaskForm {...props} />);
    fireEvent.click(screen.getByRole("combobox", { name: "Workspace" }));
    fireEvent.click(await screen.findByRole("option", { name: /Beta/ }));
    rerender(<NewTaskForm {...props} error="Storage unavailable" />);
    expect(screen.getByRole("alert").textContent).toBe("Storage unavailable");
    expect(screen.getByRole("combobox", { name: "Workspace" }).textContent).toContain("Beta");
    rerender(<NewTaskForm {...props} workspaces={[]} hasModel={false} />);
    expect(
      (screen.getByRole("button", { name: "Continue to chat" }) as HTMLButtonElement).disabled
    ).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Add workspace" }));
    fireEvent.click(screen.getByRole("button", { name: "Model settings" }));
    expect(props.onAddWorkspace).toHaveBeenCalledOnce();
    expect(props.onOpenSettings).toHaveBeenCalledOnce();
  });
  it("selects a newly added folder and restores focus when the chooser returns", async () => {
    const added = { id: "new", name: "New project", path: "/projects/new", sessionCount: 0 };
    let finish: ((workspace: typeof added | null) => void) | undefined;
    const onAddWorkspace = vi.fn(
      () =>
        new Promise<typeof added | null>((resolve) => {
          finish = resolve;
        })
    );
    const { rerender } = render(
      <NewTaskForm {...props} workspaces={[]} onAddWorkspace={onAddWorkspace} />
    );
    fireEvent.click(screen.getByRole("button", { name: "Add workspace" }));
    expect(screen.getByRole("button", { name: "Continue to chat" }).hasAttribute("disabled")).toBe(
      true
    );
    rerender(<NewTaskForm {...props} workspaces={[added]} onAddWorkspace={onAddWorkspace} />);
    await act(async () => {
      finish?.(added);
    });
    expect(screen.getByRole("combobox", { name: "Workspace" }).textContent).toContain(added.path);
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("combobox", { name: "Workspace" }))
    );
    fireEvent.click(screen.getByRole("button", { name: "Continue to chat" }));
    expect(props.onContinue).toHaveBeenCalledWith(added.id);
  });

  it("shows the selected workspace's git branch and worktree status", async () => {
    vi.mocked(invoke).mockImplementation((command, args) =>
      Promise.resolve(
        command === "get_workspace_git_context"
          ? {
              branch: `feat/${(args as { workspaceId: string }).workspaceId}`,
              revision: null,
              root: "/projects",
              isWorktree: true,
            }
          : undefined
      )
    );
    render(<NewTaskForm {...props} />);

    const badge = await screen.findByTestId("topbar-git-context");
    expect(badge.textContent).toContain("feat/alpha");
    expect(badge.textContent).toContain("worktree");

    fireEvent.click(screen.getByRole("combobox", { name: "Workspace" }));
    fireEvent.click(await screen.findByRole("option", { name: /Beta/ }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("get_workspace_git_context", { workspaceId: "beta" })
    );
    await waitFor(() =>
      expect(screen.getByTestId("topbar-git-context").textContent).toContain("feat/beta")
    );
  });

  it("syncs activeWorkspaceId when it resolves after initial render", () => {
    const { rerender } = render(<NewTaskForm {...props} activeWorkspaceId={undefined} />);
    expect(screen.getByRole("combobox", { name: "Workspace" }).textContent).toContain(
      "Choose a workspace"
    );
    rerender(<NewTaskForm {...props} activeWorkspaceId="beta" />);
    expect(screen.getByRole("combobox", { name: "Workspace" }).textContent).toContain("Beta");
  });
});
