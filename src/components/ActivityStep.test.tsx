import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ActivityStep } from "./ActivityStep";
import type { ActionCard as ActionCardType } from "../types";

afterEach(() => {
  cleanup();
});

function renderStep(card: ActionCardType) {
  return render(
    <ol>
      <ActivityStep card={card} />
    </ol>,
  );
}

function readFileCard(status: ActionCardType["status"]): ActionCardType {
  return {
    id: "tool-read",
    type: "file",
    summary: "Read file",
    detail: "src/lib.rs",
    target: "src/lib.rs",
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

function groupedReadCard(): ActionCardType {
  const single = (id: string, detail: string): ActionCardType => ({
    ...readFileCard("completed"),
    id,
    detail,
  });
  return {
    ...readFileCard("completed"),
    id: "tool-read-group",
    groupCount: 3,
    passes: [
      single("pass-1", "from 1 to 40"),
      single("pass-2", "from 41 to 80"),
      single("pass-3", "from 81 to 120"),
    ],
  };
}

describe("ActivityStep", () => {
  it("starts collapsed and does not auto-open when a running step completes", () => {
    const { rerender } = renderStep(readFileCard("calling"));
    const button = screen.getByRole("button", { name: /read file/i });

    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText("Waiting for tool result...")).toBeNull();

    rerender(
      <ol>
        <ActivityStep card={readFileCard("completed")} />
      </ol>,
    );

    expect(screen.getByRole("button", { name: /read file/i }).getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText("file contents")).toBeNull();
  });

  it("reveals details only after the step is clicked", () => {
    renderStep(readFileCard("completed"));

    expect(screen.queryByText("file contents")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /read file/i }));

    expect(screen.getByText("file contents")).not.toBeNull();
  });

  it("labels a running step for assistive tech without a visible 'Running' badge", () => {
    renderStep(readFileCard("calling"));

    expect(screen.getByRole("button", { name: /read file.*running/i })).not.toBeNull();
    expect(screen.queryByText("Running")).toBeNull();
  });

  it("shows a pass count and renders each pass when steps are grouped", () => {
    renderStep(groupedReadCard());

    expect(screen.getByText("3×")).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /read file/i }));

    expect(screen.getByText(/Pass 1/)).not.toBeNull();
    expect(screen.getByText(/Pass 3/)).not.toBeNull();
    expect(screen.getAllByText("file contents")).toHaveLength(3);
  });
});
