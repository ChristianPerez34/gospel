import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import type { Workspace } from "../types";

interface UseWorkspacesReturn {
  workspaces: Workspace[];
  activeWorkspace: Workspace | null;
  loading: boolean;
  addWorkspace: (path: string) => Promise<Workspace | null>;
  removeWorkspace: (id: string) => Promise<boolean>;
  switchWorkspace: (id: string) => Promise<boolean>;
  refresh: () => Promise<void>;
}

export function useWorkspaces(): UseWorkspacesReturn {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [activeWorkspace, setActiveWorkspace] = useState<Workspace | null>(null);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    try {
      const [ws, active] = await Promise.all([
        invoke<Workspace[]>("list_workspaces"),
        invoke<Workspace | null>("get_active_workspace"),
      ]);
      setWorkspaces(ws);
      setActiveWorkspace(active);
    } catch (e) {
      console.error("Failed to load workspaces:", e);
    }
  }, []);

  useEffect(() => {
    setLoading(true);
    void refresh().finally(() => setLoading(false));
  }, [refresh]);

  const addWorkspace = useCallback(
    async (path: string): Promise<Workspace | null> => {
      try {
        const ws = await invoke<Workspace>("add_workspace", { path });
        await invoke("set_active_workspace", { id: ws.id });
        await refresh();
        return ws;
      } catch (e) {
        console.error("Failed to add workspace:", e);
        return null;
      }
    },
    [refresh]
  );

  const removeWorkspace = useCallback(
    async (id: string): Promise<boolean> => {
      try {
        await invoke("remove_workspace", { id });
        await refresh();
        return true;
      } catch (e) {
        console.error("Failed to remove workspace:", e);
        return false;
      }
    },
    [refresh]
  );

  const switchWorkspace = useCallback(
    async (id: string): Promise<boolean> => {
      try {
        await invoke("set_active_workspace", { id });
        await refresh();
        return true;
      } catch (e) {
        console.error("Failed to switch workspace:", e);
        return false;
      }
    },
    [refresh]
  );

  return {
    workspaces,
    activeWorkspace,
    loading,
    addWorkspace,
    removeWorkspace,
    switchWorkspace,
    refresh,
  };
}
