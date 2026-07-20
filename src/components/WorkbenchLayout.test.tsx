import { invoke } from "@tauri-apps/api/core";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UseReviewProgress } from "../hooks/useReviewProgress";
import type { ReviewComment, ReviewResult } from "../types";
import { WorkbenchLayout } from "./WorkbenchLayout";

vi.mock("./ConstellationCanvas", () => ({
  ConstellationCanvas: () => <div data-testid="constellation-canvas" />,
}));

vi.mock("./ReviewerPanelCard", () => ({
  ReviewerPanelCard: () => <div data-testid="reviewer-card" />,
}));

const actionableComment: ReviewComment = {
  comment_id: "rc_1",
  file: "src/main.rs",
  line_start: 10,
  line_end: 12,
  severity: "High",
  category: "injection",
  focus: "Security",
  focus_subcategory: null,
  cwe_id: "CWE-78",
  cwe_name: "OS Command Injection",
  title: "Unsanitized command",
  description: "User input reaches a shell command.",
  rationale: "Shell execution expands attacker-controlled text.",
  evidence: 'Command::new("sh").arg(user_input)',
  suggestion: "Use fixed command arguments.",
  verification_plan: "Run a payload test.",
  signal_tier: "tier_1",
};

const reviewResult: ReviewResult = {
  run_id: "run-1",
  focus: "Security",
  comments: [actionableComment],
  summary: "Found one actionable finding.",
  validated: true,
  warnings: [],
  files_scanned: 2,
  mode: { type: "local" },
  suppressed_count: 0,
  snr_percent: 100,
  user_visible: true,
};

function makeReviewProgress(): UseReviewProgress {
  return {
    runId: null,
    done: false,
    failed: false,
    pipeline: {
      detector: { chunk: 0, totalChunks: 0, candidateCount: 0, status: "idle" },
      validator: "idle",
      finalize: "idle",
      done: false,
      failed: false,
      failureDetail: null,
      findings: 0,
      suppressed: 0,
    },
    perFocus: {},
    log: [],
    reset: vi.fn(),
  };
}

function workbenchProps(
  overrides: Partial<ComponentProps<typeof WorkbenchLayout>> = {}
): ComponentProps<typeof WorkbenchLayout> {
  return {
    messages: [],
    currentTurn: null,
    isStreaming: false,
    reviewProgress: makeReviewProgress(),
    reviewProvider: "openai",
    reviewModel: "gpt-5",
    workspacePath: "/workspace/gospel",
    canSendTurn: true,
    conversationSlot: <div>Conversation content</div>,
    ...overrides,
  };
}

describe("WorkbenchLayout", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "gospel_review") return reviewResult;
      if (command === "gospel_record_review_outcome") {
        return {
          success: true,
          message: "Recorded outcome.",
          run_id: "run-1",
          comment_id: "rc_1",
          outcome: "accepted",
          recorded_at: "2026-07-19T00:00:00Z",
        };
      }
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

  it("switches to the Reviewers tab when its tab button is clicked", async () => {
    render(<WorkbenchLayout {...workbenchProps()} />);

    expect(screen.getByText("Conversation content")).toBeDefined();
    expect(screen.queryByRole("button", { name: "Run" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /Reviewers/ }));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Run" })).toBeDefined();
    });
    expect(screen.queryByText("Conversation content")).toBeNull();
  });

  it("keeps completed findings and their review actions accessible", async () => {
    const onFixFinding = vi.fn().mockResolvedValue(undefined);
    const onSuccess = vi.fn();
    render(<WorkbenchLayout {...workbenchProps({ onFixFinding, onSuccess })} />);

    fireEvent.click(screen.getByRole("button", { name: /Reviewers/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Run" }));

    expect(await screen.findByText("Unsanitized command")).toBeDefined();
    expect(screen.getByText("Found one actionable finding.")).toBeDefined();
    expect(screen.getByText('Command::new("sh").arg(user_input)')).toBeDefined();
    expect(screen.getByRole("button", { name: "Copy finding 1 to agent" })).toBeDefined();
    expect(screen.getByRole("button", { name: "Fix issue 1" })).toBeDefined();
    expect(screen.getByRole("button", { name: "Accept finding 1" })).toBeDefined();
    expect(screen.getByRole("button", { name: "Reject finding 1" })).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: "Copy finding 1 to agent" }));
    await waitFor(() => {
      expect(navigator.clipboard.writeText).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(screen.getByRole("button", { name: "Accept finding 1" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("gospel_record_review_outcome", {
        runId: "run-1",
        commentId: "rc_1",
        outcome: "accepted",
      });
    });
    expect(onSuccess).toHaveBeenCalledWith("Finding accepted.");

    fireEvent.click(screen.getByRole("button", { name: "Fix issue 1" }));
    await waitFor(() => {
      expect(onFixFinding).toHaveBeenCalledTimes(1);
    });
    expect(onFixFinding.mock.calls[0]?.[0]).toContain("Start with file: src/main.rs");
    expect(await screen.findByText("Conversation content")).toBeDefined();
  });
});
