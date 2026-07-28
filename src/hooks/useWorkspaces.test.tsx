import { invoke } from "@tauri-apps/api/core";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Workspace } from "../types";
import { useWorkspaces, WorkspacesProvider } from "./useWorkspaces";

const FIRST_WORKSPACE: Workspace = {
  id: "workspace-1",
  name: "First",
  path: "/workspaces/first",
  sessionCount: 0,
};

const SECOND_WORKSPACE: Workspace = {
  id: "workspace-2",
  name: "Second",
  path: "/workspaces/second",
  sessionCount: 0,
};

function wrapper({ children }: { children: ReactNode }) {
  return <WorkspacesProvider>{children}</WorkspacesProvider>;
}

describe("WorkspacesProvider", () => {
  let activeWorkspace: Workspace;

  beforeEach(() => {
    activeWorkspace = FIRST_WORKSPACE;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      switch (command) {
        case "list_workspaces":
          return [FIRST_WORKSPACE, SECOND_WORKSPACE];
        case "get_active_workspace":
          return activeWorkspace;
        case "set_active_workspace":
          activeWorkspace =
            (args as { id?: string } | undefined)?.id === SECOND_WORKSPACE.id
              ? SECOND_WORKSPACE
              : FIRST_WORKSPACE;
          return undefined;
        default:
          return undefined;
      }
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("shares workspace switches between independent consumers", async () => {
    const { result } = renderHook(
      () => ({
        appShell: useWorkspaces(),
        planOverlay: useWorkspaces(),
      }),
      { wrapper }
    );

    await waitFor(() => {
      expect(result.current.appShell.loading).toBe(false);
      expect(result.current.planOverlay.activeWorkspace).toEqual(FIRST_WORKSPACE);
    });

    await act(async () => {
      await result.current.appShell.switchWorkspace(SECOND_WORKSPACE.id);
    });

    expect(result.current.planOverlay.activeWorkspace).toEqual(SECOND_WORKSPACE);
  });
});
