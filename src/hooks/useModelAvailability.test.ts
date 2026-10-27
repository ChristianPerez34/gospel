import { invoke } from "@tauri-apps/api/core";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  type AvailableModel,
  defaultStartupModel,
  type ModelAvailabilitySnapshot,
  useModelAvailability,
} from "./useModelAvailability";

function modelAvailability(credentialed: boolean): ModelAvailabilitySnapshot {
  return {
    providers: [
      {
        provider: "opencode_go",
        display_name: "OpenCode Go",
        auth_type: "api_key",
        credentialed,
        visible: credentialed,
        model_fetch_status: credentialed ? "loaded" : "not_credentialed",
        model_count: credentialed ? 1 : 0,
      },
    ],
    available_models: credentialed ? [{ provider: "opencode_go", model: "glm-5.3" }] : [],
    empty_reason: credentialed ? null : "no_credentialed_providers",
    warnings: [],
  };
}

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

describe("useModelAvailability", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());
  afterEach(() => cleanup());

  it("does not let an older startup snapshot undo a saved provider", async () => {
    const pending: Array<(snapshot: ModelAvailabilitySnapshot) => void> = [];
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_model_availability") {
        return new Promise<ModelAvailabilitySnapshot>((resolve) => pending.push(resolve));
      }
      return Promise.resolve(undefined);
    });

    const { result } = renderHook(() => useModelAvailability());
    await waitFor(() => expect(pending).toHaveLength(1));

    act(() => {
      result.current.setProviders([
        {
          id: "opencode_go",
          name: "OpenCode Go",
          authType: "api_key",
          credentialed: true,
          visible: true,
          modelFetchStatus: "loaded",
          modelCount: 1,
          apiKey: "",
          enabled: true,
          status: "success",
          testMessage: "Key saved",
        },
      ]);
      void result.current.refreshModelAvailability();
    });
    await waitFor(() => expect(pending).toHaveLength(2));

    await act(async () => pending[1](modelAvailability(true)));
    await waitFor(() => expect(result.current.providers[0]?.credentialed).toBe(true));

    await act(async () => pending[0](modelAvailability(false)));

    expect(result.current.providers[0]).toMatchObject({
      credentialed: true,
      visible: true,
      status: "success",
      testMessage: "Key saved",
    });
  });
});
