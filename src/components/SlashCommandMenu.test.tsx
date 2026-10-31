import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SkillSummary } from "../hooks/useSkills";
import { SlashCommandMenu } from "./SlashCommandMenu";

function skill(name: string): SkillSummary {
  return {
    name,
    description: `${name} skill`,
    source: "Workspace",
    scripts: [],
    user_invocable: true,
  };
}

describe("SlashCommandMenu", () => {
  afterEach(() => {
    cleanup();
  });

  it("shows all matching skills instead of capping at five", () => {
    render(
      <SlashCommandMenu
        skills={[
          skill("ask-matt"),
          skill("codebase-design"),
          skill("diagnosing-bugs"),
          skill("domain-modeling"),
          skill("grill-me"),
          skill("handoff"),
        ]}
        filter=""
        visible
        onSelect={vi.fn()}
      />
    );

    expect(screen.getByText("/ask-matt")).toBeTruthy();
    expect(screen.getByText("/codebase-design")).toBeTruthy();
    expect(screen.getByText("/diagnosing-bugs")).toBeTruthy();
    expect(screen.getByText("/domain-modeling")).toBeTruthy();
    expect(screen.getByText("/grill-me")).toBeTruthy();
    expect(screen.getByText("/handoff")).toBeTruthy();
  });

  it("shows both prefix and substring/token matches like code-review and review-animations for /review", () => {
    render(
      <SlashCommandMenu
        skills={[skill("code-review"), skill("review-animations"), skill("tdd"), skill("handoff")]}
        filter="review"
        visible
        onSelect={vi.fn()}
      />
    );

    expect(screen.getByText("/code-review")).toBeTruthy();
    expect(screen.getByText("/review-animations")).toBeTruthy();
    expect(screen.queryByText("/tdd")).toBeNull();
    expect(screen.queryByText("/handoff")).toBeNull();
  });

  it("shows suggestion when typing a typo with no direct matches", () => {
    render(
      <SlashCommandMenu
        skills={[skill("code-review"), skill("review-animations"), skill("handoff")]}
        filter="handofx"
        visible
        onSelect={vi.fn()}
      />
    );

    expect(screen.getByText(/Did you mean:/)).toBeTruthy();
    expect(screen.getByText("/handoff")).toBeTruthy();
  });
});
