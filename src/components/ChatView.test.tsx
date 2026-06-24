import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ChatView } from "./ChatView";
import type { CurrentTurn, FinalizedToolActivity, Message } from "../types";

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
  finalizedToolActivities = [],
}: {
  messages?: Message[];
  currentTurn?: CurrentTurn | null;
  finalizedToolActivities?: FinalizedToolActivity[];
}) {
  return render(
    <ChatView
      messages={messages}
      workspacePath="/workspace/gospel"
      isThinking={false}
      currentTurn={currentTurn}
      finalizedToolActivities={finalizedToolActivities}
    />,
  );
}

describe("ChatView current turn rendering", () => {
  it("renders live tool rows in occurrence order and keeps details collapsed until clicked", () => {
    renderChat({
      currentTurn: {
        id: "turn-1",
        content: "",
        createdAt: new Date("2026-06-24T00:00:30Z"),
        toolActivities: [
          {
            id: "tool-search",
            name: "search_code",
            arguments: { pattern: "currentTurn" },
            result: JSON.stringify({ matches: [], scanned_files: 2 }),
            status: "completed",
          },
          {
            id: "tool-read",
            name: "read_file",
            arguments: { path: "src/components/ChatView.tsx" },
            status: "calling",
          },
        ],
      },
    });

    expect(screen.getAllByTestId("live-tool-row-label").map((node) => node.textContent)).toEqual([
      "Search code",
      "Read file",
    ]);
    expect(screen.getByTestId("live-tool-activity-list").className).toContain("max-w-[720px]");
    expect(screen.getByText("Read file...")).not.toBeNull();
    expect(screen.queryByText("Waiting for tool result...")).toBeNull();

    const readRow = screen.getByRole("button", { name: /read file.*running/i });
    expect(readRow.getAttribute("aria-expanded")).toBe("false");

    fireEvent.click(readRow);

    expect(readRow.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("Waiting for tool result...")).not.toBeNull();
  });

  it("renders finalized tool history behind a collapsed disclosure with width constraints", () => {
    renderChat({
      messages: [
        userMessage,
        {
          id: "turn-1",
          role: "agent",
          content: "Done.",
          timestamp: new Date("2026-06-24T00:01:00Z"),
        },
      ],
      finalizedToolActivities: [
        {
          messageId: "turn-1",
          activities: [
            {
              id: "tool-read",
              name: "read_file",
              arguments: { path: "src/lib.rs" },
              result: JSON.stringify({
                path: "src/lib.rs",
                content: "pub fn main() {}",
              }),
              status: "completed",
            },
          ],
        },
      ],
    });

    const disclosure = screen.getByRole("button", { name: /tool activity \(1\)/i });
    expect(disclosure.getAttribute("aria-expanded")).toBe("false");
    expect(screen.getByTestId("finalized-tool-activity").className).toContain("max-w-[720px]");
    expect(screen.queryByText("Read file")).toBeNull();

    fireEvent.click(disclosure);

    expect(disclosure.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByTestId("finalized-tool-activity-cards").className).toContain("max-w-[960px]");
    expect(screen.getByText("Read file")).not.toBeNull();
    expect(screen.queryByText("pub fn main() {}")).toBeNull();
  });

  it("renders historical text-only sessions without stale tool disclosures", () => {
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
      finalizedToolActivities: [
        {
          messageId: "other-turn",
          activities: [
            {
              id: "tool-read",
              name: "read_file",
              status: "completed",
            },
          ],
        },
      ],
    });

    expect(screen.getByText("Historical reply.")).not.toBeNull();
    expect(screen.queryByRole("button", { name: /tool activity/i })).toBeNull();
  });

  it("preserves the keyed agent turn container when live content finalizes", () => {
    const { rerender } = renderChat({
      currentTurn: {
        id: "turn-stable",
        content: "Live text",
        createdAt: new Date("2026-06-24T00:00:30Z"),
        toolActivities: [],
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
          },
        ]}
        workspacePath="/workspace/gospel"
        isThinking={false}
        currentTurn={null}
        finalizedToolActivities={[]}
      />,
    );

    expect(screen.getByTestId("agent-turn-turn-stable")).toBe(liveContainer);
  });

  it("marks new live turn animation surfaces as reduced-motion aware", () => {
    renderChat({
      currentTurn: {
        id: "turn-motion",
        content: "",
        createdAt: new Date("2026-06-24T00:00:30Z"),
        toolActivities: [
          {
            id: "tool-read",
            name: "read_file",
            status: "calling",
          },
        ],
      },
    });

    expect(screen.getByTestId("agent-turn-turn-motion").className).toContain("motion-reduce:animate-none");
    expect(screen.getByRole("button", { name: /read file.*running/i }).className).toContain("motion-reduce:transition-none");
  });

  it("renders a thinking placeholder during the latency gap before the first stream event", () => {
    render(
      <ChatView
        messages={[userMessage]}
        workspacePath="/workspace/gospel"
        isThinking={true}
        currentTurn={null}
        finalizedToolActivities={[]}
      />,
    );

    expect(screen.getByText("Thinking...")).not.toBeNull();
    expect(screen.getByTestId("agent-turn-thinking-placeholder")).not.toBeNull();
  });

  it("does not render a thinking placeholder when not thinking", () => {
    render(
      <ChatView
        messages={[userMessage]}
        workspacePath="/workspace/gospel"
        isThinking={false}
        currentTurn={null}
        finalizedToolActivities={[]}
      />,
    );

    expect(screen.queryByText("Thinking...")).toBeNull();
    expect(screen.queryByTestId("agent-turn-thinking-placeholder")).toBeNull();
  });
});
