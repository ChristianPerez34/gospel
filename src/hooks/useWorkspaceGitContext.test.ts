import { invoke } from "@tauri-apps/api/core";
import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceGitContext } from "../types";
import { useWorkspaceGitContext } from "./useWorkspaceGitContext";

describe("useWorkspaceGitContext", () => {
  afterEach(cleanup);

  it("loads Git context for the requested workspace", async () => {
    const context: WorkspaceGitContext = {
      branch: "main",
      revision: null,
      root: "/workspaces/project",
      isWorktree: false,
    };
    vi.mocked(invoke).mockResolvedValue(context);

    const { result } = renderHook(() => useWorkspaceGitContext("workspace-1"));

    await waitFor(() => expect(result.current).toEqual(context));
    expect(invoke).toHaveBeenCalledWith("get_workspace_git_context", {
      workspaceId: "workspace-1",
    });
  });
});
