import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import type {
  CreateMcpServerRequest,
  McpApplyImportResult,
  McpImportPreview,
  McpServer,
  UpdateMcpServerRequest,
} from "../types";

function replaceServer(servers: McpServer[], updated: McpServer) {
  return servers.map((server) => (server.id === updated.id ? updated : server));
}

export function useMcpServers(active: boolean) {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [loading, setLoading] = useState(false);
  const [savingId, setSavingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [importPreview, setImportPreview] = useState<McpImportPreview | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<McpServer[]>("list_mcp_servers");
      setServers(next);
    } catch (e) {
      setError(`Failed to load MCP servers: ${e}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!active) return;
    void reload();
  }, [active, reload]);

  const setEnabled = useCallback(async (server: McpServer, enabled: boolean) => {
    setSavingId(server.id);
    setError(null);
    try {
      const updated = await invoke<McpServer>("set_mcp_server_enabled", {
        kind: server.kind,
        id: server.id,
        enabled,
      });
      setServers((current) => replaceServer(current, updated));
    } catch (e) {
      setError(`Failed to update MCP server: ${e}`);
    } finally {
      setSavingId(null);
    }
  }, []);

  const trust = useCallback(async (server: McpServer) => {
    setSavingId(server.id);
    setError(null);
    try {
      const updated = await invoke<McpServer>("trust_mcp_server", { id: server.id });
      setServers((current) => replaceServer(current, updated));
    } catch (e) {
      setError(`Failed to trust MCP server: ${e}`);
    } finally {
      setSavingId(null);
    }
  }, []);

  const revokeTrust = useCallback(async (server: McpServer) => {
    setSavingId(server.id);
    setError(null);
    try {
      const updated = await invoke<McpServer>("revoke_trust_mcp_server", { id: server.id });
      setServers((current) => replaceServer(current, updated));
    } catch (e) {
      setError(`Failed to revoke MCP server trust: ${e}`);
    } finally {
      setSavingId(null);
    }
  }, []);

  const refresh = useCallback(async (server: McpServer) => {
    setSavingId(server.id);
    setError(null);
    try {
      const updated = await invoke<McpServer>("refresh_mcp_server", {
        kind: server.kind,
        id: server.id,
      });
      setServers((current) => replaceServer(current, updated));
    } catch (e) {
      setError(`Failed to refresh MCP server: ${e}`);
    } finally {
      setSavingId(null);
    }
  }, []);

  const create = useCallback(
    async (request: CreateMcpServerRequest) => {
      setLoading(true);
      setError(null);
      try {
        await invoke<McpServer>("create_mcp_server", { request });
        await reload();
      } catch (e) {
        setError(`Failed to create MCP server: ${e}`);
        throw e;
      } finally {
        setLoading(false);
      }
    },
    [reload]
  );

  const update = useCallback(async (id: string, request: UpdateMcpServerRequest) => {
    setSavingId(id);
    setError(null);
    try {
      const updated = await invoke<McpServer>("update_mcp_server", { id, request });
      setServers((current) => replaceServer(current, updated));
    } catch (e) {
      setError(`Failed to update MCP server: ${e}`);
      throw e;
    } finally {
      setSavingId(null);
    }
  }, []);

  const remove = useCallback(async (server: McpServer) => {
    setSavingId(server.id);
    setError(null);
    try {
      await invoke("delete_mcp_server", { id: server.id });
      setServers((current) => current.filter((item) => item.id !== server.id));
    } catch (e) {
      setError(`Failed to delete MCP server: ${e}`);
    } finally {
      setSavingId(null);
    }
  }, []);

  const previewImport = useCallback(async (sourcePath: string) => {
    setLoading(true);
    setError(null);
    try {
      const preview = await invoke<McpImportPreview>("preview_import_mcp_servers", {
        sourcePath,
      });
      setImportPreview(preview);
    } catch (e) {
      setError(`Failed to preview MCP import: ${e}`);
    } finally {
      setLoading(false);
    }
  }, []);

  const applyImport = useCallback(
    async (
      token: string,
      selectedExternalIds: string[],
      overwriteExisting: boolean
    ): Promise<McpApplyImportResult | null> => {
      setLoading(true);
      setError(null);
      try {
        const result = await invoke<McpApplyImportResult>("apply_import_mcp_servers", {
          request: { token, selectedExternalIds, overwriteExisting },
        });
        setImportPreview(null);
        await reload();
        return result;
      } catch (e) {
        setError(`Failed to apply MCP import: ${e}`);
        return null;
      } finally {
        setLoading(false);
      }
    },
    [reload]
  );

  const builtInServers = servers.filter((server) => server.kind === "built_in");
  const customServers = servers.filter((server) => server.kind === "custom");

  return {
    servers,
    builtInServers,
    customServers,
    loading,
    savingId,
    error,
    importPreview,
    setImportPreview,
    reload,
    setEnabled,
    trust,
    revokeTrust,
    refresh,
    create,
    update,
    remove,
    previewImport,
    applyImport,
  };
}
