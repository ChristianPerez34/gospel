import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SkillOptPanel } from "./SkillOptPanel";

const adopt = vi.fn();
const optimize = vi.fn();

vi.mock("../hooks/useStagedSkills", () => ({
  useStagedSkills: () => ({
    stagedSkills: [
      {
        name: "tdd",
        staged: true,
        has_rejected: false,
        current_mean: 0.5,
        candidate_mean: 1.0,
        preview: "Always watch it fail.",
        diff: "+ Always watch it fail.",
      },
      { name: "diagnose", staged: false, has_rejected: true },
    ],
    loading: false,
    optimizing: false,
    error: "selection split is empty",
    harvest: { skill: "tdd", trainCount: 4, selectionCount: 1 },
    previewHarvest: vi.fn(),
    adopt,
    optimize,
    reload: vi.fn(),
  }),
}));

describe("SkillOptPanel", () => {
  afterEach(() => {
    cleanup();
  });

  it("adopts a staged skill without offering adopt for rejects-only entries", () => {
    render(<SkillOptPanel workspacePath="/tmp/gospel" provider="openai" model="gpt-5.5" />);

    fireEvent.click(screen.getByRole("button", { name: "Adopt tdd" }));
    expect(adopt).toHaveBeenCalledWith("tdd");
    expect(screen.queryByRole("button", { name: "Adopt diagnose" })).toBeNull();
    expect(screen.getByText("diagnose")).toBeTruthy();
    expect(screen.getByText(/0\.50 → 1\.00/)).toBeTruthy();
    expect(screen.getByText("+ Always watch it fail.")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("selection split is empty");
  });

  it("optimizes the named skill with the session model", () => {
    render(<SkillOptPanel workspacePath="/tmp/gospel" provider="openai" model="gpt-5.5" />);

    fireEvent.change(screen.getByLabelText("Skill to optimize"), {
      target: { value: "tdd" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Optimize tdd" }));

    expect(optimize).toHaveBeenCalledWith("tdd", {
      provider: "openai",
      model: "gpt-5.5",
      variant: null,
    });
  });
});
