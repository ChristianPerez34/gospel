import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApprovalCard } from "./ApprovalCard";
import type { TurnBlock } from "../types";

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
});
