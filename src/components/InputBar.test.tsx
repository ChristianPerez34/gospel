import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ModelPreferences } from "../hooks/useModelPreferences";
import { modelOptionId } from "../types";
import { InputBar } from "./InputBar";

vi.mock("../hooks/useSkills", () => ({
  useSkills: () => ({ skills: [], reloadSkills: vi.fn() }),
}));

const models = [
  {
    id: modelOptionId("openai", "gpt-5.5"),
    name: "gpt-5.5",
    provider: "openai",
    model: "gpt-5.5",
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

function emptyModelPreferences(overrides: Partial<ModelPreferences> = {}): ModelPreferences {
  return {
    favorites: new Set(),
    hidden: new Set(),
    onToggleFavorite: vi.fn(),
    onToggleHidden: vi.fn(),
    ...overrides,
  };
}

function renderInputBar(overrides: Partial<ComponentProps<typeof InputBar>> = {}) {
  return render(
    <div data-testid="input-boundary">
      <InputBar
        models={models}
        selectedModel={modelOptionId("openai", "gpt-5.5")}
        selectedVariant={null}
        modelPreferences={emptyModelPreferences()}
        onModelChange={vi.fn()}
        onVariantChange={vi.fn()}
        onSend={vi.fn()}
        {...overrides}
      />
    </div>
  );
}

function domRect({
  left,
  top,
  width,
  height,
}: {
  left: number;
  top: number;
  width: number;
  height: number;
}) {
  return {
    left,
    top,
    right: left + width,
    bottom: top + height,
    width,
    height,
    x: left,
    y: top,
    toJSON: () => ({}),
  } as DOMRect;
}

describe("InputBar", () => {
  afterEach(() => {
    cleanup();
  });

  it("hides the variant picker for models without variants", () => {
    renderInputBar({ selectedModel: modelOptionId("anthropic", "claude-sonnet-4") });

    expect(screen.queryByRole("button", { name: "Select variant" })).toBeNull();
  });

  it("shows non-deprecated variants for the selected model", () => {
    renderInputBar();

    fireEvent.click(screen.getByRole("button", { name: "Select variant" }));

    expect(screen.getByRole("option", { name: /Default/ })).toBeTruthy();
    expect(screen.getByRole("option", { name: /Reasoning High/ })).toBeTruthy();
    expect(screen.queryByRole("option", { name: /Legacy Hidden/ })).toBeNull();
  });

  it("keeps a deprecated selected variant visible in the picker label", () => {
    renderInputBar({ selectedVariant: "legacy-hidden" });

    expect(screen.getByText("Legacy Hidden")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Select variant" }));

    expect(screen.queryByRole("option", { name: /Legacy Hidden/ })).toBeNull();
  });

  it("calls onVariantChange with a variant id and null for Default", () => {
    const onVariantChange = vi.fn();
    renderInputBar({ onVariantChange });

    fireEvent.click(screen.getByRole("button", { name: "Select variant" }));
    fireEvent.click(screen.getByRole("option", { name: /Reasoning High/ }));

    expect(onVariantChange).toHaveBeenCalledWith("reasoning-high");

    cleanup();
    renderInputBar({ selectedVariant: "reasoning-high", onVariantChange });

    fireEvent.click(screen.getByRole("button", { name: "Select variant" }));
    fireEvent.click(screen.getByRole("option", { name: /Default/ }));

    expect(onVariantChange).toHaveBeenCalledWith(null);
  });

  it("keeps the variant menu inside its visible overflow boundary", () => {
    renderInputBar();
    const boundary = screen.getByTestId("input-boundary");
    boundary.style.overflow = "hidden";
    vi.spyOn(boundary, "getBoundingClientRect").mockReturnValue(
      domRect({ left: 0, top: 100, width: 280, height: 500 })
    );

    const trigger = screen.getByRole("button", { name: "Select variant" });
    const anchor = trigger.parentElement as HTMLDivElement;
    vi.spyOn(anchor, "getBoundingClientRect").mockReturnValue(
      domRect({ left: 12, top: 500, width: 192, height: 40 })
    );

    fireEvent.click(trigger);

    const menu = screen.getByRole("listbox");
    expect(menu.style.left).toBe("-4px");
    expect(menu.style.width).toBe("264px");
    expect(menu.style.maxHeight).toBe("288px");
    expect(menu.style.bottom).toBe("calc(100% + 0.5rem)");
  });

  it("renders Stop control when isStreaming is true and calls onCancelStream on click", () => {
    const onCancelStream = vi.fn();
    renderInputBar({ isStreaming: true, onCancelStream });

    expect(screen.queryByRole("button", { name: "Send message" })).toBeNull();
    const stopButton = screen.getByRole("button", { name: "Stop streaming" });
    expect(stopButton).toBeTruthy();

    fireEvent.click(stopButton);
    expect(onCancelStream).toHaveBeenCalledTimes(1);
  });

  it("renders Send (not Stop) when not streaming", () => {
    const onCancelStream = vi.fn();
    renderInputBar({ isStreaming: false, onCancelStream });

    expect(screen.getByRole("button", { name: "Send message" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Stop streaming" })).toBeNull();
  });

  it("filters the model list as the user types", () => {
    renderInputBar();

    fireEvent.click(screen.getByRole("button", { name: "Select model" }));
    expect(screen.getAllByRole("option")).toHaveLength(2);

    fireEvent.change(screen.getByLabelText("Search models"), { target: { value: "sonnet" } });

    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(1);
    expect(options[0].textContent).toContain("claude-sonnet-4");
  });

  it("matches the filter against provider names and multiple tokens", () => {
    renderInputBar();
    fireEvent.click(screen.getByRole("button", { name: "Select model" }));

    fireEvent.change(screen.getByLabelText("Search models"), {
      target: { value: "anthropic sonnet" },
    });

    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(1);
    expect(options[0].textContent).toContain("claude-sonnet-4");
  });

  it("shows an empty-filter state when nothing matches", () => {
    renderInputBar();
    fireEvent.click(screen.getByRole("button", { name: "Select model" }));

    fireEvent.change(screen.getByLabelText("Search models"), {
      target: { value: "does-not-exist" },
    });

    const options = screen.queryAllByRole("option");
    expect(options).toHaveLength(1);
    expect(options[0].getAttribute("aria-disabled")).toBe("true");
    expect(screen.getByText("No matching models.")).toBeTruthy();
  });

  it("sorts favorite models to the top of the list", () => {
    renderInputBar({
      modelPreferences: emptyModelPreferences({
        favorites: new Set([modelOptionId("anthropic", "claude-sonnet-4")]),
      }),
    });

    fireEvent.click(screen.getByRole("button", { name: "Select model" }));

    const options = screen.getAllByRole("option");
    expect(options[0].textContent).toContain("claude-sonnet-4");
    expect(options[1].textContent).toContain("gpt-5.5");
  });

  it("calls onToggleFavorite when the star is clicked without closing the menu", () => {
    const onToggleFavorite = vi.fn();
    const onModelChange = vi.fn();
    renderInputBar({
      modelPreferences: emptyModelPreferences({ onToggleFavorite }),
      onModelChange,
    });

    fireEvent.click(screen.getByRole("button", { name: "Select model" }));
    fireEvent.click(screen.getByRole("button", { name: "Add claude-sonnet-4 to favorites" }));

    expect(onToggleFavorite).toHaveBeenCalledWith(modelOptionId("anthropic", "claude-sonnet-4"));
    expect(onModelChange).not.toHaveBeenCalled();
    expect(screen.getByRole("listbox")).toBeTruthy();
  });

  it("moves hidden models into a collapsible hidden section", () => {
    const onToggleHidden = vi.fn();
    renderInputBar({
      modelPreferences: emptyModelPreferences({
        hidden: new Set([modelOptionId("openai", "gpt-5.5")]),
        onToggleHidden,
      }),
    });

    fireEvent.click(screen.getByRole("button", { name: "Select model" }));

    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(1);
    expect(options[0].textContent).toContain("claude-sonnet-4");
    expect(screen.queryByText("gpt-5.5")).toBeTruthy(); // still the trigger label

    fireEvent.click(screen.getByRole("button", { name: /Show 1 hidden model/ }));
    fireEvent.click(screen.getByRole("button", { name: "Show gpt-5.5 in picker" }));

    expect(onToggleHidden).toHaveBeenCalledWith(modelOptionId("openai", "gpt-5.5"));
  });

  it("calls onToggleHidden when the eye is clicked without selecting", () => {
    const onToggleHidden = vi.fn();
    const onModelChange = vi.fn();
    renderInputBar({
      modelPreferences: emptyModelPreferences({ onToggleHidden }),
      onModelChange,
    });

    fireEvent.click(screen.getByRole("button", { name: "Select model" }));
    fireEvent.click(screen.getByRole("button", { name: "Hide claude-sonnet-4 from picker" }));

    expect(onToggleHidden).toHaveBeenCalledWith(modelOptionId("anthropic", "claude-sonnet-4"));
    expect(onModelChange).not.toHaveBeenCalled();
    expect(screen.getByRole("listbox")).toBeTruthy();
  });

  it("selects the first filtered model with Enter", () => {
    const onModelChange = vi.fn();
    renderInputBar({ onModelChange });

    fireEvent.click(screen.getByRole("button", { name: "Select model" }));
    const search = screen.getByLabelText("Search models");
    fireEvent.change(search, { target: { value: "sonnet" } });
    fireEvent.keyDown(search, { key: "Enter" });

    expect(onModelChange).toHaveBeenCalledWith(modelOptionId("anthropic", "claude-sonnet-4"));
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("moves the highlight with arrow keys before Enter selects", () => {
    const onModelChange = vi.fn();
    renderInputBar({ onModelChange });

    fireEvent.click(screen.getByRole("button", { name: "Select model" }));
    const search = screen.getByLabelText("Search models");
    fireEvent.keyDown(search, { key: "ArrowDown" });
    fireEvent.keyDown(search, { key: "Enter" });

    expect(onModelChange).toHaveBeenCalledWith(modelOptionId("anthropic", "claude-sonnet-4"));
  });
});
