import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { copyToClipboard, installClipboardPolyfill } from "./clipboard";

function setClipboard(writeText?: (text: string) => Promise<void>) {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: writeText ? { writeText: vi.fn(writeText) } : undefined,
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
    expect(execCommand).not.toHaveBeenCalled();
  });

  it("falls back to execCommand when writeText rejects (Tauri WebView)", async () => {
    setClipboard(async () => {
      throw new Error("Permission denied");
    });
    const execCommand = stubExecCommand(() => true);
    await copyToClipboard("fallback text");
    expect(execCommand).toHaveBeenCalledWith("copy");
  });

  it("falls back to execCommand when the Clipboard API is missing", async () => {
    setClipboard(undefined);
    const execCommand = stubExecCommand(() => true);
    await copyToClipboard("no api");
    expect(execCommand).toHaveBeenCalledWith("copy");
  });

  it("rejects when every copy path fails", async () => {
    setClipboard(async () => {
      throw new Error("Permission denied");
    });
    stubExecCommand(() => false);
    await expect(copyToClipboard("doomed")).rejects.toThrow();
  });
});

describe("installClipboardPolyfill", () => {
  it("rescues direct navigator.clipboard.writeText calls via the fallback", async () => {
    setClipboard(async () => {
      throw new Error("Permission denied");
    });
    const execCommand = stubExecCommand(() => true);
    installClipboardPolyfill();
    // Third-party buttons (Streamdown code blocks) call this directly.
    await navigator.clipboard.writeText("code snippet");
    expect(execCommand).toHaveBeenCalledWith("copy");
  });

  it("still delegates to the native API when it works", async () => {
    const native = vi.fn(async (_text: string) => {});
    setClipboard(native);
    const execCommand = stubExecCommand(() => true);
    installClipboardPolyfill();
    await navigator.clipboard.writeText("native path");
    expect(native).toHaveBeenCalledWith("native path");
    expect(execCommand).not.toHaveBeenCalled();
  });
});
