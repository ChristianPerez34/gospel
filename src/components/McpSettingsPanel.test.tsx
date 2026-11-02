import { invoke } from "@tauri-apps/api/core";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { McpServer } from "../types";
import { McpSettingsPanel } from "./McpSettingsPanel";

const mockInvoke = vi.mocked(invoke);

function sampleServer(overrides: Partial<McpServer> = {}): McpServer {
  return {
    id: "mcp-http-1",
    kind: "custom",
    displayName: "api.mobbin.com",
    enabled: true,
    trusted: true,
    safetyClass: "read_only",
    scope: "main_and_exploration",
    transport: "http",
    url: "https://api.mobbin.com/mcp",
    args: [],
    env: [],
    headers: [],
    secretEnvKeys: [],
    secretHeaderKeys: [],
    missingSecretEnvKeys: [],
    missingSecretHeaderKeys: [],
    authStatus: "required",
    readiness: "awaiting_first_connection",
    health: "auth_required",
    inventory: [],
    lastErrorSummary: "Sign-in required.",
    createdAt: "2026-08-16T00:00:00Z",
    updatedAt: "2026-08-16T00:00:00Z",
    ...overrides,
  };
}

describe("McpSettingsPanel", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it("does not duplicate 'sign-in required' when server needs authentication", async () => {
    const server = sampleServer();
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_mcp_servers") return [server];
      return undefined;
    });

    render(<McpSettingsPanel />);

    await waitFor(() => {
      expect(screen.getByText("api.mobbin.com")).toBeTruthy();
    });

    expect(screen.getByText("sign-in required")).toBeTruthy();
    expect(screen.queryByText("Sign-in required.")).toBeNull();
    expect(screen.getByRole("button", { name: /sign in/i })).toBeTruthy();
  });

  it("keeps a distinct error summary for a server that needs authentication", async () => {
    const server = sampleServer({ lastErrorSummary: "Token refresh failed" });
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_mcp_servers") return [server];
      return undefined;
    });

    render(<McpSettingsPanel />);

    await waitFor(() => {
      expect(screen.getByText("Token refresh failed")).toBeTruthy();
    });
    expect(screen.getByText("sign-in required")).toBeTruthy();
  });

  it("displays non-auth error summary when connection fails for other reasons", async () => {
    const server = sampleServer({
      health: "not_connected",
      authStatus: "none",
      lastErrorSummary: "Connection timed out",
    });
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_mcp_servers") return [server];
      return undefined;
    });

    render(<McpSettingsPanel />);

    await waitFor(() => {
      expect(screen.getByText("Connection timed out")).toBeTruthy();
    });
  });
});
