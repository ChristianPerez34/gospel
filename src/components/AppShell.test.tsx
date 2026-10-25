import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WorkspacesProvider } from "../hooks/useWorkspaces";
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

const otherWorkspace = {
  id: "ws-2",
  name: "Other Workspace",
  path: "/path/to/other",
  sessionCount: 0,
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

function handleCommonInvoke(
  cmd: string,
  args: unknown,
  ctx?: {
    workspaces?: Array<typeof sampleWorkspace>;
    getActiveWorkspace?: () => typeof sampleWorkspace;
    setActiveWorkspace?: (id: string) => void;
  }
) {
  const workspaces = ctx?.workspaces ?? [sampleWorkspace];
  if (cmd === "list_workspaces") return workspaces;
  if (cmd === "get_active_workspace") {
    return ctx?.getActiveWorkspace?.() ?? workspaces[0] ?? sampleWorkspace;
  }
  if (cmd === "get_model_availability") return sampleAvailability;
  if (cmd === "get_archive_policy") {
    return { workspaceId: null, retentionDays: 30, autoArchiveHours: 24 };
  }
  if (cmd === "get_archive_stats") return { archived_count: 0, expired_count: 0 };
  if (cmd === "list_sessions") return [];
  if (cmd === "list_archived_sessions") return [];
  if (cmd === "list_skills") return [];
  if (cmd === "set_active_workspace") {
    ctx?.setActiveWorkspace?.((args as { id: string }).id);
    return undefined;
  }
  return undefined;
}

describe("AppShell session title editing", () => {
  beforeEach(() => {
    capturedListeners = {};
    vi.mocked(listen).mockImplementation(async (eventName, callback) => {
      if (!capturedListeners[eventName]) capturedListeners[eventName] = [];
      capturedListeners[eventName].push(callback as (event: { payload: unknown }) => void);
      return () => {};
    });

    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) =>
      handleCommonInvoke(cmd, args)
    );
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("updates local session title when creation failed (backendCreated: false) without calling update_session_title invoke", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "create_session") {
        throw new Error("backend database error");
      }
      return handleCommonInvoke(cmd, args);
    });

    render(
      <WorkspacesProvider>
        <AppShell />
      </WorkspacesProvider>
    );

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
    // A reply in the open session is already in view, so it does not need you.
    expect(screen.queryByRole("list", { name: "Needs you" })).toBeNull();
    expect(screen.getByRole("button", { name: "Toggle tasks" })).toBeDefined();

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

  it("allows WorkspaceSwitcher selection while a turn is streaming", async () => {
    let activeWorkspace = sampleWorkspace;
    const workspaces = [sampleWorkspace, otherWorkspace];

    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "create_session") return { id: "stream-session" };
      if (cmd === "complete_streaming") return undefined;
      return handleCommonInvoke(cmd, args, {
        workspaces,
        getActiveWorkspace: () => activeWorkspace,
        setActiveWorkspace: (id) => {
          activeWorkspace = workspaces.find((workspace) => workspace.id === id) ?? activeWorkspace;
        },
      });
    });

    render(
      <WorkspacesProvider>
        <AppShell />
      </WorkspacesProvider>
    );

    const workspaceSwitchButton = (await screen.findByRole("button", {
      name: "Switch workspace",
    })) as HTMLButtonElement;
    await waitFor(() => {
      expect(workspaceSwitchButton.disabled).toBe(false);
    });
    fireEvent.click(workspaceSwitchButton);
    const otherWorkspaceButton = await screen.findByRole("button", { name: /Other Workspace/ });

    const textarea = (await screen.findByRole("textbox", {
      name: "Message input",
    })) as HTMLTextAreaElement;
    await waitFor(() => {
      expect(textarea.disabled).toBe(false);
    });

    await act(async () => {
      fireEvent.change(textarea, { target: { value: "streaming prompt" } });
      fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    });
    act(() => {
      triggerEvent<string>("llm-token", "live token");
    });
    expect(await screen.findByText("live token")).toBeDefined();

    // Control plane: switching workspaces never blocks on streams. The view
    // resets to the new workspace draft while the running task stays live in
    // the task rail.
    await act(async () => {
      fireEvent.click(otherWorkspaceButton);
    });

    const setActiveWorkspaceCalls = vi
      .mocked(invoke)
      .mock.calls.filter(([cmd]) => cmd === "set_active_workspace");
    expect(setActiveWorkspaceCalls).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Switch workspace" }).textContent).toContain(
      "Other Workspace"
    );
    const rail = screen.getByTestId("task-rail");
    expect(within(rail).getByRole("list", { name: "Running" }).textContent).toContain(
      "streaming prompt"
    );
    // The new workspace's draft is free to send while the task runs elsewhere.
    expect(textarea.disabled).toBe(false);

    await act(async () => {
      triggerEvent("llm-done", { response: "live token" });
    });
    // The completed turn landed in the background task record and waits for you.
    expect(within(rail).queryByRole("list", { name: "Running" })).toBeNull();
    expect(within(rail).getByRole("list", { name: "Needs you" }).textContent).toContain(
      "streaming prompt"
    );
    expect(screen.getByRole("button", { name: "Toggle tasks, 1 session needs you" })).toBeDefined();

    fireEvent.click(screen.getByTitle("streaming prompt — Test Workspace — New reply"));
    await waitFor(() => {
      expect(within(rail).queryByRole("list", { name: "Needs you" })).toBeNull();
      expect(screen.getByRole("button", { name: "Toggle tasks" })).toBeDefined();
    });
  });

  it("starts and switches sessions from the keyboard while another session runs", async () => {
    const platform = vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
    try {
      vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
        if (cmd === "create_session") return { id: "stream-session" };
        if (cmd === "complete_streaming") return undefined;
        return handleCommonInvoke(cmd, args);
      });

      render(
        <WorkspacesProvider>
          <AppShell />
        </WorkspacesProvider>
      );

      const textarea = (await screen.findByRole("textbox", {
        name: "Message input",
      })) as HTMLTextAreaElement;
      await waitFor(() => {
        expect(textarea.disabled).toBe(false);
      });

      await act(async () => {
        fireEvent.change(textarea, { target: { value: "streaming prompt" } });
        fireEvent.click(screen.getByRole("button", { name: "Send message" }));
      });
      act(() => {
        triggerEvent<string>("llm-token", "live token");
      });
      expect(await screen.findByText("live token")).toBeDefined();

      // Cmd+N opens a draft that can send while the first session keeps running.
      act(() => {
        fireEvent.keyDown(document, { key: "n", metaKey: true });
      });
      const rail = screen.getByTestId("task-rail");
      const runningRow = within(within(rail).getByRole("list", { name: "Running" })).getByTitle(
        "streaming prompt — Test Workspace — Running"
      );
      expect(runningRow.getAttribute("aria-current")).toBeNull();
      expect(screen.queryByText("live token")).toBeNull();
      expect(textarea.disabled).toBe(false);

      await act(async () => {
        triggerEvent("llm-done", { response: "live token" });
      });
      expect(within(rail).getByRole("list", { name: "Needs you" }).textContent).toContain(
        "streaming prompt"
      );
      expect(
        screen.getByRole("button", { name: "Toggle tasks, 1 session needs you" })
      ).toBeDefined();

      // Session shortcuts stand down while the command palette owns the keyboard.
      await act(async () => {
        fireEvent.keyDown(document, { key: "k", metaKey: true });
      });
      const palette = await screen.findByRole("dialog", { name: "Command palette" });
      act(() => {
        fireEvent.keyDown(document, { key: "j", metaKey: true });
      });
      expect(
        screen.getByRole("button", { name: "Toggle tasks, 1 session needs you" })
      ).toBeDefined();
      act(() => {
        fireEvent.keyDown(palette, { key: "Escape" });
      });
      await waitFor(() => {
        expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
      });

      // Cmd+J opens the session that needs you.
      await act(async () => {
        fireEvent.keyDown(document, { key: "j", metaKey: true });
      });
      await waitFor(() => {
        expect(screen.getByRole("button", { name: "Toggle tasks" })).toBeDefined();
      });
      expect(
        screen.getByTitle("streaming prompt — Test Workspace").getAttribute("aria-current")
      ).toBe("true");
      expect(screen.getByText("live token")).toBeDefined();
    } finally {
      platform.mockRestore();
    }
  });
  it("switches Active Workspace Context from the command palette", async () => {
    let activeWorkspace = sampleWorkspace;
    const workspaces = [sampleWorkspace, otherWorkspace];

    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) =>
      handleCommonInvoke(cmd, args, {
        workspaces,
        getActiveWorkspace: () => activeWorkspace,
        setActiveWorkspace: (id) => {
          activeWorkspace = workspaces.find((workspace) => workspace.id === id) ?? activeWorkspace;
        },
      })
    );

    render(
      <WorkspacesProvider>
        <AppShell />
      </WorkspacesProvider>
    );

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Switch workspace" }).textContent).toContain(
        "Test Workspace"
      );
    });

    await act(async () => {
      fireEvent.keyDown(document, { key: "k", metaKey: true });
    });

    const search = await screen.findByLabelText("Search commands");
    fireEvent.change(search, { target: { value: "Other" } });

    fireEvent.click(await screen.findByRole("button", { name: /Other Workspace/ }));

    await waitFor(() => {
      expect(vi.mocked(invoke).mock.calls.some(([cmd]) => cmd === "set_active_workspace")).toBe(
        true
      );
    });
    expect(
      vi.mocked(invoke).mock.calls.find(([cmd]) => cmd === "set_active_workspace")?.[1]
    ).toEqual({ id: "ws-2" });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Switch workspace" }).textContent).toContain(
        "Other Workspace"
      );
    });
  });

  it("switches workspace from the command palette while a turn is streaming", async () => {
    let activeWorkspace = sampleWorkspace;
    const workspaces = [sampleWorkspace, otherWorkspace];

    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "create_session") return { id: "stream-session" };
      if (cmd === "complete_streaming") return undefined;
      return handleCommonInvoke(cmd, args, {
        workspaces,
        getActiveWorkspace: () => activeWorkspace,
        setActiveWorkspace: (id) => {
          activeWorkspace = workspaces.find((workspace) => workspace.id === id) ?? activeWorkspace;
        },
      });
    });

    render(
      <WorkspacesProvider>
        <AppShell />
      </WorkspacesProvider>
    );

    const textarea = (await screen.findByRole("textbox", {
      name: "Message input",
    })) as HTMLTextAreaElement;
    await waitFor(() => {
      expect(textarea.disabled).toBe(false);
    });

    await act(async () => {
      fireEvent.change(textarea, { target: { value: "streaming prompt" } });
      fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    });
    act(() => {
      triggerEvent<string>("llm-token", "live token");
    });
    expect(await screen.findByText("live token")).toBeDefined();

    await act(async () => {
      fireEvent.keyDown(document, { key: "k", metaKey: true });
    });

    const search = await screen.findByLabelText("Search commands");
    fireEvent.change(search, { target: { value: "Other" } });
    fireEvent.click(await screen.findByRole("button", { name: /Other Workspace/ }));

    expect(
      vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "set_active_workspace")
    ).toHaveLength(1);
    expect(screen.queryByLabelText("Search commands")).toBeNull();
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Switch workspace" }).textContent).toContain(
        "Other Workspace"
      );
    });
  });

  it("updates local session title and calls update_session_title invoke when backendCreated is true", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "create_session") return { id: "sess-backend-1" };
      if (cmd === "update_session_title") return undefined;
      return handleCommonInvoke(cmd, args);
    });

    render(
      <WorkspacesProvider>
        <AppShell />
      </WorkspacesProvider>
    );

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

    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "create_session") return { id: "sess-backend-1" };
      if (cmd === "update_session_title") {
        return new Promise<void>((_resolve, reject) => {
          rejectRename = reject;
        });
      }
      return handleCommonInvoke(cmd, args);
    });

    render(
      <WorkspacesProvider>
        <AppShell />
      </WorkspacesProvider>
    );

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
      return handleCommonInvoke(cmd, args);
    });

    render(
      <WorkspacesProvider>
        <AppShell />
      </WorkspacesProvider>
    );

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
