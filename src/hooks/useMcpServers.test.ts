import { invoke } from "@tauri-apps/api/core";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { McpServer } from "../types";
import { useMcpServers } from "./useMcpServers";

function sampleServer(overrides: Partial<McpServer> = {}): McpServer {
  return {
    id: "mcp-1",
    kind: "custom",
    displayName: "Docs",
    enabled: false,
    trusted: false,
    safetyClass: "read_only",
    scope: "main_and_exploration",
    args: [],
    env: [],
    secretEnvKeys: [],
    readiness: "awaiting_first_connection",
    health: "not_connected",
    inventory: [],
    createdAt: "2026-08-16T00:00:00Z",
    updatedAt: "2026-08-16T00:00:00Z",
    ...overrides,
  };
}

describe("useMcpServers", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it("does not fetch servers when active is false", () => {
    renderHook(() => useMcpServers(false));

    expect(invoke).not.toHaveBeenCalledWith("list_mcp_servers");
  });

  it("fetches servers and sets state when active is true", async () => {
    const server = sampleServer();
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_mcp_servers") return [server];
      return undefined;
    });

    const { result } = renderHook(() => useMcpServers(true));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    expect(result.current.servers).toEqual([server]);
    expect(result.current.error).toBeNull();
  });

  it("handles IPC rejection gracefully", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_mcp_servers") {
        throw new Error("backend unavailable");
      }
      return undefined;
    });

    const { result } = renderHook(() => useMcpServers(true));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    expect(result.current.servers).toEqual([]);
    expect(result.current.error).toBe("Failed to load MCP servers: Error: backend unavailable");
  });

  it("does not throw if unmounted before list_mcp_servers resolves", async () => {
    let resolveList: ((servers: McpServer[]) => void) | null = null;
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "list_mcp_servers") {
        return new Promise<McpServer[]>((resolve) => {
          resolveList = resolve;
        });
      }
      return Promise.resolve(undefined);
    });

    const { unmount } = renderHook(() => useMcpServers(true));
    unmount();

    await act(async () => {
      resolveList?.([sampleServer()]);
    });
  });

  it("ignores a late list_mcp_servers response after active becomes false", async () => {
    let resolveList: ((servers: McpServer[]) => void) | null = null;
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "list_mcp_servers") {
        return new Promise<McpServer[]>((resolve) => {
          resolveList = resolve;
        });
      }
      return Promise.resolve(undefined);
    });

    const { result, rerender } = renderHook(({ active }) => useMcpServers(active), {
      initialProps: { active: true },
    });
    await waitFor(() => {
      expect(result.current.loading).toBe(true);
    });

    rerender({ active: false });
    await act(async () => {
      resolveList?.([sampleServer()]);
    });

    expect(result.current.servers).toEqual([]);
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("updates server enabled state on setEnabled call", async () => {
    const server = sampleServer({ enabled: false });
    const enabled = sampleServer({ enabled: true });
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_mcp_servers") return [server];
      if (cmd === "set_mcp_server_enabled") return enabled;
      return undefined;
    });

    const { result } = renderHook(() => useMcpServers(true));
    await waitFor(() => {
      expect(result.current.servers).toEqual([server]);
    });

    await act(async () => {
      await result.current.setEnabled(server, true);
    });

    expect(result.current.servers).toEqual([enabled]);
    expect(invoke).toHaveBeenCalledWith("set_mcp_server_enabled", {
      kind: server.kind,
      id: server.id,
      enabled: true,
    });
  });
});
