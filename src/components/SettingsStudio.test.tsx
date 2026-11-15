import { invoke } from "@tauri-apps/api/core";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ProviderConfig } from "./ProviderSelector";
import { SettingsStudio } from "./SettingsStudio";

const mockInvoke = vi.mocked(invoke);

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

function provider(
  overrides: Partial<ProviderConfig> & Pick<ProviderConfig, "id" | "name">
): ProviderConfig {
  return {
    authType: "api_key",
    credentialed: true,
    visible: true,
    modelFetchStatus: "loaded",
    modelCount: 2,
    apiKey: "",
    enabled: true,
    status: "idle",
    testMessage: "",
    ...overrides,
  };
}

function renderStudio(providers: ProviderConfig[], onProvidersChange = vi.fn()) {
  render(
    <SettingsStudio
      onClose={vi.fn()}
      initialCategory="providers"
      providers={providers}
      onProvidersChange={onProvidersChange}
      onRefreshAvailability={vi.fn().mockResolvedValue(undefined)}
      isRefreshingModels={false}
      themePreference="dark"
      onThemePreferenceChange={vi.fn()}
    />
  );
  return onProvidersChange;
}

describe("SettingsStudio providers category", () => {
  it("lists a custom provider only in the custom panel, never in the built-in list", async () => {
    mockInvoke.mockImplementation(async (command) => {
      if (command === "list_skills") return [];
      if (command === "list_custom_providers")
        return [
          {
            id: "custom_abc",
            name: "Gateway",
            baseUrl: "https://example.com/v1",
            models: [],
            hasApiKey: false,
          },
        ];
      return undefined;
    });

    renderStudio([
      provider({ id: "openai", name: "OpenAI" }),
      provider({ id: "custom_abc", name: "Gateway", isCustom: true }),
    ]);

    // The built-in ProviderSelector and the custom panel both offer a
    // visibility toggle for the same provider. It must appear exactly once.
    // Wait for the custom panel's own row first so the count is not racy.
    await screen.findByRole("button", { name: "Edit Gateway" });
    expect(screen.getAllByRole("button", { name: "Hide Gateway from model picker" })).toHaveLength(
      1
    );
    expect(screen.getByText("Built-in providers")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Hide OpenAI from model picker" })).toBeTruthy();
  });

  it("keeps custom providers when a built-in provider is edited", async () => {
    mockInvoke.mockImplementation(async (command) => {
      if (command === "list_skills") return [];
      if (command === "list_custom_providers") return [];
      return undefined;
    });

    const onProvidersChange = renderStudio([
      provider({ id: "openai", name: "OpenAI" }),
      provider({ id: "custom_abc", name: "Gateway", isCustom: true }),
    ]);

    fireEvent.click(screen.getByRole("button", { name: "Hide OpenAI from model picker" }));

    await waitFor(() => expect(onProvidersChange).toHaveBeenCalled());
    const calls = onProvidersChange.mock.calls;
    const updated = calls[calls.length - 1][0] as ProviderConfig[];
    expect(updated.map((p) => p.id)).toEqual(["openai", "custom_abc"]);
    expect(updated.find((p) => p.id === "openai")?.visible).toBe(false);
  });
});
