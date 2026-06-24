import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ActionCard } from "./ActionCard";
import type { ActionCard as ActionCardType } from "../types";

afterEach(() => {
  cleanup();
});

function readFileCard(status: ActionCardType["status"]): ActionCardType {
  return {
    id: "tool-read",
    type: "file",
    summary: "Read file",
    detail: "src/lib.rs",
    status,
    expanded: false,
    sections: [
      {
        type: "text",
        title: "Result",
        content: status === "calling" ? "Waiting for tool result..." : "file contents",
        monospace: true,
      },
    ],
  };
}

describe("ActionCard", () => {
  it("starts collapsed and does not auto-open when a running card completes", () => {
    const { rerender } = render(<ActionCard card={readFileCard("calling")} />);
    const button = screen.getByRole("button", { name: /read file/i });

    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText("Waiting for tool result...")).toBeNull();

    rerender(<ActionCard card={readFileCard("completed")} />);

    expect(screen.getByRole("button", { name: /read file/i }).getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText("file contents")).toBeNull();
  });

  it("reveals details only after the card is clicked", () => {
    render(<ActionCard card={readFileCard("completed")} />);

    expect(screen.queryByText("file contents")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /read file/i }));

    expect(screen.getByText("file contents")).not.toBeNull();
  });
});
