import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SlashCommandMenu } from "./SlashCommandMenu";
import type { SkillSummary } from "../hooks/useSkills";

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
});
