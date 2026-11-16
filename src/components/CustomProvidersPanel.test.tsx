import { invoke } from "@tauri-apps/api/core";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CustomProvidersPanel } from "./CustomProvidersPanel";
import type { ProviderConfig } from "./ProviderSelector";

const mockInvoke = vi.mocked(invoke);

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

function availabilityFor(custom: { id: string; name: string }): ProviderConfig {
  return {
    id: custom.id,
    name: custom.name,
    authType: "api_key",
    credentialed: true,
    visible: true,
    modelFetchStatus: "loaded",
    modelCount: 2,
    apiKey: "",
    enabled: true,
    status: "idle",
    testMessage: "",
    isCustom: true,
  };
}

describe("CustomProvidersPanel", () => {
  it("adds a keyless provider with comma-separated model IDs", async () => {
    mockInvoke.mockImplementation(async (command) =>
      command === "list_custom_providers" ? [] : undefined
    );
    const refresh = vi.fn().mockResolvedValue(undefined);
    render(<CustomProvidersPanel providers={[]} onRefreshAvailability={refresh} />);

    fireEvent.click(screen.getByRole("button", { name: "Add provider" }));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Local" } });
    fireEvent.change(screen.getByLabelText("Base URL"), {
      target: { value: "http://localhost:11434/v1" },
    });
    // Plain HTTP endpoints get an encryption warning.
    expect(screen.getByText(/without encryption/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Model IDs (optional)"), {
      target: { value: "llama-3, other/model" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("save_custom_provider", {
        id: null,
        name: "Local",
        baseUrl: "http://localhost:11434/v1",
        models: ["llama-3", "other/model"],
        apiKey: null,
        removeKey: false,
      })
    );
    // Saving must not force-refresh every built-in provider.
    await waitFor(() => expect(refresh).toHaveBeenCalledWith());
    expect(refresh).not.toHaveBeenCalledWith(true);
  });

  it("preserves the saved provider when the credential update fails", async () => {
    const custom = {
      id: "custom_saved",
      name: "Gateway",
      baseUrl: "https://example.com/v1",
      models: [],
      hasApiKey: false,
    };
    let saved = false;
    mockInvoke.mockImplementation(async (command) => {
      if (command === "list_custom_providers") return saved ? [custom] : [];
      if (command === "save_custom_provider") {
        saved = true;
        return {
          provider: custom,
          warning: "Provider saved, but its API key could not be updated",
        };
      }
      return undefined;
    });
    render(
      <CustomProvidersPanel
        providers={[]}
        onRefreshAvailability={vi.fn().mockResolvedValue(undefined)}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Add provider" }));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Gateway" } });
    fireEvent.change(screen.getByLabelText("Base URL"), { target: { value: custom.baseUrl } });
    fireEvent.change(screen.getByLabelText("API key (optional)"), { target: { value: "key" } });
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    expect((await screen.findByRole("alert")).textContent).toContain(
      "Provider saved, but its API key could not be updated"
    );
    expect(screen.queryByLabelText("API key (optional)")).toBeNull();
    expect(await screen.findByText("Gateway")).toBeTruthy();
  });

  it("edits, hides, and confirms deletion of a keyless custom provider", async () => {
    const custom = {
      id: "custom_abc",
      name: "Gateway",
      baseUrl: "https://example.com/v1",
      models: [],
      hasApiKey: false,
    };
    mockInvoke.mockImplementation(async (command) =>
      command === "list_custom_providers" ? [custom] : undefined
    );
    const refresh = vi.fn().mockResolvedValue(undefined);
    render(
      <CustomProvidersPanel providers={[availabilityFor(custom)]} onRefreshAvailability={refresh} />
    );
    await screen.findByText("Gateway");
    fireEvent.click(screen.getByRole("button", { name: "Hide Gateway from model picker" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("set_provider_visibility", {
        provider: custom.id,
        visible: false,
      })
    );
    fireEvent.click(screen.getByRole("button", { name: "Edit Gateway" }));
    expect((screen.getByLabelText("Base URL") as HTMLInputElement).disabled).toBe(true);
    // Without a saved key there is nothing to remove.
    expect(screen.queryByLabelText("Remove saved API key")).toBeNull();
    fireEvent.change(screen.getByLabelText("API key (optional)"), { target: { value: "new-key" } });
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith(
        "save_custom_provider",
        expect.objectContaining({ id: custom.id, apiKey: "new-key" })
      )
    );
    await waitFor(() => expect(screen.queryByLabelText("API key (optional)")).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Delete Gateway" }));
    // A keyless provider's confirmation does not mention a saved API key.
    expect(screen.getByText("Remove Gateway?")).toBeTruthy();
    expect(mockInvoke).not.toHaveBeenCalledWith("delete_custom_provider", expect.anything());
    fireEvent.click(screen.getByRole("button", { name: "Remove provider" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("delete_custom_provider", { id: custom.id })
    );
  });

  it("renames a provider, switches it back to discovery, and can remove its saved key", async () => {
    const custom = {
      id: "custom_keyed",
      name: "Gateway",
      baseUrl: "https://example.com/v1",
      models: ["manual-model"],
      hasApiKey: true,
    };
    mockInvoke.mockImplementation(async (command) =>
      command === "list_custom_providers" ? [custom] : undefined
    );
    render(
      <CustomProvidersPanel
        providers={[availabilityFor(custom)]}
        onRefreshAvailability={vi.fn().mockResolvedValue(undefined)}
      />
    );
    await screen.findByText("Gateway");
    fireEvent.click(screen.getByRole("button", { name: "Delete Gateway" }));
    // A provider with a saved key warns about it in the confirmation.
    expect(screen.getByText("Remove Gateway and its saved API key?")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    fireEvent.click(screen.getByRole("button", { name: "Edit Gateway" }));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Renamed" } });
    // Clearing the model list switches the provider back to endpoint discovery.
    fireEvent.change(screen.getByLabelText("Model IDs (optional)"), { target: { value: "" } });
    fireEvent.click(screen.getByLabelText("Remove saved API key"));
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("save_custom_provider", {
        id: custom.id,
        name: "Renamed",
        baseUrl: custom.baseUrl,
        models: [],
        apiKey: null,
        removeKey: true,
      })
    );
  });

  it("does not clear a draft for a different provider when removing a provider", async () => {
    const custom1 = {
      id: "custom_1",
      name: "Provider One",
      baseUrl: "https://one.example.com/v1",
      models: [],
      hasApiKey: false,
    };
    const custom2 = {
      id: "custom_2",
      name: "Provider Two",
      baseUrl: "https://two.example.com/v1",
      models: [],
      hasApiKey: false,
    };
    mockInvoke.mockImplementation(async (command) => {
      if (command === "list_custom_providers") return [custom1, custom2];
      return undefined;
    });

    render(
      <CustomProvidersPanel
        providers={[availabilityFor(custom1), availabilityFor(custom2)]}
        onRefreshAvailability={vi.fn().mockResolvedValue(undefined)}
      />
    );
    await screen.findByText("Provider One");
    // Start editing Provider One
    fireEvent.click(screen.getByRole("button", { name: "Edit Provider One" }));
    expect(screen.getByDisplayValue("Provider One")).toBeTruthy();

    // Now delete Provider Two
    fireEvent.click(screen.getByRole("button", { name: "Delete Provider Two" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove provider" }));

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("delete_custom_provider", { id: "custom_2" })
    );
    // The draft for Provider One should still be open!
    expect(screen.getByDisplayValue("Provider One")).toBeTruthy();
  });
});
