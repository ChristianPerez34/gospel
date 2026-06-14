import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ReviewPanel } from "./ReviewPanel";
import type { ReviewComment, ReviewResult } from "../types";

const actionableComment: ReviewComment = {
  comment_id: "rc_1",
  file: "src/main.rs",
  line_start: 10,
  line_end: 12,
  severity: "High",
  category: "injection",
  cwe_id: "CWE-78",
  cwe_name: "OS Command Injection",
  title: "Unsanitized command",
  description: "User input reaches a shell command.",
  rationale: "Shell execution expands attacker-controlled text.",
  evidence: "Command::new(\"sh\").arg(user_input)",
  suggestion: "Use fixed command arguments.",
  verification_plan: "Run a payload test.",
  signal_tier: "tier_1",
};

const noiseComment: ReviewComment = {
  ...actionableComment,
  comment_id: "rc_2",
  title: "Informational finding",
  severity: "Info",
  signal_tier: "noise",
};

const reviewResult: ReviewResult = {
  run_id: "run-1",
  comments: [actionableComment, noiseComment],
  summary: "Found two findings.",
  validated: true,
  warnings: [],
  files_scanned: 2,
  mode: { type: "local" },
  suppressed_count: 0,
  snr_percent: 50,
  user_visible: true,
};

function renderOpenPanel(overrides: Partial<ComponentProps<typeof ReviewPanel>> = {}) {
  return render(
    <ReviewPanel
      open
      provider="openai"
      model="gpt-4o"
      workspacePath="/workspace/gospel"
      canSendTurn
      onClose={vi.fn()}
      {...overrides}
    />,
  );
}

async function runReview() {
  fireEvent.click(screen.getByRole("button", { name: "Run" }));
  await screen.findByText("Unsanitized command");
}

describe("ReviewPanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "gospel_review") return reviewResult;
      return undefined;
    });
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: vi.fn().mockResolvedValue(undefined),
      },
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("copies every visible finding to a single-finding external-agent prompt", async () => {
    const onSuccess = vi.fn();
    renderOpenPanel({ canSendTurn: false, onSuccess });

    await runReview();

    const copyButtons = screen.getAllByRole("button", { name: /copy finding .* to agent/i });
    expect(copyButtons).toHaveLength(2);
    expect((copyButtons[0] as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(copyButtons[0]!);

    await waitFor(() => {
      expect(navigator.clipboard.writeText).toHaveBeenCalledTimes(1);
    });
    const prompt = vi.mocked(navigator.clipboard.writeText).mock.calls[0]![0];
    expect(prompt).toContain("Finding ID: rc_1");
    expect(prompt).toContain("Tier: Tier 1");
    expect(prompt).toContain("Workspace path: /workspace/gospel");
    expect(prompt).not.toContain("run_security_review");
    expect(onSuccess).toHaveBeenCalledWith("Finding prompt copied.");
  });

  it("shows Fix issue only for actionable findings and starts a normal chat turn", async () => {
    const onClose = vi.fn();
    const onFixFinding = vi.fn();
    renderOpenPanel({ onClose, onFixFinding });

    await runReview();

    const fixButtons = screen.getAllByRole("button", { name: /fix issue/i });
    expect(fixButtons).toHaveLength(1);

    fireEvent.click(fixButtons[0]!);

    await waitFor(() => {
      expect(onFixFinding).toHaveBeenCalledTimes(1);
    });
    expect(onClose).toHaveBeenCalledTimes(1);
    const prompt = onFixFinding.mock.calls[0]![0];
    expect(prompt.startsWith("Start with file: src/main.rs")).toBe(true);
    expect(prompt).toContain("Do not call `record_review_outcome`");
    expect(prompt).toContain("run_security_review");
  });

  it("disables Fix issue when Gospel cannot send a turn but leaves Copy to agent enabled", async () => {
    renderOpenPanel({ canSendTurn: false, onFixFinding: vi.fn() });

    await runReview();

    const fixButton = screen.getByRole("button", { name: /fix issue/i }) as HTMLButtonElement;
    const copyButton = screen.getAllByRole("button", { name: /copy finding .* to agent/i })[0] as HTMLButtonElement;

    expect(fixButton.disabled).toBe(true);
    expect(copyButton.disabled).toBe(false);
  });
});
