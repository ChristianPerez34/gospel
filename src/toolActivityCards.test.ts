import { describe, expect, it } from "vitest";
import { toolActivitiesToActionCards } from "./toolActivityCards";

describe("toolActivitiesToActionCards review tools", () => {
  it("formats run_security_review with indexed findings and SNR fields", () => {
    const cards = toolActivitiesToActionCards([
      {
        id: "tool-1",
        name: "run_security_review",
        arguments: { mode: "local" },
        result: JSON.stringify({
          success: true,
          message: "Found 2 potential issues.",
          review: {
            run_id: "run-1",
            comments: [
              {
                comment_id: "rc_1",
                file: "src/main.rs",
                line_start: 10,
                line_end: 10,
                severity: "High",
                category: "injection",
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
              title: "Unsanitized command",
              signal_tier: "tier_1",
            },
          ],
        }),
        status: "completed",
      },
    ]);

    expect(cards[0]?.summary).toBe("Security review");
    const reviewFields = cards[0]?.sections?.find(
      (section) => section.type === "fields" && section.title === "Review",
    );
    expect(reviewFields).toMatchObject({
      fields: expect.arrayContaining([
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
});
