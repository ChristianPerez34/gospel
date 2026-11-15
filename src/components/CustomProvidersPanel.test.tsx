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

describe("CustomProvidersPanel", () => {
  it("adds a keyless provider with manual model IDs", async () => {
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
    fireEvent.change(screen.getByLabelText("Model IDs (optional)"), {
      target: { value: "llama-3\nother/model" },
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
    await waitFor(() => expect(refresh).toHaveBeenCalledWith(true));
  });

  it("preserves the saved provider when the credential update fails", async () => {
    const custom = {
      id: "custom_saved",
      name: "Gateway",
      baseUrl: "https://example.com/v1",
      models: [],
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

  it("edits, hides, and confirms deletion of a custom provider", async () => {
    const custom = {
      id: "custom_abc",
      name: "Gateway",
      baseUrl: "https://example.com/v1",
      models: [],
    };
    mockInvoke.mockImplementation(async (command) =>
      command === "list_custom_providers" ? [custom] : undefined
    );
    const availability: ProviderConfig = {
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
    };
    const refresh = vi.fn().mockResolvedValue(undefined);
    render(<CustomProvidersPanel providers={[availability]} onRefreshAvailability={refresh} />);
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
    expect(mockInvoke).not.toHaveBeenCalledWith("delete_custom_provider", expect.anything());
    fireEvent.click(screen.getByRole("button", { name: "Remove provider" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("delete_custom_provider", { id: custom.id })
    );
  });
});
