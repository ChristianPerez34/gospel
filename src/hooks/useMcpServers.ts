import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
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

  const mountedRef = useRef(true);
  const generationRef = useRef(0);

  const canCommit = useCallback(
    (generation: number) => mountedRef.current && generationRef.current === generation,
    []
  );

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      generationRef.current += 1;
    };
  }, []);

  useEffect(() => {
    if (!active) {
      generationRef.current += 1;
    }
  }, [active]);

  const reload = useCallback(async () => {
    const generation = generationRef.current;
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<McpServer[]>("list_mcp_servers");
      if (!canCommit(generation)) return;
      setServers(next);
    } catch (e) {
      if (!canCommit(generation)) return;
      setError(`Failed to load MCP servers: ${e}`);
    } finally {
      if (canCommit(generation)) {
        setLoading(false);
      }
    }
  }, [canCommit]);

  useEffect(() => {
    if (!active) return;
    void reload();
  }, [active, reload]);

  const setEnabled = useCallback(
    async (server: McpServer, enabled: boolean) => {
      const generation = generationRef.current;
      setSavingId(server.id);
      setError(null);
      try {
        const updated = await invoke<McpServer>("set_mcp_server_enabled", {
          kind: server.kind,
          id: server.id,
          enabled,
        });
        if (!canCommit(generation)) return;
        setServers((current) => replaceServer(current, updated));
      } catch (e) {
        if (!canCommit(generation)) return;
        setError(`Failed to update MCP server: ${e}`);
      } finally {
        if (canCommit(generation)) {
          setSavingId(null);
        }
      }
    },
    [canCommit]
  );

  const trust = useCallback(
    async (server: McpServer) => {
      const generation = generationRef.current;
      setSavingId(server.id);
      setError(null);
      try {
        const updated = await invoke<McpServer>("trust_mcp_server", { id: server.id });
        if (!canCommit(generation)) return;
        setServers((current) => replaceServer(current, updated));
      } catch (e) {
        if (!canCommit(generation)) return;
        setError(`Failed to trust MCP server: ${e}`);
      } finally {
        if (canCommit(generation)) {
          setSavingId(null);
        }
      }
    },
    [canCommit]
  );

  const revokeTrust = useCallback(
    async (server: McpServer) => {
      const generation = generationRef.current;
      setSavingId(server.id);
      setError(null);
      try {
        const updated = await invoke<McpServer>("revoke_trust_mcp_server", { id: server.id });
        if (!canCommit(generation)) return;
        setServers((current) => replaceServer(current, updated));
      } catch (e) {
        if (!canCommit(generation)) return;
        setError(`Failed to revoke MCP server trust: ${e}`);
      } finally {
        if (canCommit(generation)) {
          setSavingId(null);
        }
      }
    },
    [canCommit]
  );

  const refresh = useCallback(
    async (server: McpServer) => {
      const generation = generationRef.current;
      setSavingId(server.id);
      setError(null);
      try {
        const updated = await invoke<McpServer>("refresh_mcp_server", {
          kind: server.kind,
          id: server.id,
        });
        if (!canCommit(generation)) return;
        setServers((current) => replaceServer(current, updated));
      } catch (e) {
        if (!canCommit(generation)) return;
        setError(`Failed to refresh MCP server: ${e}`);
      } finally {
        if (canCommit(generation)) {
          setSavingId(null);
        }
      }
    },
    [canCommit]
  );

  const create = useCallback(
    async (request: CreateMcpServerRequest) => {
      const generation = generationRef.current;
      setLoading(true);
      setError(null);
      try {
        await invoke<McpServer>("create_mcp_server", { request });
        if (!canCommit(generation)) return;
        await reload();
      } catch (e) {
        if (canCommit(generation)) {
          setError(`Failed to create MCP server: ${e}`);
        }
        throw e;
      } finally {
        if (canCommit(generation)) {
          setLoading(false);
        }
      }
    },
    [canCommit, reload]
  );

  const update = useCallback(
    async (id: string, request: UpdateMcpServerRequest) => {
      const generation = generationRef.current;
      setSavingId(id);
      setError(null);
      try {
        const updated = await invoke<McpServer>("update_mcp_server", { id, request });
        if (!canCommit(generation)) return;
        setServers((current) => replaceServer(current, updated));
      } catch (e) {
        if (canCommit(generation)) {
          setError(`Failed to update MCP server: ${e}`);
        }
        throw e;
      } finally {
        if (canCommit(generation)) {
          setSavingId(null);
        }
      }
    },
    [canCommit]
  );

  const remove = useCallback(
    async (server: McpServer) => {
      const generation = generationRef.current;
      setSavingId(server.id);
      setError(null);
      try {
        await invoke("delete_mcp_server", { id: server.id });
        if (!canCommit(generation)) return;
        setServers((current) => current.filter((item) => item.id !== server.id));
      } catch (e) {
        if (!canCommit(generation)) return;
        setError(`Failed to delete MCP server: ${e}`);
      } finally {
        if (canCommit(generation)) {
          setSavingId(null);
        }
      }
    },
    [canCommit]
  );

  const previewImport = useCallback(
    async (sourcePath: string) => {
      const generation = generationRef.current;
      setLoading(true);
      setError(null);
      try {
        const preview = await invoke<McpImportPreview>("preview_import_mcp_servers", {
          sourcePath,
        });
        if (!canCommit(generation)) return;
        setImportPreview(preview);
      } catch (e) {
        if (!canCommit(generation)) return;
        setError(`Failed to preview MCP import: ${e}`);
      } finally {
        if (canCommit(generation)) {
          setLoading(false);
        }
      }
    },
    [canCommit]
  );

  const applyImport = useCallback(
    async (
      token: string,
      selectedExternalIds: string[],
      overwriteExisting: boolean
    ): Promise<McpApplyImportResult | null> => {
      const generation = generationRef.current;
      setLoading(true);
      setError(null);
      try {
        const result = await invoke<McpApplyImportResult>("apply_import_mcp_servers", {
          request: { token, selectedExternalIds, overwriteExisting },
        });
        if (!canCommit(generation)) return null;
        setImportPreview(null);
        await reload();
        return result;
      } catch (e) {
        if (canCommit(generation)) {
          setError(`Failed to apply MCP import: ${e}`);
        }
        return null;
      } finally {
        if (canCommit(generation)) {
          setLoading(false);
        }
      }
    },
    [canCommit, reload]
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
