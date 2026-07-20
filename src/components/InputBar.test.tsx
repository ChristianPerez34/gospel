import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
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

function renderInputBar(overrides: Partial<ComponentProps<typeof InputBar>> = {}) {
  return render(
    <div data-testid="input-boundary">
      <InputBar
        models={models}
        selectedModel={modelOptionId("openai", "gpt-5.5")}
        selectedVariant={null}
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
});
