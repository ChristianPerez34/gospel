import { writeText as writeTextViaPlugin } from "@tauri-apps/plugin-clipboard-manager";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { installClipboardPolyfill } from "./clipboard";

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn(),
}));

const pluginWriteText = vi.mocked(writeTextViaPlugin);

function setClipboard(writeText?: ((text: string) => Promise<void>) | Record<string, unknown>) {
  let value: unknown;
  if (typeof writeText === "function") {
    value = { writeText: vi.fn(writeText) };
  } else if (writeText !== undefined) {
    value = writeText;
  } else {
    value = undefined;
  }
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value,
  });
}

beforeEach(() => {
  setClipboard(async () => {});
  pluginWriteText.mockReset();
  pluginWriteText.mockRejectedValue(new Error("not in Tauri"));
});

describe("installClipboardPolyfill", () => {
  it("rescues direct navigator.clipboard.writeText calls via the plugin when native rejects", async () => {
    setClipboard(async () => {
      throw new Error("Permission denied");
    });
    pluginWriteText.mockResolvedValue(undefined);
    installClipboardPolyfill();

    await navigator.clipboard.writeText("code snippet");
    expect(pluginWriteText).toHaveBeenCalledWith("code snippet");
  });

  it("still delegates to the native API when it works", async () => {
    const native = vi.fn(async (_text: string) => {});
    setClipboard(native);
    installClipboardPolyfill();

    await navigator.clipboard.writeText("native path");
    expect(native).toHaveBeenCalledWith("native path");
    expect(pluginWriteText).not.toHaveBeenCalled();
  });

  it("defines navigator.clipboard if initially undefined", async () => {
    setClipboard(undefined);
    pluginWriteText.mockResolvedValue(undefined);
    installClipboardPolyfill();

    expect(navigator.clipboard).toBeDefined();
    expect(typeof navigator.clipboard.writeText).toBe("function");
    await navigator.clipboard.writeText("defined");
    expect(pluginWriteText).toHaveBeenCalledWith("defined");
  });

  it("preserves other clipboard members when writeText assignment throws", async () => {
    const mockReadText = vi.fn(async () => "existing text");
    const existingClipboard = {
      readText: mockReadText,
    };
    Object.defineProperty(existingClipboard, "writeText", {
      get() {
        return undefined;
      },
      set() {
        throw new TypeError("Cannot assign to read-only property 'writeText'");
      },
      configurable: true,
    });
    setClipboard(existingClipboard);
    pluginWriteText.mockResolvedValue(undefined);

    installClipboardPolyfill();

    expect((navigator.clipboard as unknown as { readText: typeof mockReadText }).readText).toBe(
      mockReadText
    );
    await navigator.clipboard.writeText("polyfilled");
    expect(pluginWriteText).toHaveBeenCalledWith("polyfilled");
  });

  it("is idempotent when called multiple times", async () => {
    const native = vi.fn(async (_text: string) => {});
    setClipboard(native);
    installClipboardPolyfill();
    const firstPatched = navigator.clipboard.writeText;
    installClipboardPolyfill();
    expect(navigator.clipboard.writeText).toBe(firstPatched);
  });
});
