import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CommandPalette } from "./CommandPalette";
import { modelOptionId, type Session, type Workspace } from "../types";

const now = new Date("2026-06-19T12:00:00Z");
const sessions: Session[] = [
  {
    id: "session-main",
    title: "Session in active workspace",
    provider: "openai",
    model: "gpt-4o",
    timestamp: now,
    messages: [],
    status: "idle",
    workspaceId: "workspace-main",
  },
  {
    id: "session-other",
    title: "Session in other workspace",
    provider: "openai",
    model: "gpt-4o",
    timestamp: new Date(now.getTime() - 60 * 60 * 1000),
    messages: [],
    status: "idle",
    workspaceId: "workspace-other",
  },
];

const workspace: Workspace = {
  id: "workspace-main",
  name: "Main",
  path: "/tmp/main",
  sessionCount: 2,
};

const models = [
  {
    id: modelOptionId("openai", "gpt-4o"),
    name: "gpt-4o",
    provider: "openai",
    model: "gpt-4o",
    configured: true,
    variants: [
      {
        id: "reasoning-high",
        name: "Reasoning High",
        description: "More reasoning",
      },
      {
        id: "legacy-hidden",
        name: "Legacy Hidden",
        description: "Deprecated variant",
        deprecated: true,
      },
    ],
  },
  {
    id: modelOptionId("anthropic", "claude-sonnet-4"),
    name: "claude-sonnet-4",
    provider: "anthropic",
    model: "claude-sonnet-4",
    configured: true,
  },
];

function renderPalette(overrides: Partial<ComponentProps<typeof CommandPalette>> = {}) {
  return render(
    <CommandPalette
      open
      sessions={sessions}
      activeSessionId="session-main"
      workspace={workspace}
      models={models}
      selectedModelId={modelOptionId("openai", "gpt-4o")}
      workspaceNames={{ "workspace-main": "Main", "workspace-other": "Other" }}
      onClose={vi.fn()}
      onSelectSession={vi.fn()}
      onNewSession={vi.fn()}
      onOpenSettings={vi.fn()}
      onOpenWorkspaceSwitcher={vi.fn()}
      onToggleSessions={vi.fn()}
      onSwitchToReview={vi.fn()}
      onSelectModel={vi.fn()}
      onVariantChange={vi.fn()}
      {...overrides}
    />,
  );
}

describe("CommandPalette", () => {
  afterEach(() => {
    cleanup();
  });

  it("shows workspace names in session search details", () => {
    renderPalette();

    expect(screen.getByText(/gpt-4o · Main/)).toBeTruthy();
    expect(screen.getByText(/gpt-4o · Other/)).toBeTruthy();
  });

  it("searches by workspace name across sessions", () => {
    renderPalette();
    fireEvent.change(screen.getByLabelText("Search commands"), {
      target: { value: "other" },
    });

    expect(screen.getByRole("button", { name: /Session in other workspace/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Session in active workspace/ })).toBeNull();
  });

  it("shows variants for the selected model without deprecated variants", () => {
    renderPalette();

    expect(screen.getByText("Variants")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Default/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Reasoning High/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Legacy Hidden/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /Use gpt-4o · Reasoning High/ })).toBeNull();
  });

  it("hides variants when the selected model has no variants", () => {
    renderPalette({ selectedModelId: modelOptionId("anthropic", "claude-sonnet-4") });

    expect(screen.queryByText("Variants")).toBeNull();
    expect(screen.queryByRole("button", { name: /Reasoning High/ })).toBeNull();
  });

  it("selects variant commands scoped to the current model", () => {
    const onVariantChange = vi.fn();
    renderPalette({ onVariantChange });

    fireEvent.click(screen.getByRole("button", { name: /Reasoning High/ }));
    expect(onVariantChange).toHaveBeenCalledWith("reasoning-high");

    cleanup();
    renderPalette({ onVariantChange, selectedVariant: "reasoning-high" });
    fireEvent.click(screen.getAllByRole("button", { name: /Default/ })[0]);
    expect(onVariantChange).toHaveBeenCalledWith(null);
  });
});
