import { describe, expect, it } from "vitest";
import {
  buildExternalAgentFindingPrompt,
  buildGospelFixFindingPrompt,
  isActionableReviewFinding,
} from "./reviewPrompts";
import type { ReviewComment, ReviewResult } from "./types";

const comment: ReviewComment = {
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

const review: ReviewResult = {
  run_id: "run-1",
  comments: [comment],
  summary: "Found one issue.",
  validated: true,
  warnings: [],
  files_scanned: 1,
  mode: { type: "local" },
  suppressed_count: 4,
  snr_percent: 20,
  user_visible: true,
};

describe("review prompt builders", () => {
  it("builds a single-finding external prompt without Gospel tool instructions or run metrics", () => {
    const prompt = buildExternalAgentFindingPrompt({
      comment,
      review,
      index: 1,
      workspacePath: "/workspace/gospel",
    });

    expect(prompt).toContain("Finding ID: rc_1");
    expect(prompt).toContain("Tier: Tier 1");
    expect(prompt).toContain("Severity: High");
    expect(prompt).toContain("CWE: CWE-78 (OS Command Injection)");
    expect(prompt).toContain("Workspace path: /workspace/gospel");
    expect(prompt).toContain("Evidence:");
    expect(prompt).toContain("Suggested fix:");
    expect(prompt).toContain("Verification plan:");
    expect(prompt).not.toContain("run_security_review");
    expect(prompt).not.toContain("record_review_outcome");
    expect(prompt).not.toContain("SNR");
    expect(prompt).not.toContain("suppression");
    expect(prompt).not.toContain("20");
    expect(prompt).not.toContain("suppressed_count");
  });

  it("builds a Gospel fix prompt that starts at the finding file and forbids outcome recording", () => {
    const prompt = buildGospelFixFindingPrompt({
      comment,
      review,
      index: 1,
      workspacePath: "/workspace/gospel",
    });

    expect(prompt.startsWith("Start with file: src/main.rs")).toBe(true);
    expect(prompt).toContain("Review metadata:");
    expect(prompt).toContain("Do not call `record_review_outcome`");
    expect(prompt).toContain("rerun `run_security_review` in `local` mode");
  });

  it("does not suggest automatic security review reruns for full scans", () => {
    const prompt = buildGospelFixFindingPrompt({
      comment,
      review: { ...review, mode: { type: "full_scan" } },
      index: 1,
    });

    expect(prompt).not.toContain("run_security_review");
  });

  it("classifies only tier 1 and tier 2 findings as actionable", () => {
    expect(isActionableReviewFinding(comment)).toBe(true);
    expect(isActionableReviewFinding({ ...comment, signal_tier: "tier_2" })).toBe(true);
    expect(isActionableReviewFinding({ ...comment, signal_tier: "noise" })).toBe(false);
    expect(isActionableReviewFinding({ ...comment, signal_tier: "unclassified" })).toBe(false);
  });
});
