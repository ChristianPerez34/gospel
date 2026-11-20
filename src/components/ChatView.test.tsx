import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { CurrentTurn, Message } from "../types";
import { ChatView } from "./ChatView";

const userMessage: Message = {
  id: "m-user",
  role: "user",
  content: "Inspect the stream",
  timestamp: new Date("2026-06-24T00:00:00Z"),
};

afterEach(() => {
  cleanup();
});

function renderChat({
  messages = [userMessage],
  currentTurn = null,
  isThinking = false,
}: {
  messages?: Message[];
  currentTurn?: CurrentTurn | null;
  isThinking?: boolean;
} = {}) {
  return render(
    <ChatView
      messages={messages}
      workspacePath="/workspace/gospel"
      isThinking={isThinking}
      currentTurn={currentTurn}
    />
  );
}

function chatView(container: HTMLElement) {
  const element = container.querySelector(".chat-view");
  if (!(element instanceof HTMLElement)) {
    throw new Error("Expected chat view element to be rendered");
  }
  return element;
}

function defineScrollMetrics(
  element: HTMLElement,
  {
    scrollHeight,
    clientHeight,
    scrollTop,
  }: { scrollHeight: number; clientHeight: number; scrollTop: number }
) {
  Object.defineProperties(element, {
    scrollHeight: { configurable: true, value: scrollHeight },
    clientHeight: { configurable: true, value: clientHeight },
  });
  element.scrollTop = scrollTop;
}

describe("ChatView block timeline rendering", () => {
  it("shows a reply's background verification with its steps and verdict", () => {
    const reply: Message = {
      id: "answer",
      role: "agent",
      content: "Done",
      timestamp: new Date(),
      blocks: [{ kind: "text", id: "text-1", text: "Done" }],
      verification: {
        status: "running",
        activity: {
          agent: "verification",
          steps: [
            { id: "v1", name: "read_file", arguments: { path: "src/lib.rs" }, status: "calling" },
          ],
          text: "",
        },
        concerns: [],
      },
    };
    const { rerender } = renderChat({ messages: [userMessage, reply] });
    const row = screen.getByTestId("verification-activity");
    expect(row.textContent).toContain("Verify response");
    expect(row.textContent).toContain("Checking…");
    expect(screen.getByTestId("subagent-live-label").textContent).toContain("Read file src/lib.rs");

    rerender(
      <ChatView
        messages={[
          userMessage,
          {
            ...reply,
            verification: {
              status: "concerns",
              summary: "One gap",
              concerns: ["No test for the empty case"],
              activity: {
                ...reply.verification!.activity,
                steps: [{ ...reply.verification!.activity.steps[0], status: "completed" }],
              },
            },
          },
        ]}
        workspacePath="/workspace/gospel"
        isThinking={false}
        currentTurn={null}
      />
    );
    expect(screen.getByTestId("verification-activity").textContent).toContain("Concerns found");
    expect(screen.queryByTestId("subagent-live-label")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /verify response/i }));
    expect(screen.getByText(/No test for the empty case/)).not.toBeNull();
  });
  it("copies assistant text without including reasoning or tool payloads", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    renderChat({
      messages: [
        {
          id: "answer",
          role: "agent",
          content: "fallback",
          timestamp: new Date(),
          blocks: [
            { kind: "text", id: "text-1", text: "First paragraph." },
            { kind: "reasoning", id: "reason-1", text: "Private reasoning.", phase: "complete" },
            {
              kind: "tool",
              id: "tool-1",
              name: "read_file",
              status: "completed",
              result: "private payload",
            },
            { kind: "text", id: "text-2", text: "Second paragraph." },
          ],
        },
      ],
    });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Copy message" })));
    expect(writeText).toHaveBeenCalledWith("First paragraph.\n\nSecond paragraph.");
    expect(screen.getByText("Copied")).not.toBeNull();
  });

  it("reports clipboard failures", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: vi.fn().mockRejectedValue(new Error("Permission denied")),
      },
    });
    renderChat();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Copy message" })));
    expect(screen.getByRole("alert").textContent).toContain("Unable to copy");
  });

  it("routes retry and fork to the selected assistant message and disables them while busy", () => {
    const onRetry = vi.fn().mockResolvedValue(undefined);
    const onFork = vi.fn().mockResolvedValue(undefined);
    const answer: Message = {
      id: "answer",
      role: "agent",
      content: "Response",
      timestamp: new Date(),
    };
    const props = {
      messages: [userMessage, answer],
      workspacePath: "/workspace",
      isThinking: false,
      onRetry,
      onFork,
    };
    const { rerender } = render(<ChatView {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Retry message" }));
    fireEvent.click(screen.getByRole("button", { name: "Fork conversation" }));
    expect(onRetry).toHaveBeenCalledWith("answer");
    expect(onFork).toHaveBeenCalledWith("answer");
    rerender(<ChatView {...props} actionsDisabled />);
    fireEvent.click(screen.getByRole("button", { name: "Retry message" }));
    fireEvent.click(screen.getByRole("button", { name: "Fork conversation" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(onFork).toHaveBeenCalledTimes(1);
  });

  it("renders live text and tool blocks in occurrence order", () => {
    const { container } = renderChat({
      currentTurn: {
        id: "turn-1",
        createdAt: new Date("2026-06-24T00:00:30Z"),
        blocks: [
          { kind: "text", id: "text-0", text: "First, I will search." },
          {
            kind: "tool",
            id: "tool-search",
            name: "search_code",
            arguments: { pattern: "currentTurn" },
            result: JSON.stringify({ matches: [], scanned_files: 2 }),
            status: "completed",
          },
          { kind: "text", id: "text-2", text: "Then I will read the file." },
          {
            kind: "tool",
            id: "tool-read",
            name: "read_file",
            arguments: { path: "src/components/ChatView.tsx" },
            result: JSON.stringify({
              path: "src/components/ChatView.tsx",
              content: "export function ChatView() {}",
            }),
            status: "completed",
          },
        ],
      },
    });

    expect(screen.getAllByTestId("tool-row-label").map((node) => node.textContent)).toEqual([
      "Search code",
      "Read file",
    ]);
    expect(screen.queryByRole("button", { name: /tool activity/i })).toBeNull();

    const text = container.textContent ?? "";
    expect(text.indexOf("First, I will search.")).toBeLessThan(text.indexOf("Search code"));
    expect(text.indexOf("Search code")).toBeLessThan(text.indexOf("Then I will read the file."));
    expect(text.indexOf("Then I will read the file.")).toBeLessThan(text.indexOf("Read file"));
  });

  it("keeps the live running pill and expands a running tool row in place", () => {
    renderChat({
      currentTurn: {
        id: "turn-running",
        createdAt: new Date("2026-06-24T00:00:30Z"),
        blocks: [
          {
            kind: "tool",
            id: "tool-read",
            name: "read_file",
            arguments: { path: "src/components/ChatView.tsx" },
            status: "calling",
          },
        ],
      },
    });

    expect(screen.getByText("Read file...")).not.toBeNull();
    expect(screen.getByTestId("inline-tool-activity-list").className).toContain("max-w-[960px]");
    const waitingDisclosure = screen
      .getByText("Waiting for tool result...")
      .closest(".activity-step-disclosure");
    expect(waitingDisclosure?.getAttribute("aria-hidden")).toBe("true");

    const readRow = screen.getByRole("button", { name: /read file.*running/i });
    expect(readRow.getAttribute("aria-expanded")).toBe("false");
    expect(screen.getByTestId("agent-turn-turn-running").className).not.toContain(
      "animate-fade-slide-in-fast"
    );
    expect(readRow.className).toContain("motion-reduce:transition-none");

    fireEvent.click(readRow);

    expect(readRow.getAttribute("aria-expanded")).toBe("true");
    expect(waitingDisclosure?.getAttribute("aria-hidden")).toBe("false");
  });

  it("renders finalized tool blocks inline and collapsed by default", () => {
    const { container } = renderChat({
      messages: [
        userMessage,
        {
          id: "turn-final",
          role: "agent",
          content: "Before read.After read.",
          timestamp: new Date("2026-06-24T00:01:00Z"),
          blocks: [
            { kind: "text", id: "text-0", text: "Before read." },
            {
              kind: "tool",
              id: "tool-read",
              name: "read_file",
              arguments: { path: "src/lib.rs" },
              result: JSON.stringify({
                path: "src/lib.rs",
                content: "pub fn main() {}",
              }),
              status: "completed",
            },
            { kind: "text", id: "text-2", text: "After read." },
          ],
        },
      ],
    });

    expect(screen.queryByRole("button", { name: /tool activity/i })).toBeNull();
    expect(screen.queryByText("Running")).toBeNull();
    const resultDisclosure = screen
      .getByText("pub fn main() {}")
      .closest(".activity-step-disclosure");
    expect(resultDisclosure?.getAttribute("aria-hidden")).toBe("true");

    const text = container.textContent ?? "";
    expect(text.indexOf("Before read.")).toBeLessThan(text.indexOf("Read file"));
    expect(text.indexOf("Read file")).toBeLessThan(text.indexOf("After read."));

    const readRow = screen.getByRole("button", { name: /read file.*done/i });
    expect(readRow.getAttribute("aria-expanded")).toBe("false");

    fireEvent.click(readRow);

    expect(readRow.getAttribute("aria-expanded")).toBe("true");
    expect(resultDisclosure?.getAttribute("aria-hidden")).toBe("false");
  });

  it("renders consecutive tool blocks inside a single shared timeline", () => {
    const { container } = renderChat({
      currentTurn: {
        id: "turn-tools-run",
        createdAt: new Date("2026-06-24T00:00:30Z"),
        blocks: [
          {
            kind: "tool",
            id: "tool-search",
            name: "search_code",
            arguments: { pattern: "currentTurn" },
            result: JSON.stringify({ matches: [], scanned_files: 2 }),
            status: "completed",
          },
          {
            kind: "tool",
            id: "tool-read",
            name: "read_file",
            arguments: { path: "src/components/ChatView.tsx" },
            result: JSON.stringify({
              path: "src/components/ChatView.tsx",
              content: "export function ChatView() {}",
            }),
            status: "completed",
          },
        ],
      },
    });

    expect(screen.getAllByTestId("inline-tool-activity-list")).toHaveLength(1);

    const items = container.querySelectorAll("[data-testid='inline-tool-activity-list'] li");
    expect(items).toHaveLength(2);
  });

  it("hides a resolved approval card while the tool call shows the command and outcome", () => {
    const tool = {
      kind: "tool" as const,
      id: "tool-cmd",
      name: "run_shell_command",
      arguments: { program: "bun", args: ["run", "test"] },
      result: JSON.stringify({
        success: true,
        exit_code: 0,
        stdout: "440 passed",
        stderr: "",
        truncated: false,
      }),
      status: "completed" as const,
    };
    const approval = {
      kind: "approval" as const,
      id: "approval-1",
      toolName: "run_shell_command",
      approvalKind: "command" as const,
      title: "Allow mutating command?",
      summary: "bun run test",
      reason: "Mutating command",
      risk: "mutating" as const,
      status: "approved" as const,
    };
    const { container, rerender } = renderChat({
      currentTurn: {
        id: "turn-approval",
        createdAt: new Date("2026-06-24T00:00:30Z"),
        blocks: [tool, approval],
      },
    });

    expect(container.querySelector(".approval-card")).toBeNull();
    expect(screen.getByRole("button", { name: "Run command bun run test Done" })).not.toBeNull();

    rerender(
      <ChatView
        messages={[userMessage]}
        workspacePath="/workspace/gospel"
        isThinking={false}
        currentTurn={{
          id: "turn-approval",
          createdAt: new Date("2026-06-24T00:00:30Z"),
          blocks: [tool, { ...approval, status: "pending" }],
        }}
      />
    );

    expect(container.querySelector(".approval-card")).not.toBeNull();
  });

  it.each([
    "denied",
    "auto_denied",
    "timed_out",
  ] as const)("surfaces Denied on the collapsed tool row when an approval resolves as %s", (approvalStatus) => {
    const { container } = renderChat({
      currentTurn: {
        id: `turn-${approvalStatus}`,
        createdAt: new Date("2026-06-24T00:00:30Z"),
        blocks: [
          {
            kind: "tool",
            id: "tool-cmd",
            name: "run_shell_command",
            arguments: { program: "rm", args: ["-rf", "build"] },
            result: JSON.stringify({
              success: false,
              exit_code: -1,
              stdout: "",
              stderr: "",
              truncated: false,
              needs_approval: true,
              reason: "approval_denied",
              message:
                "Approval denied for `rm -rf build`. The user did not authorize this command.",
            }),
            status: "completed",
          },
          {
            kind: "approval",
            id: "approval-denied",
            toolName: "run_shell_command",
            approvalKind: "command",
            title: "Allow destructive command?",
            summary: "rm -rf build",
            reason: "Destructive command",
            risk: "destructive",
            status: approvalStatus,
          },
        ],
      },
    });

    expect(container.querySelector(".approval-card")).toBeNull();
    expect(screen.getByRole("button", { name: "Run command rm -rf build Denied" })).not.toBeNull();
    expect(screen.getByText("Denied")).not.toBeNull();
  });

  it("drops stale pending approvals from finalized messages and ignores resolved approvals before coalescing", () => {
    const resolvedApproval = {
      kind: "approval" as const,
      id: "approval-resolved",
      toolName: "run_shell_command",
      approvalKind: "command" as const,
      title: "Allow mutating command?",
      summary: "bun run test",
      reason: "Mutating command",
      risk: "mutating" as const,
      status: "approved" as const,
    };
    const { container, rerender } = renderChat({
      isThinking: true,
      currentTurn: {
        id: "turn-thinking-fallback",
        createdAt: new Date("2026-06-24T00:00:30Z"),
        blocks: [resolvedApproval],
      },
    });

    expect(screen.getByText("Thinking...")).not.toBeNull();
    expect(container.querySelector(".approval-card")).toBeNull();
    expect(screen.queryByTestId("inline-tool-activity-list")).toBeNull();

    rerender(
      <ChatView
        messages={[
          userMessage,
          {
            id: "turn-finalized-stale-approval",
            role: "agent",
            content: "Failed before decision.",
            error: "Completion failed.",
            timestamp: new Date("2026-06-24T00:01:00Z"),
            blocks: [
              { kind: "text", id: "t1", text: "Failed before decision." },
              { ...resolvedApproval, id: "approval-stale-pending", status: "pending" },
            ],
          },
        ]}
        workspacePath="/workspace/gospel"
        isThinking={false}
        currentTurn={null}
      />
    );

    expect(container.querySelector(".approval-card")).toBeNull();
    expect(screen.queryByText("Awaiting your decision")).toBeNull();
  });

  it("shows a cancelled tool in a finalized turn", () => {
    renderChat({
      messages: [
        userMessage,
        {
          id: "turn-cancelled",
          role: "agent",
          content: "Stream cancelled by user.",
          timestamp: new Date("2026-06-24T00:01:00Z"),
          blocks: [
            {
              kind: "tool",
              id: "tool-cmd",
              name: "run_shell_command",
              arguments: { program: "bun", args: ["run", "test"] },
              status: "cancelled",
            },
          ],
        },
      ],
    });

    expect(screen.getByRole("button", { name: /run command.*cancelled/i })).not.toBeNull();
    expect(screen.getByText("Cancelled")).not.toBeNull();
    expect(screen.queryByText("Waiting for tool result...")).toBeNull();
  });

  it("renders historical text-only sessions without stale tool rows", () => {
    renderChat({
      messages: [
        userMessage,
        {
          id: "m-agent",
          role: "agent",
          content: "Historical reply.",
          timestamp: new Date("2026-06-24T00:01:00Z"),
        },
      ],
    });

    expect(screen.getByText("Historical reply.")).not.toBeNull();
    expect(screen.queryByTestId("inline-tool-activity-list")).toBeNull();
    expect(screen.queryByRole("button", { name: /tool activity/i })).toBeNull();
  });

  it("preserves the keyed agent turn container when live content finalizes", () => {
    const { rerender } = renderChat({
      currentTurn: {
        id: "turn-stable",
        createdAt: new Date("2026-06-24T00:00:30Z"),
        blocks: [{ kind: "text", id: "text-0", text: "Live text" }],
      },
    });
    const liveContainer = screen.getByTestId("agent-turn-turn-stable");

    rerender(
      <ChatView
        messages={[
          userMessage,
          {
            id: "turn-stable",
            role: "agent",
            content: "Final text",
            timestamp: new Date("2026-06-24T00:01:00Z"),
            blocks: [{ kind: "text", id: "text-0", text: "Final text" }],
          },
        ]}
        workspacePath="/workspace/gospel"
        isThinking={false}
        currentTurn={null}
      />
    );

    expect(screen.getByTestId("agent-turn-turn-stable")).toBe(liveContainer);
  });

  it("renders a thinking placeholder during the latency gap before the first stream event", () => {
    renderChat({
      messages: [userMessage],
      isThinking: true,
      currentTurn: null,
    });

    expect(screen.getByText("Thinking...")).not.toBeNull();
    expect(screen.getByTestId("agent-turn-thinking-placeholder")).not.toBeNull();
  });

  it("renders live reasoning blocks as subdued text in occurrence order", () => {
    const { container } = renderChat({
      currentTurn: {
        id: "turn-reasoning",
        createdAt: new Date("2026-06-24T00:00:30Z"),
        blocks: [
          { kind: "text", id: "text-0", text: "Public answer." },
          {
            kind: "reasoning",
            id: "rs-1",
            text: "private thoughts",
            phase: "complete",
          },
        ],
      },
    });

    const reasoning = screen.getByTestId("agent-reasoning-rs-1");
    expect(reasoning.textContent).toBe("private thoughts");
    expect(reasoning.className).toMatch(/text-text-muted/);
    expect(reasoning.className).toMatch(/font-mono/);

    const text = container.textContent ?? "";
    expect(text.indexOf("Public answer.")).toBeLessThan(text.indexOf("private thoughts"));
  });

  it("does not render reasoning blocks for finalized messages", () => {
    renderChat({
      messages: [
        userMessage,
        {
          id: "m-finalized",
          role: "agent",
          content: "Public answer.",
          timestamp: new Date("2026-06-24T00:01:00Z"),
          blocks: [
            { kind: "text", id: "text-0", text: "Public answer." },
            {
              kind: "reasoning",
              id: "rs-1",
              text: "private thoughts",
              phase: "complete",
            },
          ],
        },
      ],
    });

    // Reasoning blocks are dropped from finalized messages; they must not
    // render in the historical timeline.
    expect(screen.queryByTestId("agent-reasoning-rs-1")).toBeNull();
    expect(screen.getByText("Public answer.")).not.toBeNull();
  });

  it("does not render a thinking placeholder when not thinking", () => {
    renderChat({
      messages: [userMessage],
      isThinking: false,
      currentTurn: null,
    });

    expect(screen.queryByText("Thinking...")).toBeNull();
    expect(screen.queryByTestId("agent-turn-thinking-placeholder")).toBeNull();
  });
});

describe("ChatView scroll following", () => {
  it("auto-scrolls live updates when the chat is near the bottom", () => {
    const { container, rerender } = renderChat();
    const scrollContainer = chatView(container);
    defineScrollMetrics(scrollContainer, {
      scrollHeight: 1200,
      clientHeight: 400,
      scrollTop: 800,
    });

    fireEvent.scroll(scrollContainer);

    rerender(
      <ChatView
        messages={[userMessage]}
        workspacePath="/workspace/gospel"
        isThinking={true}
        currentTurn={{
          id: "turn-follow",
          createdAt: new Date("2026-06-24T00:00:30Z"),
          blocks: [{ kind: "text", id: "text-0", text: "Streaming text" }],
        }}
      />
    );

    expect(scrollContainer.scrollTop).toBe(1200);
  });

  it("does not auto-scroll live updates after the user scrolls away from the bottom", () => {
    const { container, rerender } = renderChat();
    const scrollContainer = chatView(container);
    defineScrollMetrics(scrollContainer, {
      scrollHeight: 1200,
      clientHeight: 400,
      scrollTop: 500,
    });

    fireEvent.scroll(scrollContainer);

    rerender(
      <ChatView
        messages={[userMessage]}
        workspacePath="/workspace/gospel"
        isThinking={true}
        currentTurn={{
          id: "turn-paused",
          createdAt: new Date("2026-06-24T00:00:30Z"),
          blocks: [{ kind: "text", id: "text-0", text: "Streaming text" }],
        }}
      />
    );

    expect(scrollContainer.scrollTop).toBe(500);
  });

  it("resumes auto-scroll when a new user turn is submitted while scrolled away", () => {
    const { container, rerender } = renderChat();
    const scrollContainer = chatView(container);
    defineScrollMetrics(scrollContainer, {
      scrollHeight: 1200,
      clientHeight: 400,
      scrollTop: 500,
    });

    fireEvent.scroll(scrollContainer);

    rerender(
      <ChatView
        messages={[
          userMessage,
          {
            id: "m-user-2",
            role: "user",
            content: "Follow up prompt",
            timestamp: new Date("2026-06-24T00:02:00Z"),
          },
        ]}
        workspacePath="/workspace/gospel"
        isThinking={true}
        currentTurn={null}
      />
    );

    expect(scrollContainer.scrollTop).toBe(1200);
  });

  it("resumes auto-scroll after the user returns near the bottom", () => {
    const { container, rerender } = renderChat({
      currentTurn: {
        id: "turn-resume",
        createdAt: new Date("2026-06-24T00:00:30Z"),
        blocks: [{ kind: "text", id: "text-0", text: "First chunk" }],
      },
    });
    const scrollContainer = chatView(container);
    defineScrollMetrics(scrollContainer, {
      scrollHeight: 1200,
      clientHeight: 400,
      scrollTop: 500,
    });
    fireEvent.scroll(scrollContainer);

    scrollContainer.scrollTop = 760;
    fireEvent.scroll(scrollContainer);

    rerender(
      <ChatView
        messages={[userMessage]}
        workspacePath="/workspace/gospel"
        isThinking={true}
        currentTurn={{
          id: "turn-resume",
          createdAt: new Date("2026-06-24T00:00:30Z"),
          blocks: [
            { kind: "text", id: "text-0", text: "First chunk" },
            {
              kind: "tool",
              id: "tool-read",
              name: "read_file",
              arguments: { path: "src/components/ChatView.tsx" },
              status: "calling",
            },
          ],
        }}
      />
    );

    expect(scrollContainer.scrollTop).toBe(1200);
  });
});

describe("ChatView history loading", () => {
  it("shows skeleton rows instead of the empty state while history loads", () => {
    render(
      <ChatView
        messages={[]}
        workspacePath="/workspace/gospel"
        isThinking={false}
        currentTurn={null}
        historyLoading={true}
      />
    );

    expect(screen.getByTestId("chat-history-skeleton")).toBeTruthy();
    expect(screen.queryByText("What can I help you with?")).toBeNull();
  });

  it("renders an earlier-history sentinel that loads more on click", () => {
    const onLoadEarlier = vi.fn();
    render(
      <ChatView
        messages={[userMessage]}
        workspacePath="/workspace/gospel"
        isThinking={false}
        hasMoreHistory={true}
        onLoadEarlier={onLoadEarlier}
      />
    );

    fireEvent.click(screen.getByTestId("chat-earlier-sentinel"));
    expect(onLoadEarlier).toHaveBeenCalledTimes(1);
  });

  it("does not count live-turn growth when anchoring an earlier page", () => {
    const messages = [userMessage];
    const { container, rerender } = render(
      <ChatView
        messages={messages}
        workspacePath="/workspace/gospel"
        isThinking={true}
        currentTurn={{
          id: "live",
          createdAt: new Date(),
          blocks: [{ kind: "text", id: "live-text", text: "start" }],
        }}
        hasMoreHistory={true}
      />
    );
    const scrollContainer = chatView(container);
    defineScrollMetrics(scrollContainer, { scrollHeight: 1000, clientHeight: 400, scrollTop: 300 });
    fireEvent.scroll(scrollContainer);
    defineScrollMetrics(scrollContainer, { scrollHeight: 1300, clientHeight: 400, scrollTop: 300 });
    rerender(
      <ChatView
        messages={messages}
        workspacePath="/workspace/gospel"
        isThinking={true}
        currentTurn={{
          id: "live",
          createdAt: new Date(),
          blocks: [{ kind: "text", id: "live-text", text: "more live text" }],
        }}
        hasMoreHistory={true}
      />
    );
    const older: Message = {
      id: "older",
      role: "user",
      content: "Earlier page",
      timestamp: new Date(),
    };
    defineScrollMetrics(scrollContainer, { scrollHeight: 1700, clientHeight: 400, scrollTop: 300 });
    rerender(
      <ChatView
        messages={[older, userMessage]}
        workspacePath="/workspace/gospel"
        isThinking={true}
        currentTurn={{
          id: "live",
          createdAt: new Date(),
          blocks: [{ kind: "text", id: "live-text", text: "more live text" }],
        }}
        hasMoreHistory={true}
      />
    );
    expect(scrollContainer.scrollTop).toBe(700);
  });

  it("anchors the viewport on the same message when earlier history prepends", () => {
    const { container, rerender } = render(
      <ChatView
        messages={[userMessage]}
        workspacePath="/workspace/gospel"
        isThinking={false}
        hasMoreHistory={true}
      />
    );
    const scrollContainer = chatView(container);
    defineScrollMetrics(scrollContainer, {
      scrollHeight: 1000,
      clientHeight: 400,
      scrollTop: 300,
    });
    fireEvent.scroll(scrollContainer);

    const older: Message = {
      id: "m-older",
      role: "user",
      content: "Earlier page",
      timestamp: new Date("2026-06-23T00:00:00Z"),
    };
    // The feed grew by the prepended page's height.
    defineScrollMetrics(scrollContainer, {
      scrollHeight: 1400,
      clientHeight: 400,
      scrollTop: 300,
    });
    rerender(
      <ChatView
        messages={[older, userMessage]}
        workspacePath="/workspace/gospel"
        isThinking={false}
        hasMoreHistory={false}
      />
    );

    // scrollTop shifted by the added 400px — the same message stays in view.
    expect(scrollContainer.scrollTop).toBe(700);
  });
});
