import { invoke } from "@tauri-apps/api/core";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { type ProviderConfig, ProviderSelector } from "./ProviderSelector";

const mockInvoke = vi.mocked(invoke);

const sampleProviders: ProviderConfig[] = [
  {
    id: "openai",
    name: "OpenAI",
    authType: "api_key",
    credentialed: false,
    visible: false,
    modelFetchStatus: "not_credentialed",
    modelCount: 0,
    apiKey: "",
    enabled: false,
    status: "idle",
    testMessage: "",
  },
  {
    id: "anthropic",
    name: "Anthropic",
    authType: "api_key",
    credentialed: true,
    visible: true,
    modelFetchStatus: "loaded",
    modelCount: 3,
    apiKey: "",
    enabled: true,
    status: "idle",
    testMessage: "",
  },
  {
    id: "gemini",
    name: "Gemini",
    authType: "api_key",
    credentialed: true,
    visible: false,
    modelFetchStatus: "loaded",
    modelCount: 2,
    apiKey: "",
    enabled: false,
    status: "idle",
    testMessage: "",
  },
];

describe("ProviderSelector", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("defaults uncredentialed provider toggle to OFF and disabled", () => {
    const onProvidersChange = vi.fn();
    const onRefreshAvailability = vi.fn();

    render(
      <ProviderSelector
        providers={sampleProviders}
        onProvidersChange={onProvidersChange}
        onRefreshAvailability={onRefreshAvailability}
      />
    );

    const openaiToggle = screen.getByRole("button", {
      name: "Add credentials to enable OpenAI",
    }) as HTMLButtonElement;

    expect(openaiToggle).toBeTruthy();
    expect(openaiToggle.disabled).toBe(true);
    expect(openaiToggle.getAttribute("aria-pressed")).toBe("false");
    expect(openaiToggle.className).toContain("opacity-40");
    expect(openaiToggle.className).toContain("cursor-not-allowed");

    fireEvent.click(openaiToggle);
    expect(onProvidersChange).not.toHaveBeenCalled();
    expect(mockInvoke).not.toHaveBeenCalledWith("set_provider_visibility", expect.anything());
  });

  it("coerces stale visible:true on uncredentialed providers to OFF and disabled", () => {
    const onProvidersChange = vi.fn();
    const onRefreshAvailability = vi.fn();

    render(
      <ProviderSelector
        providers={sampleProviders.map((p) => (p.id === "openai" ? { ...p, visible: true } : p))}
        onProvidersChange={onProvidersChange}
        onRefreshAvailability={onRefreshAvailability}
      />
    );

    const openaiToggle = screen.getByRole("button", {
      name: "Add credentials to enable OpenAI",
    }) as HTMLButtonElement;

    expect(openaiToggle.disabled).toBe(true);
    expect(openaiToggle.getAttribute("aria-pressed")).toBe("false");
  });

  it("defaults credentialed provider toggle to whatever has been set", () => {
    const onProvidersChange = vi.fn();
    const onRefreshAvailability = vi.fn();

    render(
      <ProviderSelector
        providers={sampleProviders}
        onProvidersChange={onProvidersChange}
        onRefreshAvailability={onRefreshAvailability}
      />
    );

    // Anthropic is credentialed and visible -> toggle ON & enabled
    const anthropicToggle = screen.getByRole("button", {
      name: "Hide Anthropic from model picker",
    }) as HTMLButtonElement;
    expect(anthropicToggle).toBeTruthy();
    expect(anthropicToggle.disabled).toBe(false);
    expect(anthropicToggle.getAttribute("aria-pressed")).toBe("true");

    // Gemini is credentialed but hidden -> toggle OFF & enabled
    const geminiToggle = screen.getByRole("button", {
      name: "Show Gemini in model picker",
    }) as HTMLButtonElement;
    expect(geminiToggle).toBeTruthy();
    expect(geminiToggle.disabled).toBe(false);
    expect(geminiToggle.getAttribute("aria-pressed")).toBe("false");
  });

  it("toggles visibility for credentialed providers and updates backend", async () => {
    const onProvidersChange = vi.fn();
    const onRefreshAvailability = vi.fn().mockResolvedValue(undefined);

    render(
      <ProviderSelector
        providers={sampleProviders}
        onProvidersChange={onProvidersChange}
        onRefreshAvailability={onRefreshAvailability}
      />
    );

    const anthropicToggle = screen.getByRole("button", {
      name: "Hide Anthropic from model picker",
    });
    fireEvent.click(anthropicToggle);

    await waitFor(() => {
      expect(onProvidersChange).toHaveBeenCalledWith(
        expect.arrayContaining([
          expect.objectContaining({
            id: "anthropic",
            visible: false,
            enabled: false,
          }),
        ])
      );
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("set_provider_visibility", {
        provider: "anthropic",
        visible: false,
      });
    });
  });

  it("turns toggle ON and enables it upon saving an API key", async () => {
    const onProvidersChange = vi.fn();
    const onRefreshAvailability = vi.fn().mockResolvedValue(undefined);

    const providersWithKey = sampleProviders.map((p) =>
      p.id === "openai" ? { ...p, apiKey: "sk-test-key-123" } : p
    );

    render(
      <ProviderSelector
        providers={providersWithKey}
        onProvidersChange={onProvidersChange}
        onRefreshAvailability={onRefreshAvailability}
      />
    );

    const saveButton = screen.getByRole("button", { name: "Save" });
    fireEvent.click(saveButton);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("set_api_key", {
        provider: "openai",
        apiKey: "sk-test-key-123",
      });
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("set_provider_visibility", {
        provider: "openai",
        visible: true,
      });
    });

    await waitFor(() => {
      expect(onProvidersChange).toHaveBeenCalledWith(
        expect.arrayContaining([
          expect.objectContaining({
            id: "openai",
            credentialed: true,
            visible: true,
            enabled: true,
            status: "success",
          }),
        ])
      );
    });
  });

  it("turns toggle OFF and disables it upon removing an API key", async () => {
    const onProvidersChange = vi.fn();
    const onRefreshAvailability = vi.fn().mockResolvedValue(undefined);

    render(
      <ProviderSelector
        providers={sampleProviders}
        onProvidersChange={onProvidersChange}
        onRefreshAvailability={onRefreshAvailability}
      />
    );

    const removeButton = screen.getByRole("button", { name: "Remove" });
    fireEvent.click(removeButton);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("delete_api_key", {
        provider: "anthropic",
      });
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("set_provider_visibility", {
        provider: "anthropic",
        visible: false,
      });
    });

    await waitFor(() => {
      expect(onProvidersChange).toHaveBeenCalledWith(
        expect.arrayContaining([
          expect.objectContaining({
            id: "anthropic",
            credentialed: false,
            visible: false,
            enabled: false,
            status: "idle",
          }),
        ])
      );
    });
  });
});
