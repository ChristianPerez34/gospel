import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ActionCard as ActionCardType } from "../types";
import { ActivityStep } from "./ActivityStep";

afterEach(() => {
  cleanup();
});

function renderStep(card: ActionCardType) {
  return render(
    <ol>
      <ActivityStep card={card} />
    </ol>
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
  const single = (id: string, detail: string, rawPayload?: string): ActionCardType => ({
    ...readFileCard("completed"),
    id,
    detail,
    rawPayload,
  });
  return {
    ...readFileCard("completed"),
    id: "tool-read-group",
    groupCount: 3,
    passes: [
      single("pass-1", "from 1 to 40", '{"path":"src/lib.rs","bytes":1}'),
      single("pass-2", "from 41 to 80", '{"path":"src/lib.rs","bytes":2}'),
      single("pass-3", "from 81 to 120", '{"path":"src/lib.rs","bytes":3}'),
    ],
  };
}

function diffCard(): ActionCardType {
  return {
    id: "tool-diff",
    type: "diff",
    summary: "Edit file",
    detail: "src/lib.rs",
    target: "src/lib.rs",
    status: "completed",
    expanded: false,
    sections: [
      {
        type: "text",
        title: "Diff",
        content: [
          "@@ src/lib.rs:1 @@",
          "  fn main() {",
          '-    println!("old");',
          '+    println!("new");',
          "  }",
        ].join("\n"),
        monospace: true,
      },
    ],
  };
}

describe("ActivityStep", () => {
  it("starts collapsed and does not auto-open when a running step completes", () => {
    const { rerender } = renderStep(readFileCard("calling"));
    const button = screen.getByRole("button", { name: /read file/i });
    const waitingDisclosure = screen
      .getByText("Waiting for tool result...")
      .closest(".activity-step-disclosure");

    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(waitingDisclosure?.getAttribute("aria-hidden")).toBe("true");

    rerender(
      <ol>
        <ActivityStep card={readFileCard("completed")} />
      </ol>
    );

    expect(screen.getByRole("button", { name: /read file/i }).getAttribute("aria-expanded")).toBe(
      "false"
    );
    expect(
      screen
        .getByText("file contents")
        .closest(".activity-step-disclosure")
        ?.getAttribute("aria-hidden")
    ).toBe("true");
  });

  it("reveals details only after the step is clicked", () => {
    renderStep(readFileCard("completed"));
    const button = screen.getByRole("button", { name: /read file/i });
    const disclosure = screen.getByText("file contents").closest(".activity-step-disclosure");

    expect(disclosure?.getAttribute("aria-hidden")).toBe("true");
    fireEvent.click(button);

    expect(button.getAttribute("aria-expanded")).toBe("true");
    expect(disclosure?.getAttribute("aria-hidden")).toBe("false");
  });

  it("reverses repeated disclosure toggles without remounting its content", () => {
    renderStep(readFileCard("completed"));
    const button = screen.getByRole("button", { name: /read file/i });
    const disclosure = screen.getByText("file contents").closest(".activity-step-disclosure");

    expect(disclosure?.getAttribute("data-open")).toBe("false");

    fireEvent.click(button);
    expect(disclosure?.getAttribute("data-open")).toBe("true");

    fireEvent.click(button);
    expect(disclosure?.getAttribute("data-open")).toBe("false");

    fireEvent.click(button);
    expect(disclosure?.getAttribute("data-open")).toBe("true");
    expect(screen.getByText("file contents").closest(".activity-step-disclosure")).toBe(disclosure);
  });

  it("makes details inert when they arrive while the step remains collapsed", () => {
    const cardWithoutBody = { ...readFileCard("calling"), sections: [] };
    const { rerender } = renderStep(cardWithoutBody);

    expect(document.querySelector(".activity-step-disclosure")).toBeNull();

    rerender(
      <ol>
        <ActivityStep card={readFileCard("completed")} />
      </ol>
    );

    const disclosure = screen
      .getByText("file contents")
      .closest(".activity-step-disclosure") as HTMLDivElement | null;
    expect(disclosure?.getAttribute("aria-hidden")).toBe("true");
    expect(disclosure?.inert).toBe(true);
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

  it("exposes the per-pass raw JSON toggle on grouped steps", () => {
    renderStep(groupedReadCard());

    fireEvent.click(screen.getByRole("button", { name: /read file/i }));

    const toggles = screen.getAllByRole("button", { name: /show raw json/i });
    expect(toggles).toHaveLength(3);

    fireEvent.click(toggles[0]);

    expect(toggles[0].getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText('{"path":"src/lib.rs","bytes":1}')).not.toBeNull();
  });

  it("renders hunk, removed, added, and context rows for a diff preview", () => {
    renderStep(diffCard());
    fireEvent.click(screen.getByRole("button", { name: /edit file/i }));

    expect(screen.getByText("@@ src/lib.rs:1 @@")).not.toBeNull();
    expect(screen.getByText("Removed:").parentElement?.textContent).toContain('println!("old")');
    expect(screen.getByText("Added:").parentElement?.textContent).toContain('println!("new")');
    expect(screen.getAllByText("Context:")[0].parentElement?.textContent).toContain("fn main() {");
  });

  it("keeps textual +/- markers visible on added and removed diff rows", () => {
    renderStep(diffCard());
    fireEvent.click(screen.getByRole("button", { name: /edit file/i }));

    const addedRow = screen.getByText("Added:").parentElement!.parentElement!;
    const removedRow = screen.getByText("Removed:").parentElement!.parentElement!;
    expect(addedRow.textContent).toContain("+");
    expect(removedRow.textContent).toContain("-");
  });

  it("still uses the plain preview and truncation for non-diff text sections", () => {
    renderStep(readFileCard("completed"));
    fireEvent.click(screen.getByRole("button", { name: /read file/i }));

    expect(screen.queryByText(/Context:/)).toBeNull();
    expect(screen.getByText("file contents")).not.toBeNull();
  });
});
