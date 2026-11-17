import { writeText as writeTextViaPlugin } from "@tauri-apps/plugin-clipboard-manager";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { copyToClipboard, installClipboardPolyfill } from "./clipboard";

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

function stubExecCommand(impl: () => boolean) {
  const spy = vi.fn(impl);
  Object.defineProperty(document, "execCommand", {
    configurable: true,
    writable: true,
    value: spy,
  });
  return spy;
}

beforeEach(() => {
  setClipboard(async () => {});
  pluginWriteText.mockRejectedValue(new Error("not in Tauri"));
});

afterEach(() => {
  // happy-dom has no execCommand; remove the stub so each test starts clean.
  if ("execCommand" in document) {
    delete (document as unknown as Record<string, unknown>).execCommand;
  }
});

describe("copyToClipboard", () => {
  it("uses the async Clipboard API when available", async () => {
    const execCommand = stubExecCommand(() => true);
    await copyToClipboard("hello");
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("hello");
    expect(pluginWriteText).not.toHaveBeenCalled();
    expect(execCommand).not.toHaveBeenCalled();
  });

  it("falls back to the Tauri plugin when writeText rejects (Tauri WebView)", async () => {
    setClipboard(async () => {
      throw new Error("Permission denied");
    });
    pluginWriteText.mockResolvedValue(undefined);
    const execCommand = stubExecCommand(() => true);
    await copyToClipboard("plugin text");
    expect(pluginWriteText).toHaveBeenCalledWith("plugin text");
    expect(execCommand).not.toHaveBeenCalled();
  });

  it("falls back to execCommand when both writeText and the plugin fail", async () => {
    setClipboard(async () => {
      throw new Error("Permission denied");
    });
    const execCommand = stubExecCommand(() => true);
    await copyToClipboard("fallback text");
    expect(execCommand).toHaveBeenCalledWith("copy");
  });

  it("rejects when every copy path fails", async () => {
    setClipboard(async () => {
      throw new Error("Permission denied");
    });
    stubExecCommand(() => false);
    await expect(copyToClipboard("doomed")).rejects.toThrow();
  });

  it("guarantees textarea removal when execCommand throws", async () => {
    setClipboard(async () => {
      throw new Error("Permission denied");
    });
    stubExecCommand(() => {
      throw new Error("execCommand disabled");
    });
    const initialChildren = document.body.children.length;
    await expect(copyToClipboard("textarea leak check")).rejects.toThrow("execCommand disabled");
    expect(document.body.children.length).toBe(initialChildren);
  });

  it("delegates directly to patched writeText without double-wrapping when polyfill is installed", async () => {
    setClipboard(async () => {
      throw new Error("Permission denied");
    });
    const execCommand = stubExecCommand(() => false);
    installClipboardPolyfill();
    await expect(copyToClipboard("doomed once")).rejects.toThrow();
    // Plugin and execCommand should each be attempted exactly once, not twice.
    expect(pluginWriteText).toHaveBeenCalledTimes(1);
    expect(execCommand).toHaveBeenCalledTimes(1);
  });
});

describe("installClipboardPolyfill", () => {
  it("rescues direct navigator.clipboard.writeText calls via the plugin", async () => {
    setClipboard(async () => {
      throw new Error("Permission denied");
    });
    pluginWriteText.mockResolvedValue(undefined);
    const execCommand = stubExecCommand(() => true);
    installClipboardPolyfill();
    // Third-party buttons (Streamdown code blocks) call this directly.
    await navigator.clipboard.writeText("code snippet");
    expect(pluginWriteText).toHaveBeenCalledWith("code snippet");
    expect(execCommand).not.toHaveBeenCalled();
  });

  it("still delegates to the native API when it works", async () => {
    const native = vi.fn(async (_text: string) => {});
    setClipboard(native);
    const execCommand = stubExecCommand(() => true);
    installClipboardPolyfill();
    await navigator.clipboard.writeText("native path");
    expect(native).toHaveBeenCalledWith("native path");
    expect(pluginWriteText).not.toHaveBeenCalled();
    expect(execCommand).not.toHaveBeenCalled();
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
