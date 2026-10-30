import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type { WorkspaceGitContext } from "../types";

export function useWorkspaceGitContext(workspaceId?: string) {
  const [current, setCurrent] = useState<{
    workspaceId: string;
    context: WorkspaceGitContext | null;
  } | null>(null);

  useEffect(() => {
    if (!workspaceId) {
      setCurrent(null);
      return;
    }

    let active = true;
    let loading = false;
    const refresh = async () => {
      if (loading) return;
      loading = true;
      try {
        const context = await invoke<WorkspaceGitContext | null>("get_workspace_git_context", {
          workspaceId,
        });
        if (active) setCurrent({ workspaceId, context });
      } catch {
        if (active) setCurrent({ workspaceId, context: null });
      } finally {
        loading = false;
      }
    };

    void refresh();
    // ponytail: 15s polling can lag external branch changes; use a watcher if that latency matters.
    const interval = window.setInterval(refresh, 15_000);
    window.addEventListener("focus", refresh);
    return () => {
      active = false;
      window.clearInterval(interval);
      window.removeEventListener("focus", refresh);
    };
  }, [workspaceId]);

  return current && current.workspaceId === workspaceId ? current.context : null;
}
