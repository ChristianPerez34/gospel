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

async function openTaskSetup(title: string, instruction = title) {
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Switch workspace" }).textContent).not.toContain(
      "No workspace"
    )
  );
  fireEvent.click(
    within(screen.getByTestId("task-rail")).getByRole("button", { name: "New task" })
  );
  fireEvent.change(await screen.findByLabelText("Task name"), { target: { value: title } });
  fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: instruction } });
  await waitFor(() =>
    expect(
      (screen.getByRole("button", { name: "Create and start task" }) as HTMLButtonElement).disabled
    ).toBe(false)
  );
}

async function startTask(title: string, instruction = title) {
  await openTaskSetup(title, instruction);
  await act(async () =>
    fireEvent.click(screen.getByRole("button", { name: "Create and start task" }))
  );
  return screen.findByRole("textbox", { name: "Message input" });
}

describe("AppShell task flow", () => {
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

  it("requires task setup and saves in the chosen workspace before starting its instruction", async () => {
    let activeWorkspace = sampleWorkspace;
    let saveTask: ((record: { id: string }) => void) | undefined;
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "create_session")
        return new Promise<{ id: string }>((resolve) => {
          saveTask = resolve;
        });
      if (cmd === "complete_streaming") return new Promise(() => {});
      return handleCommonInvoke(cmd, args, {
        workspaces: [sampleWorkspace, otherWorkspace],
        getActiveWorkspace: () => activeWorkspace,
        setActiveWorkspace: (id) => {
          activeWorkspace = id === otherWorkspace.id ? otherWorkspace : sampleWorkspace;
        },
      });
    });
    render(
      <WorkspacesProvider>
        <AppShell />
      </WorkspacesProvider>
    );
    expect(await screen.findByRole("main", { name: "Command center" })).toBeDefined();
    expect(screen.queryByRole("textbox", { name: "Message input" })).toBeNull();
    await openTaskSetup("Billing review", "Review invoices without changing checkout");
    fireEvent.change(screen.getByLabelText("Workspace"), { target: { value: otherWorkspace.id } });
    expect(screen.getByText(otherWorkspace.path)).toBeDefined();
    expect(vi.mocked(invoke).mock.calls.some(([cmd]) => cmd === "set_active_workspace")).toBe(
      false
    );
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Create and start task" }))
    );
    expect(invoke).toHaveBeenCalledWith("create_session", {
      title: "Billing review",
      workspaceId: otherWorkspace.id,
      provider: "openai",
      model: "gpt-4o",
      variant: null,
    });
    expect(screen.queryByRole("textbox", { name: "Message input" })).toBeNull();
    expect(vi.mocked(invoke).mock.calls.some(([cmd]) => cmd === "complete_streaming")).toBe(false);
    await act(async () => {
      saveTask?.({ id: "chosen-workspace-task" });
    });
    expect(await screen.findByRole("textbox", { name: "Message input" })).toBeDefined();
    expect(invoke).toHaveBeenCalledWith(
      "complete_streaming",
      expect.objectContaining({
        sessionId: "chosen-workspace-task",
        prompt: "Review invoices without changing checkout",
      })
    );
    expect(screen.getByRole("button", { name: "Edit session title" }).textContent).toBe(
      "Billing review"
    );
    expect(screen.getByRole("button", { name: "Switch workspace" }).textContent).toContain(
      otherWorkspace.name
    );
    expect(vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "create_session")).toHaveLength(
      1
    );
  });

  it("keeps task setup open and does not run when task creation fails", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "create_session") throw new Error("backend database error");
      return handleCommonInvoke(cmd, args);
    });
    render(
      <WorkspacesProvider>
        <AppShell />
      </WorkspacesProvider>
    );
    await openTaskSetup("Fix auth", "Find and fix the redirect loop");
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Create and start task" }))
    );
    expect(await screen.findByRole("alert")).toBeDefined();
    expect(screen.getByRole("alert").textContent).toContain("backend database error");
    expect(screen.queryByRole("textbox", { name: "Message input" })).toBeNull();
    expect((screen.getByLabelText("Instruction") as HTMLTextAreaElement).value).toBe(
      "Find and fix the redirect loop"
    );
    expect(vi.mocked(invoke).mock.calls.some(([cmd]) => cmd === "complete_streaming")).toBe(false);
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

    await startTask("streaming prompt");
    act(() => {
      triggerEvent<string>("llm-token", "live token");
    });
    expect(await within(screen.getByRole("main")).findByText("live token")).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: "Switch workspace" }));
    const otherWorkspaceButton = await screen.findByRole("button", { name: /Other Workspace/ });

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
    expect(screen.getByRole("main", { name: "New task setup" })).toBeDefined();

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

      await startTask("streaming prompt");
      act(() => {
        triggerEvent<string>("llm-token", "live token");
      });
      expect(await within(screen.getByRole("main")).findByText("live token")).toBeDefined();

      // Cmd+N opens a draft that can send while the first session keeps running.
      act(() => {
        fireEvent.keyDown(document, { key: "n", metaKey: true });
      });
      const rail = screen.getByTestId("task-rail");
      const runningRow = within(within(rail).getByRole("list", { name: "Running" })).getByTitle(
        "streaming prompt — Test Workspace — Running"
      );
      expect(runningRow.getAttribute("aria-current")).toBeNull();
      expect(within(screen.getByRole("main")).queryByText("live token")).toBeNull();
      expect(screen.getByRole("main", { name: "New task setup" })).toBeDefined();

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

  it("returns to the overview without stopping a task and surfaces its background reply", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "create_session") return { id: "overview-run" };
      return handleCommonInvoke(cmd, args);
    });
    render(
      <WorkspacesProvider>
        <AppShell />
      </WorkspacesProvider>
    );
    expect(await screen.findByRole("main", { name: "Command center" })).toBeDefined();
    await startTask("Keep this task running");
    act(() => triggerEvent<string>("llm-token", "Working in the background"));
    await within(screen.getByRole("main")).findByText("Working in the background");
    fireEvent.click(screen.getByRole("button", { name: "Gospel command center" }));
    const overview = screen.getByRole("main", { name: "Command center" });
    expect(await within(overview).findByText("Working in the background")).toBeDefined();
    expect(within(within(overview).getByRole("list")).getByText("Running")).toBeDefined();
    expect(screen.queryByRole("textbox", { name: "Message input" })).toBeNull();
    expect(vi.mocked(invoke).mock.calls.some(([cmd]) => cmd === "cancel_streaming")).toBe(false);
    await act(async () => triggerEvent("llm-done", { response: "Working in the background" }));
    expect(within(overview).getByText("New reply")).toBeDefined();
    expect(within(overview).getByText("Working in the background")).toBeDefined();
    await act(async () =>
      fireEvent.click(
        within(overview).getByRole("button", {
          name: "Open Keep this task running in Test Workspace",
        })
      )
    );
    expect(screen.queryByRole("main", { name: "Command center" })).toBeNull();
    expect(within(screen.getByRole("main")).getByText("Working in the background")).toBeDefined();
    expect(screen.queryByRole("button", { name: "Toggle tasks, 1 session needs you" })).toBeNull();
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

    await startTask("streaming prompt");
    act(() => {
      triggerEvent<string>("llm-token", "live token");
    });
    expect(await within(screen.getByRole("main")).findByText("live token")).toBeDefined();

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

    await startTask("Backend session prompt");

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

    await startTask("Backend session prompt");

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

    await startTask("Backend session prompt");

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
