import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "./AppShell";

let capturedListeners: Record<string, ((event: { payload: unknown }) => void)[]> = {};

function triggerEvent<T>(eventName: string, payload: T) {
  const listeners = capturedListeners[eventName] ?? [];
  for (const listener of listeners) {
    listener({ payload });
  }
}

const sampleWorkspace = {
  id: "ws-1",
  name: "Test Workspace",
  path: "/path/to/ws",
  sessionCount: 1,
};

const sampleAvailability = {
  providers: [
    {
      provider: "openai",
      display_name: "OpenAI",
      auth_type: "api_key",
      credentialed: true,
      visible: true,
      model_fetch_status: "success",
      model_count: 1,
    },
  ],
  available_models: [
    {
      provider: "openai",
      model: "gpt-4o",
    },
  ],
  warnings: [],
};

describe("AppShell session title editing", () => {
  beforeEach(() => {
    capturedListeners = {};
    vi.mocked(listen).mockImplementation(async (eventName, callback) => {
      if (!capturedListeners[eventName]) capturedListeners[eventName] = [];
      capturedListeners[eventName].push(callback as (event: { payload: unknown }) => void);
      return () => {};
    });

    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_workspaces") {
        return [sampleWorkspace];
      }
      if (cmd === "get_active_workspace") {
        return sampleWorkspace;
      }
      if (cmd === "get_model_availability") {
        return sampleAvailability;
      }
      if (cmd === "get_archive_policy") {
        return { workspaceId: null, retentionDays: 30, autoArchiveHours: 24 };
      }
      if (cmd === "get_archive_stats") {
        return { archived_count: 0, expired_count: 0 };
      }
      if (cmd === "list_sessions") return [];
      if (cmd === "list_archived_sessions") return [];
      if (cmd === "list_skills") return [];
      return undefined;
    });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("updates local session title when creation failed (backendCreated: false) without calling update_session_title invoke", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_workspaces") {
        return [sampleWorkspace];
      }
      if (cmd === "get_active_workspace") {
        return sampleWorkspace;
      }
      if (cmd === "get_model_availability") {
        return sampleAvailability;
      }
      if (cmd === "get_archive_policy") {
        return { workspaceId: null, retentionDays: 30, autoArchiveHours: 24 };
      }
      if (cmd === "get_archive_stats") {
        return { archived_count: 0, expired_count: 0 };
      }
      if (cmd === "list_sessions") return [];
      if (cmd === "list_archived_sessions") return [];
      if (cmd === "list_skills") return [];
      if (cmd === "create_session") {
        throw new Error("backend database error");
      }
      return undefined;
    });

    render(<AppShell />);

    // Wait for input textarea to be enabled
    const textarea = await screen.findByRole("textbox", { name: "Message input" });
    await waitFor(() => {
      expect(textarea.hasAttribute("disabled")).toBe(false);
    });

    // Send a message to create a local fallback session
    await act(async () => {
      fireEvent.change(textarea, { target: { value: "Initial title prompt" } });
      const sendBtn = screen.getByRole("button", { name: "Send message" });
      fireEvent.click(sendBtn);
    });

    // Complete the stream so session.isStreaming becomes false
    await act(async () => {
      triggerEvent("llm-done", { response: "Hello back" });
    });

    // Initial session title should be rendered in TopBar
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Edit session title" })).toBeDefined();
    });
    expect(screen.getByRole("button", { name: "Edit session title" }).textContent).toBe(
      "Initial title prompt"
    );

    // Now edit session title in TopBar
    fireEvent.click(screen.getByRole("button", { name: "Edit session title" }));
    const input = (await screen.findByRole("textbox", {
      name: "Session title",
    })) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Renamed Fallback Session" } });
    fireEvent.keyDown(input, { key: "Enter" });

    // The title in TopBar should be updated to "Renamed Fallback Session"
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Edit session title" }).textContent).toBe(
        "Renamed Fallback Session"
      );
    });

    // update_session_title invoke should NOT have been called because backendCreated is false
    const updateTitleCalls = vi
      .mocked(invoke)
      .mock.calls.filter(([cmd]) => cmd === "update_session_title");
    expect(updateTitleCalls).toHaveLength(0);
  });

  it("updates local session title and calls update_session_title invoke when backendCreated is true", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_workspaces") {
        return [sampleWorkspace];
      }
      if (cmd === "get_active_workspace") {
        return sampleWorkspace;
      }
      if (cmd === "get_model_availability") {
        return sampleAvailability;
      }
      if (cmd === "get_archive_policy") {
        return { workspaceId: null, retentionDays: 30, autoArchiveHours: 24 };
      }
      if (cmd === "get_archive_stats") {
        return { archived_count: 0, expired_count: 0 };
      }
      if (cmd === "list_sessions") return [];
      if (cmd === "list_archived_sessions") return [];
      if (cmd === "list_skills") return [];
      if (cmd === "create_session") {
        return { id: "sess-backend-1" };
      }
      if (cmd === "update_session_title") {
        return undefined;
      }
      return undefined;
    });

    render(<AppShell />);

    const textarea = await screen.findByRole("textbox", { name: "Message input" });
    await waitFor(() => {
      expect(textarea.hasAttribute("disabled")).toBe(false);
    });

    // Send message to create a backend session
    await act(async () => {
      fireEvent.change(textarea, { target: { value: "Backend session prompt" } });
      const sendBtn = screen.getByRole("button", { name: "Send message" });
      fireEvent.click(sendBtn);
    });

    // Complete the stream so session.isStreaming becomes false
    await act(async () => {
      triggerEvent("llm-done", { response: "Hello back" });
    });

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Edit session title" })).toBeDefined();
    });

    // Edit title
    fireEvent.click(screen.getByRole("button", { name: "Edit session title" }));
    const input = (await screen.findByRole("textbox", {
      name: "Session title",
    })) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Persisted Session Title" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Edit session title" }).textContent).toBe(
        "Persisted Session Title"
      );
    });

    // update_session_title invoke SHOULD have been called
    expect(invoke).toHaveBeenCalledWith("update_session_title", {
      sessionId: "sess-backend-1",
      title: "Persisted Session Title",
    });
  });

  it("rolls back the current rename and shows an error when update_session_title fails", async () => {
    let rejectRename: ((error: Error) => void) | null = null;

    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_workspaces") return [sampleWorkspace];
      if (cmd === "get_active_workspace") return sampleWorkspace;
      if (cmd === "get_model_availability") return sampleAvailability;
      if (cmd === "get_archive_policy") {
        return { workspaceId: null, retentionDays: 30, autoArchiveHours: 24 };
      }
      if (cmd === "get_archive_stats") return { archived_count: 0, expired_count: 0 };
      if (cmd === "list_sessions") return [];
      if (cmd === "list_archived_sessions") return [];
      if (cmd === "list_skills") return [];
      if (cmd === "create_session") return { id: "sess-backend-1" };
      if (cmd === "update_session_title") {
        return new Promise<void>((_resolve, reject) => {
          rejectRename = reject;
        });
      }
      return undefined;
    });

    render(<AppShell />);

    const textarea = await screen.findByRole("textbox", { name: "Message input" });
    await waitFor(() => {
      expect(textarea.hasAttribute("disabled")).toBe(false);
    });

    await act(async () => {
      fireEvent.change(textarea, { target: { value: "Backend session prompt" } });
      fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    });

    await act(async () => {
      triggerEvent("llm-done", { response: "Hello back" });
    });

    const editButton = () => screen.getByRole("button", { name: "Edit session title" });
    await waitFor(() => {
      expect(editButton().hasAttribute("disabled")).toBe(false);
    });

    fireEvent.click(editButton());
    const input = (await screen.findByRole("textbox", {
      name: "Session title",
    })) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Rejected Rename" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => {
      expect(editButton().textContent).toBe("Rejected Rename");
    });

    await act(async () => {
      rejectRename?.(new Error("database write failed"));
    });

    await waitFor(() => {
      expect(editButton().textContent).toBe("Backend session prompt");
    });
    expect(
      await screen.findByText("Failed to rename session: Error: database write failed")
    ).toBeDefined();
  });

  it("serializes newer renames after an older update_session_title failure", async () => {
    // Controllable pending promises for each rename request, resolved in order.
    let firstReject: ((error: Error) => void) | null = null;
    let secondResolve: (() => void) | null = null;
    const titleInvokes: string[] = [];

    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "list_workspaces") return [sampleWorkspace];
      if (cmd === "get_active_workspace") return sampleWorkspace;
      if (cmd === "get_model_availability") return sampleAvailability;
      if (cmd === "get_archive_policy") {
        return { workspaceId: null, retentionDays: 30, autoArchiveHours: 24 };
      }
      if (cmd === "get_archive_stats") return { archived_count: 0, expired_count: 0 };
      if (cmd === "list_sessions") return [];
      if (cmd === "list_archived_sessions") return [];
      if (cmd === "list_skills") return [];
      if (cmd === "create_session") return { id: "sess-backend-1" };
      if (cmd === "update_session_title") {
        const title = (args as { title: string }).title;
        titleInvokes.push(title);
        if (title === "First Rename") {
          return new Promise<void>((_resolve, reject) => {
            firstReject = reject;
          });
        }
        if (title === "Second Rename") {
          return new Promise<void>((resolve) => {
            secondResolve = resolve;
          });
        }
        return undefined;
      }
      return undefined;
    });

    render(<AppShell />);

    const textarea = await screen.findByRole("textbox", { name: "Message input" });
    await waitFor(() => {
      expect(textarea.hasAttribute("disabled")).toBe(false);
    });

    await act(async () => {
      fireEvent.change(textarea, { target: { value: "Backend session prompt" } });
      fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    });

    // Complete the stream so session.isStreaming becomes false.
    await act(async () => {
      triggerEvent("llm-done", { response: "Hello back" });
    });

    const editButton = () => screen.getByRole("button", { name: "Edit session title" });
    await waitFor(() => {
      expect(editButton().hasAttribute("disabled")).toBe(false);
    });

    // First rename — request stays in flight.
    fireEvent.click(editButton());
    let input = (await screen.findByRole("textbox", {
      name: "Session title",
    })) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "First Rename" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(titleInvokes).toContain("First Rename"));

    // Second rename supersedes the first before the first settles.
    await waitFor(() => {
      expect(editButton().hasAttribute("disabled")).toBe(false);
    });
    fireEvent.click(editButton());
    input = (await screen.findByRole("textbox", {
      name: "Session title",
    })) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Second Rename" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(titleInvokes).toEqual(["First Rename"]);

    // The first request fails. The newer optimistic title must be preserved.
    await act(async () => {
      firstReject?.(new Error("network down"));
    });
    await waitFor(() => expect(titleInvokes).toEqual(["First Rename", "Second Rename"]));
    const resolveSecond = secondResolve as (() => void) | null;
    if (resolveSecond) {
      await act(async () => {
        resolveSecond();
      });
    }

    await waitFor(() => {
      expect(editButton().textContent).toBe("Second Rename");
    });
  });
});
