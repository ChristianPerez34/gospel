import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ChatView } from "./ChatView";
import type { CurrentTurn, Message } from "../types";

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
    />,
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
  }: { scrollHeight: number; clientHeight: number; scrollTop: number },
) {
  Object.defineProperties(element, {
    scrollHeight: { configurable: true, value: scrollHeight },
    clientHeight: { configurable: true, value: clientHeight },
  });
  element.scrollTop = scrollTop;
}

describe("ChatView block timeline rendering", () => {
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
    expect(screen.queryByText("Waiting for tool result...")).toBeNull();

    const readRow = screen.getByRole("button", { name: /read file.*running/i });
    expect(readRow.getAttribute("aria-expanded")).toBe("false");
    expect(screen.getByTestId("agent-turn-turn-running").className).toContain("motion-reduce:animate-none");
    expect(readRow.className).toContain("motion-reduce:transition-none");

    fireEvent.click(readRow);

    expect(readRow.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("Waiting for tool result...")).not.toBeNull();
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
    expect(screen.queryByText("pub fn main() {}")).toBeNull();

    const text = container.textContent ?? "";
    expect(text.indexOf("Before read.")).toBeLessThan(text.indexOf("Read file"));
    expect(text.indexOf("Read file")).toBeLessThan(text.indexOf("After read."));

    const readRow = screen.getByRole("button", { name: /read file.*done/i });
    expect(readRow.getAttribute("aria-expanded")).toBe("false");

    fireEvent.click(readRow);

    expect(readRow.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("pub fn main() {}")).not.toBeNull();
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
      />,
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
      />,
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
      />,
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
      />,
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
      />,
    );

    expect(scrollContainer.scrollTop).toBe(1200);
  });
});
