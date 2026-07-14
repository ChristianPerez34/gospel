import { describe, expect, it } from "vitest";
import { type AvailableModel, defaultStartupModel } from "./useModelAvailability";

describe("defaultStartupModel", () => {
  it("prefers the ChatGPT subscription gpt-5.6-sol model on startup", () => {
    const models: AvailableModel[] = [
      { provider: "openai", model: "gpt-5.5" },
      { provider: "chatgpt", model: "gpt-5.3-codex-spark" },
      { provider: "chatgpt", model: "gpt-5.6-sol" },
    ];

    expect(defaultStartupModel(models)).toEqual({
      provider: "chatgpt",
      model: "gpt-5.6-sol",
    });
  });

  it("falls back to the first available model when gpt-5.6-sol is unavailable", () => {
    const models: AvailableModel[] = [
      { provider: "anthropic", model: "claude-sonnet-4-5" },
      { provider: "openai", model: "gpt-5.5" },
    ];

    expect(defaultStartupModel(models)).toBe(models[0]);
  });

  it("returns undefined when no models are available", () => {
    expect(defaultStartupModel([])).toBeUndefined();
  });
});
