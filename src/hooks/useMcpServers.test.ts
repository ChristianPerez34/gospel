import { invoke } from "@tauri-apps/api/core";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { McpServer } from "../types";
import { useMcpServers } from "./useMcpServers";

function makeServer(overrides: Partial<McpServer> = {}): McpServer {
  return {
    id: "server-1",
    kind: "custom",
    displayName: "Test Server",
    description: null,
    enabled: true,
    trusted: false,
    trustRevokedReason: null,
    safetyClass: "read_only",
    scope: "workspace",
    command: "npx",
    args: [],
    env: [],
    secretEnvKeys: [],
    readiness: "ready",
    health: "ok",
    inventory: [],
    lastErrorSummary: null,
    lastSuccessAt: null,
    lastResolvedExecutablePath: null,
    externalFingerprint: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

describe("useMcpServers", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("does not fetch servers when active is false", () => {
    renderHook(() => useMcpServers(false));
    expect(invoke).not.toHaveBeenCalled();
  });

  it("fetches servers and sets state when active is true", async () => {
    const servers = [makeServer()];
    vi.mocked(invoke).mockResolvedValueOnce(servers);

    const { result } = renderHook(() => useMcpServers(true));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(invoke).toHaveBeenCalledWith("list_mcp_servers");
    expect(result.current.servers).toEqual(servers);
    expect(result.current.error).toBeNull();
  });

  it("handles IPC rejection gracefully", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("backend down"));

    const { result } = renderHook(() => useMcpServers(true));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.servers).toEqual([]);
    expect(result.current.error).toBe("Failed to load MCP servers: Error: backend down");
  });

  it("ignores reload results after deactivation", async () => {
    let resolveList: (value: McpServer[]) => void = () => {};
    vi.mocked(invoke).mockImplementation(
      () =>
        new Promise<McpServer[]>((resolve) => {
          resolveList = resolve;
        })
    );

    const { result, rerender } = renderHook(({ active }) => useMcpServers(active), {
      initialProps: { active: true },
    });

    await act(async () => {});
    expect(result.current.loading).toBe(true);

    rerender({ active: false });

    await act(async () => {
      resolveList([makeServer({ id: "stale" })]);
    });

    expect(result.current.servers).toEqual([]);
    expect(result.current.error).toBeNull();
  });

  it("ignores mutation results after unmount", async () => {
    const initial = makeServer({ enabled: true });
    const updated = makeServer({ enabled: false });

    let resolveEnabled: (value: McpServer) => void = () => {};
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_mcp_servers") return [initial];
      if (cmd === "set_mcp_server_enabled") {
        return new Promise<McpServer>((resolve) => {
          resolveEnabled = resolve;
        });
      }
      return undefined as unknown;
    });

    const { result, unmount } = renderHook(() => useMcpServers(true));

    await waitFor(() => {
      expect(result.current.servers).toEqual([initial]);
    });

    let setEnabledPromise: Promise<void> | undefined;
    await act(async () => {
      setEnabledPromise = result.current.setEnabled(initial, false);
    });

    expect(result.current.savingId).toBe(initial.id);
    unmount();

    await act(async () => {
      resolveEnabled(updated);
      await setEnabledPromise;
    });

    // Last snapshot before unmount must not have been updated by the late mutation.
    expect(result.current.servers).toEqual([initial]);
    expect(result.current.savingId).toBe(initial.id);
  });

  it("updates server enabled state on setEnabled call", async () => {
    const initial = makeServer({ enabled: true });
    const updated = makeServer({ enabled: false });

    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_mcp_servers") return [initial];
      if (cmd === "set_mcp_server_enabled") return updated;
      return undefined as unknown;
    });

    const { result } = renderHook(() => useMcpServers(true));

    await waitFor(() => {
      expect(result.current.servers).toEqual([initial]);
    });

    await act(async () => {
      await result.current.setEnabled(initial, false);
    });

    expect(result.current.servers).toEqual([updated]);
    expect(result.current.savingId).toBeNull();
    expect(result.current.error).toBeNull();
  });
});
