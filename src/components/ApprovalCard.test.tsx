import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { TurnBlock } from "../types";
import { ApprovalCard } from "./ApprovalCard";

afterEach(() => {
  cleanup();
});

function pendingApproval(
  overrides: Partial<Extract<TurnBlock, { kind: "approval" }>> = {}
): Extract<TurnBlock, { kind: "approval" }> {
  return {
    kind: "approval",
    id: "approval-1",
    toolName: "run_shell_command",
    approvalKind: "command",
    title: "Allow mutating command?",
    summary: "git push origin main",
    reason: "This command may modify the workspace.",
    risk: "mutating",
    status: "pending",
    ...overrides,
  };
}

describe("ApprovalCard", () => {
  it("renders summary, reason, and Allow / Deny actions when pending", () => {
    render(
      <ol>
        <ApprovalCard block={pendingApproval()} onResolve={vi.fn().mockResolvedValue(undefined)} />
      </ol>
    );

    expect(screen.getByText("git push origin main")).toBeTruthy();
    expect(screen.getByText(/run_shell_command/)).toBeTruthy();
    expect(screen.getByText(/This command may modify the workspace/)).toBeTruthy();
    expect(screen.getByText("Allow")).toBeTruthy();
    expect(screen.getByText("Deny")).toBeTruthy();
    expect(screen.getByText("Awaiting your decision")).toBeTruthy();
  });

  it("invokes the resolver with the card id and decision", async () => {
    const onResolve = vi.fn().mockResolvedValue(undefined);
    render(
      <ol>
        <ApprovalCard block={pendingApproval()} onResolve={onResolve} />
      </ol>
    );

    fireEvent.click(screen.getByTestId("approval-allow-approval-1"));
    expect(onResolve).toHaveBeenCalledWith("approval-1", "approve");

    // After the first click, both buttons become busy/disabled until the
    // resolver settles. Reset the spy and mount a fresh card to test Deny.
    onResolve.mockClear();
    cleanup();
    render(
      <ol>
        <ApprovalCard block={pendingApproval()} onResolve={onResolve} />
      </ol>
    );
    fireEvent.click(screen.getByTestId("approval-deny-approval-1"));
    expect(onResolve).toHaveBeenCalledWith("approval-1", "deny");

    // Mount fresh card to test Always Allow
    onResolve.mockClear();
    cleanup();
    render(
      <ol>
        <ApprovalCard block={pendingApproval()} onResolve={onResolve} />
      </ol>
    );
    fireEvent.click(screen.getByTestId("approval-always-allow-approval-1"));
    expect(onResolve).toHaveBeenCalledWith("approval-1", "always_allow");
  });

  it("does not render Always Allow button for destructive commands", () => {
    render(
      <ol>
        <ApprovalCard
          block={pendingApproval({ risk: "destructive", id: "dest-1" })}
          onResolve={vi.fn().mockResolvedValue(undefined)}
        />
      </ol>
    );

    expect(screen.getByTestId("approval-allow-dest-1")).toBeTruthy();
    expect(screen.getByTestId("approval-deny-dest-1")).toBeTruthy();
    expect(screen.queryByTestId("approval-always-allow-dest-1")).toBeNull();
    expect(screen.getByText(/Guarded/)).toBeTruthy();
  });

  it("does not render Always Allow for external paths", () => {
    render(
      <ol>
        <ApprovalCard
          block={pendingApproval({
            approvalKind: "external_path",
            risk: "external_access",
            id: "external-1",
            summary: "/tmp/outside/My Notes.txt",
          })}
          onResolve={vi.fn().mockResolvedValue(undefined)}
        />
      </ol>
    );

    expect(screen.queryByTestId("approval-always-allow-external-1")).toBeNull();
  });

  it("does not render Always Allow for workspace-escaping commands", () => {
    render(
      <ol>
        <ApprovalCard
          block={pendingApproval({ id: "escape-1", workspaceScoped: false })}
          onResolve={vi.fn().mockResolvedValue(undefined)}
        />
      </ol>
    );

    expect(screen.queryByTestId("approval-always-allow-escape-1")).toBeNull();
  });

  it("renders auto_approved and auto_denied statuses", () => {
    const { rerender } = render(
      <ol>
        <ApprovalCard
          block={pendingApproval({ status: "auto_approved" })}
          onResolve={vi.fn().mockResolvedValue(undefined)}
        />
      </ol>
    );
    expect(screen.getByText("Auto-allowed")).toBeTruthy();

    rerender(
      <ol>
        <ApprovalCard
          block={pendingApproval({ status: "auto_denied" })}
          onResolve={vi.fn().mockResolvedValue(undefined)}
        />
      </ol>
    );
    expect(screen.getByText("Auto-rejected")).toBeTruthy();
  });

  it("hides the actions once the request is resolved and shows the outcome", () => {
    render(
      <ol>
        <ApprovalCard
          block={pendingApproval({ status: "approved" })}
          onResolve={vi.fn().mockResolvedValue(undefined)}
        />
      </ol>
    );

    expect(screen.queryByText("Allow")).toBeNull();
    expect(screen.queryByText("Deny")).toBeNull();
    expect(screen.getByText("Allowed")).toBeTruthy();
  });

  it("renders the destructive and external_access risk labels", () => {
    const { rerender } = render(
      <ol>
        <ApprovalCard
          block={pendingApproval({ risk: "destructive", id: "d-1" })}
          onResolve={vi.fn().mockResolvedValue(undefined)}
        />
      </ol>
    );
    expect(screen.getByText("Destructive")).toBeTruthy();

    rerender(
      <ol>
        <ApprovalCard
          block={pendingApproval({
            risk: "external_access",
            id: "e-1",
            title: "Allow external file access?",
            summary: "/etc/passwd",
            reason: "This file is outside the active workspace.",
          })}
          onResolve={vi.fn().mockResolvedValue(undefined)}
        />
      </ol>
    );
    expect(screen.getByText("External access")).toBeTruthy();
    expect(screen.getByText("/etc/passwd")).toBeTruthy();
  });

  it("renders without a resolver when onResolve is omitted", () => {
    render(
      <ol>
        <ApprovalCard block={pendingApproval()} />
      </ol>
    );
    // Buttons exist but are disabled so the user can't click into a no-op.
    const allow = screen.getByTestId("approval-allow-approval-1") as HTMLButtonElement;
    const deny = screen.getByTestId("approval-deny-approval-1") as HTMLButtonElement;
    expect(allow.disabled).toBe(true);
    expect(deny.disabled).toBe(true);
  });

  it("renders the entire long command without truncation and retains status visibility", () => {
    const longCommand =
      "python3 -c (__import__('shutil').copyfile('.gospel/report-stage.html','/tmp/citrapr-architecture-review-origin-dev.html')) --preserve-metadata --recursive --verbose --include-hidden-files";
    render(
      <ol>
        <ApprovalCard
          block={pendingApproval({
            summary: longCommand,
            status: "approved",
          })}
          onResolve={vi.fn().mockResolvedValue(undefined)}
        />
      </ol>
    );

    // Check the DOM content and the wrapping styles that keep it readable.
    const command = screen.getByTitle(longCommand);
    expect(command.classList.contains("break-words")).toBe(true);
    expect(command.classList.contains("[overflow-wrap:anywhere]")).toBe(true);
    expect(command.classList.contains("whitespace-pre-wrap")).toBe(true);
    expect(command.classList.contains("truncate")).toBe(false);
    expect(command.classList.contains("max-h-48")).toBe(true);
    // Status must remain visible on the card
    const status = screen.getByText("Allowed");
    expect(status.classList.contains("shrink-0")).toBe(true);
    expect(screen.getByText("Mutating")).toBeTruthy();
  });

  it("copies the command to clipboard when clicking the copy button", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });

    render(
      <ol>
        <ApprovalCard block={pendingApproval({ summary: "git push origin main" })} />
      </ol>
    );

    const copyBtn = screen.getByRole("button", { name: /copy command/i });
    expect(copyBtn).toBeTruthy();
    fireEvent.click(copyBtn);

    expect(writeText).toHaveBeenCalledWith("git push origin main");
    expect(await screen.findByRole("button", { name: /copied/i })).toBeTruthy();
  });

  it("toggles expand and collapse when the command is long", () => {
    const longCommand =
      "python3 -c \"import sys; print('This is a very long multiline script that should be expandable by clicking the expand button on the card for full visibility')\"\nline2\nline3";
    render(
      <ol>
        <ApprovalCard block={pendingApproval({ summary: longCommand })} />
      </ol>
    );

    const expandBtn = screen.getByRole("button", { name: /expand full command/i });
    const command = screen.getByTitle(/^python3 -c/);
    expect(command.classList.contains("max-h-48")).toBe(true);
    expect(expandBtn.getAttribute("aria-expanded")).toBe("false");

    fireEvent.click(expandBtn);
    expect(command.classList.contains("max-h-none")).toBe(true);
    expect(screen.getByRole("button", { name: /collapse command/i })).toBeTruthy();
    expect(expandBtn.getAttribute("aria-expanded")).toBe("true");

    fireEvent.click(expandBtn);
    expect(command.classList.contains("max-h-48")).toBe(true);
    expect(screen.getByRole("button", { name: /expand full command/i })).toBeTruthy();
    expect(expandBtn.getAttribute("aria-expanded")).toBe("false");
  });
});
