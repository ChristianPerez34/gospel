import { describe, expect, it } from "vitest";
import {
  toolActivitiesToActionCards,
  toolActivitiesToTimelineSteps,
} from "./toolActivityCards";
import type { ActionCardSection } from "./types";

type RowsSection = Extract<ActionCardSection, { type: "rows" }>;

const isRowsSection = (section: ActionCardSection): section is RowsSection =>
  section.type === "rows";

describe("toolActivitiesToActionCards review tools", () => {
  it("marks calling and completed tool activity cards collapsed by default", () => {
    const cards = toolActivitiesToActionCards([
      {
        id: "tool-calling",
        name: "read_file",
        arguments: { path: "src/lib.rs" },
        status: "calling",
      },
      {
        id: "tool-completed",
        name: "read_file",
        arguments: { path: "src/main.rs" },
        result: JSON.stringify({ path: "src/main.rs", content: "fn main() {}" }),
        status: "completed",
      },
    ]);

    expect(cards.map((card) => card.expanded)).toEqual([false, false]);
    expect(cards.map((card) => card.status)).toEqual(["calling", "completed"]);
  });

  it("formats source_edit as an edit card with redacted raw arguments", () => {
    const cards = toolActivitiesToActionCards([
      {
        id: "tool-edit",
        name: "source_edit",
        arguments: {
          path: "src/lib.rs",
          old_text: "raw old snippet",
          new_text: "raw new snippet",
        },
        result: JSON.stringify({
          success: true,
          message: "Edited src/lib.rs with one exact replacement.",
          reason: null,
          path: "src/lib.rs",
          changed: true,
          replacements: 1,
          start_line: 10,
          end_line: 11,
          size_bytes: 1234,
          diff_preview: "@@ src/lib.rs:10 @@\n-old\n+new",
          truncated: false,
        }),
        status: "completed",
      },
    ]);

    expect(cards[0]?.summary).toBe("Edit file");
    expect(cards[0]?.type).toBe("diff");
    expect(cards[0]?.rawPayload).toContain("[REDACTED]");
    expect(cards[0]?.rawPayload).not.toContain("raw old snippet");
    expect(cards[0]?.rawPayload).not.toContain("raw new snippet");
    const diff = cards[0]?.sections?.find(
      (section) => section.type === "text" && section.title === "Diff",
    );
    expect(diff).toMatchObject({
      content: "@@ src/lib.rs:10 @@\n-old\n+new",
    });
  });

  it("formats run_review with indexed findings, focus, and SNR fields", () => {
    const cards = toolActivitiesToActionCards([
      {
        id: "tool-1",
        name: "run_review",
        arguments: { mode: "local", focus: "Security" },
        result: JSON.stringify({
          success: true,
          message: "Found 2 potential issues.",
          review: {
            run_id: "run-1",
            focus: "Security",
            comments: [
              {
                comment_id: "rc_1",
                file: "src/main.rs",
                line_start: 10,
                line_end: 10,
                severity: "High",
                category: "injection",
                focus: "Security",
                focus_subcategory: null,
                cwe_id: "CWE-78",
                cwe_name: "OS Command Injection",
                title: "Unsanitized command",
                description: "User input reaches a command.",
                rationale: "Commands need structured arguments.",
                evidence: "Command::new(input)",
                suggestion: "Use fixed command args.",
                verification_plan: "Add a payload test.",
                signal_tier: "tier_1",
              },
            ],
            summary: "Found 2 potential issues.",
            validated: true,
            warnings: [],
            files_scanned: 1,
            mode: { type: "local" },
            suppressed_count: 1,
            snr_percent: 50,
            user_visible: true,
          },
          findings: [
            {
              index: 1,
              comment_id: "rc_1",
              file: "src/main.rs",
              line_start: 10,
              severity: "High",
              category: "injection",
              focus: "Security",
              focus_subcategory: null,
              title: "Unsanitized command",
              signal_tier: "tier_1",
            },
          ],
        }),
        status: "completed",
      },
    ]);

    expect(cards[0]?.summary).toBe("Code review");
    const reviewFields = cards[0]?.sections?.find(
      (section) => section.type === "fields" && section.title === "Review",
    );
    expect(reviewFields).toMatchObject({
      fields: expect.arrayContaining([
        { label: "Focus", value: "Security" },
        { label: "Run", value: "run-1" },
        { label: "Suppressed", value: "1" },
        { label: "SNR", value: "50%" },
      ]),
    });
    const findings = cards[0]?.sections?.find(
      (section) => section.type === "rows" && section.title === "Findings",
    );
    expect(findings).toMatchObject({
      rows: [
        expect.objectContaining({
          primary: "[1] Unsanitized command",
          meta: "rc_1",
        }),
      ],
    });
  });

  it("formats record_review_outcome arguments and result", () => {
    const cards = toolActivitiesToActionCards([
      {
        id: "tool-2",
        name: "record_review_outcome",
        arguments: { run_id: "run-1", comment_id: "rc_1", outcome: "rejected" },
        result: JSON.stringify({
          success: true,
          message: "Recorded outcome.",
          run_id: "run-1",
          comment_id: "rc_1",
          outcome: "rejected",
        }),
        status: "completed",
      },
    ]);

    expect(cards[0]?.summary).toBe("Record review outcome");
    expect(cards[0]?.detail).toBe("rejected, rc_1");
  });

  it("formats the deprecated run_security_review alias with the review renderer", () => {
    const cards = toolActivitiesToActionCards([
      {
        id: "tool-alias",
        name: "run_security_review",
        arguments: { mode: "local" },
        result: JSON.stringify({
          success: true,
          message: "No security findings detected.",
          review: {
            run_id: "run-alias",
            focus: "Security",
            comments: [],
            summary: "No security findings detected.",
            validated: true,
            warnings: [],
            files_scanned: 1,
            mode: { type: "local" },
            suppressed_count: 0,
            snr_percent: 100,
            user_visible: true,
          },
          findings: [],
        }),
        status: "completed",
      },
    ]);

    expect(cards[0]?.summary).toBe("Security review");
    expect(cards[0]?.detail).toBe("Security, local, run-alias");
  });

  it("keeps Security focus on pending run_security_review alias cards", () => {
    const cards = toolActivitiesToActionCards([
      {
        id: "tool-alias-pending",
        name: "run_security_review",
        arguments: { mode: "local" },
        status: "calling",
      },
    ]);

    expect(cards[0]?.summary).toBe("Security review");
    expect(cards[0]?.detail).toBe("Security, local");
    const reviewFields = cards[0]?.sections?.find(
      (section) => section.type === "fields" && section.title === "Review",
    );
    expect(reviewFields).toMatchObject({
      fields: expect.arrayContaining([{ label: "Focus", value: "Security" }]),
    });
  });

  it("groups consecutive identical tool + target calls into one timeline step", () => {
    const steps = toolActivitiesToTimelineSteps([
      {
        id: "read-1",
        name: "read_file",
        arguments: { path: "src/main.rs", start_line: 1, end_line: 40 },
        result: JSON.stringify({ path: "src/main.rs", content: "a" }),
        status: "completed",
      },
      {
        id: "read-2",
        name: "read_file",
        arguments: { path: "src/main.rs", start_line: 41, end_line: 80 },
        result: JSON.stringify({ path: "src/main.rs", content: "b" }),
        status: "completed",
      },
      {
        id: "read-3",
        name: "read_file",
        arguments: { path: "src/lib.rs" },
        result: JSON.stringify({ path: "src/lib.rs", content: "c" }),
        status: "completed",
      },
    ]);

    expect(steps).toHaveLength(2);
    expect(steps[0]?.groupCount).toBe(2);
    expect(steps[0]?.id).toBe("read-1");
    expect(steps[0]?.target).toBe("src/main.rs");
    expect(steps[0]?.passes?.map((pass) => pass.id)).toEqual(["read-1", "read-2"]);
    expect(steps[1]?.groupCount).toBeUndefined();
    expect(steps[1]?.target).toBe("src/lib.rs");
  });

  it("does not group identical tools split by a different target", () => {
    const steps = toolActivitiesToTimelineSteps([
      {
        id: "read-a",
        name: "read_file",
        arguments: { path: "src/a.rs" },
        result: JSON.stringify({ path: "src/a.rs", content: "a" }),
        status: "completed",
      },
      {
        id: "read-b",
        name: "read_file",
        arguments: { path: "src/b.rs" },
        result: JSON.stringify({ path: "src/b.rs", content: "b" }),
        status: "completed",
      },
    ]);

    expect(steps).toHaveLength(2);
    expect(steps.every((step) => step.groupCount === undefined)).toBe(true);
  });

  it("marks a grouped step as calling when any merged pass is still running", () => {
    const steps = toolActivitiesToTimelineSteps([
      {
        id: "read-1",
        name: "read_file",
        arguments: { path: "src/main.rs" },
        result: JSON.stringify({ path: "src/main.rs", content: "a" }),
        status: "completed",
      },
      {
        id: "read-2",
        name: "read_file",
        arguments: { path: "src/main.rs" },
        status: "calling",
      },
    ]);

    expect(steps).toHaveLength(1);
    expect(steps[0]?.status).toBe("calling");
  });

  it("keeps a grouped step's id stable as new passes are appended", () => {
    const initial = toolActivitiesToTimelineSteps([
      {
        id: "read-1",
        name: "read_file",
        arguments: { path: "src/main.rs" },
        result: JSON.stringify({ path: "src/main.rs", content: "a" }),
        status: "completed",
      },
      {
        id: "read-2",
        name: "read_file",
        arguments: { path: "src/main.rs" },
        status: "calling",
      },
    ]);
    const extended = toolActivitiesToTimelineSteps([
      ...(initial[0]?.passes ?? []).map((pass) => ({
        id: pass.id,
        name: "read_file",
        arguments: { path: "src/main.rs" },
        result: pass.rawPayload,
        status: pass.status ?? "completed",
      })),
      {
        id: "read-3",
        name: "read_file",
        arguments: { path: "src/main.rs" },
        status: "calling",
      },
    ]);

    expect(initial).toHaveLength(1);
    expect(extended).toHaveLength(1);
    expect(extended[0]?.id).toBe(initial[0]?.id);
    expect(extended[0]?.id).toBe("read-1");
    expect(extended[0]?.groupCount).toBe(3);
  });

  it("formats delegate_exploration with parsed structured sections", () => {
    const cards = toolActivitiesToActionCards([
      {
        id: "tool-3",
        name: "delegate_exploration",
        arguments: {
          task: "Investigate startup flow",
          context: "Focus on auth boundary.",
        },
        result: JSON.stringify({
          success: true,
          message: "Exploration complete.",
          reason: null,
          truncated: false,
          report: "Report body",
          summary: "Found one risky bootstrap path.",
          key_files: ["src/main.rs", "src/auth.rs"],
          findings: ["Potential unauthenticated route", "Cookie not rotated"],
          constraints: ["No blocking for now."],
          suggested_next_reads: ["Run security review", "Verify token claims"],
          tools_used: ["read_file", "search_code", "list_directory"],
        }),
        status: "completed",
      },
    ]);

    expect(cards[0]?.summary).toBe("Delegate exploration");
    expect(cards[0]?.detail).toBe("Found one risky bootstrap path., Investigate startup flow");

    const keyFilesSection = cards[0]?.sections?.find((section): section is RowsSection =>
      isRowsSection(section) && section.title === "Key files",
    );
    expect(keyFilesSection?.rows?.map((row) => row.primary)).toEqual([
      "src/main.rs",
      "src/auth.rs",
    ]);

    const findingsSection = cards[0]?.sections?.find((section): section is RowsSection =>
      isRowsSection(section) && section.title === "Findings",
    );
    expect(findingsSection?.rows?.map((row) => row.primary)).toEqual([
      "Potential unauthenticated route",
      "Cookie not rotated",
    ]);

    const nextReadsSection = cards[0]?.sections?.find((section): section is RowsSection =>
      isRowsSection(section) && section.title === "Suggested next reads",
    );
    expect(nextReadsSection?.rows?.map((row) => row.primary)).toEqual([
      "Run security review",
      "Verify token claims",
    ]);

    const toolSection = cards[0]?.sections?.find((section): section is RowsSection =>
      isRowsSection(section) && section.title === "Tools used",
    );
    expect(toolSection?.rows?.map((row) => row.primary)).toEqual([
      "read_file",
      "search_code",
      "list_directory",
    ]);
  });
});
