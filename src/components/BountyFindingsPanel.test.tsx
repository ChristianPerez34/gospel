import { invoke } from "@tauri-apps/api/core";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BountyFindingsPanel } from "./BountyFindingsPanel";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const findings = {
  runId: "run-1-delegated-bug-bounty-sweep",
  runs: ["run-1-delegated-bug-bounty-sweep"],
  checkpointPending: false,
  report: "# Sweep report\n\nOne confirmed issue.",
  signals: [
    {
      id: "s1",
      created_at: "t1",
      kind: "enforcement_gap",
      endpoint: "https://e.example/admin",
      method: "GET",
      summary: "admin panel reachable without auth",
      evidence_ids: ["e1", "e2"],
      confidence: "high",
      status: "candidate",
    },
    {
      id: "s2",
      created_at: "t2",
      kind: "other",
      endpoint: "https://e.example/ping",
      method: "GET",
      summary: "inconclusive lead",
      evidence_ids: [],
      confidence: "low",
      status: "candidate",
    },
  ],
  verdicts: [
    { id: "s1", status: "confirmed", severity: "high", rationale: "reproduced twice" },
    { id: "s2", status: "rejected", severity: "info", rationale: "expected behavior" },
  ],
};

describe("BountyFindingsPanel", () => {
  afterEach(() => {
    cleanup();
  });
  it("renders signals merged with verdicts and the run label", async () => {
    vi.mocked(invoke).mockResolvedValue(findings);
    render(<BountyFindingsPanel />);
    await waitFor(() =>
      expect(screen.getByText("admin panel reachable without auth")).toBeDefined()
    );
    expect(screen.getByText(/run-1-delegated-bug-bounty-sweep/)).toBeDefined();
    const confirmedRow = screen.getByText("admin panel reachable without auth").closest("li")!;
    const rejectedRow = screen.getByText("inconclusive lead").closest("li")!;
    expect(within(confirmedRow).getByText("confirmed", { selector: "span" })).toBeDefined();
    expect(within(rejectedRow).getByText("rejected", { selector: "span" })).toBeDefined();
    expect(screen.getByText("triage: reproduced twice")).toBeDefined();
    expect(screen.getByText("2 evidence")).toBeDefined();
    expect(screen.getByText("no evidence linked")).toBeDefined();
    expect(invoke).toHaveBeenCalledWith("list_bounty_findings", { runId: null });
  });

  it("records a user verdict and reloads", async () => {
    vi.mocked(invoke).mockResolvedValue(findings);
    render(<BountyFindingsPanel />);
    await waitFor(() =>
      expect(screen.getByText("admin panel reachable without auth")).toBeDefined()
    );
    vi.mocked(invoke).mockClear();
    const row = screen.getByText("inconclusive lead").closest("li")!;
    within(row).getByRole("button", { name: "Mark s2 confirmed" }).click();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("set_bounty_verdict", {
        runId: "run-1-delegated-bug-bounty-sweep",
        signalId: "s2",
        status: "confirmed",
      })
    );
  });

  it("shows approve button for a pending checkpoint and invokes approve", async () => {
    vi.mocked(invoke).mockResolvedValue({ ...findings, checkpointPending: true });
    render(<BountyFindingsPanel />);
    await waitFor(() => expect(screen.getByText(/checkpoint pending/i)).toBeDefined());
    vi.mocked(invoke).mockClear();
    screen.getByRole("button", { name: "Approve threat-model checkpoint" }).click();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("approve_bounty_checkpoint", {
        runId: "run-1-delegated-bug-bounty-sweep",
      })
    );
  });

  it("shows the empty state when no run exists", async () => {
    vi.mocked(invoke).mockResolvedValue({
      runId: null,
      runs: [],
      checkpointPending: false,
      signals: [],
      verdicts: [],
      report: null,
    });
    render(<BountyFindingsPanel />);
    await waitFor(() => expect(screen.getByText(/No signals yet/)).toBeDefined());
  });
});
