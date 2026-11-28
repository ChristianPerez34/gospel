import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { linkSafety } from "./ExternalLinkDialog";

afterEach(() => {
  cleanup();
});

function renderDialog(
  props: Partial<{
    isOpen: boolean;
    url: string;
    onClose: () => void;
    onConfirm: () => void;
  }> = {}
) {
  const renderModal = linkSafety.renderModal!;
  const resolved = {
    isOpen: true,
    url: "https://example.com/docs",
    onClose: vi.fn(),
    onConfirm: vi.fn(),
    ...props,
  };
  const result = render(renderModal(resolved));
  return {
    ...result,
    ...resolved,
    rerenderDialog(next: Partial<typeof resolved>) {
      result.rerender(renderModal({ ...resolved, ...next }));
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("ExternalLinkDialog", () => {
  it("uses Gospel dialog backdrop and surface tokens", () => {
    renderDialog();
    const dialog = screen.getByRole("dialog");
    const backdrop = dialog.parentElement;

    expect(backdrop?.className).toContain("bg-scrim");
    expect(backdrop?.className).toContain("z-[var(--z-dialog)]");
    expect(backdrop?.className).not.toContain("z-50");
    expect(dialog.className).toContain("bg-surface-elevated");
    expect(dialog.className).toContain("shadow-[var(--shadow-dialog)]");
  });

  it("does not hand unsupported or relative links to the system opener", async () => {
    const { rerenderDialog, onClose } = renderDialog({ url: "file:///private/data" });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Open link" })));
    expect(screen.getByRole("alert").textContent).toContain("Unable to open link");
    expect(openUrl).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();

    rerenderDialog({ url: "/relative/path" });
    expect(screen.queryByRole("alert")).toBeNull();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Open link" })));
    expect(screen.getByRole("alert").textContent).toContain("Unable to open link");
    expect(openUrl).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("uses the browser's native opener outside Tauri", async () => {
    vi.mocked(isTauri).mockReturnValueOnce(false);
    const windowOpen = vi.spyOn(window, "open").mockReturnValue(null);
    const { onClose } = renderDialog();

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Open link" })));
    expect(windowOpen).toHaveBeenCalledWith("https://example.com/docs", "_blank", "noreferrer");
    expect(openUrl).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledTimes(1);
    windowOpen.mockRestore();
  });

  it("ignores a late rejection after the dialog is canceled and reopened", async () => {
    const pending = deferred<void>();
    vi.mocked(openUrl).mockReturnValueOnce(pending.promise);
    const { rerenderDialog, onClose } = renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "Open link" }));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);

    rerenderDialog({ isOpen: false });
    await act(async () => {
      pending.reject(new Error("Browser failed"));
      await Promise.resolve();
    });

    rerenderDialog({ isOpen: true });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("ignores a late resolution from an earlier canceled attempt after reopen", async () => {
    const pending = deferred<void>();
    vi.mocked(openUrl).mockReturnValueOnce(pending.promise);
    const { rerenderDialog, onClose } = renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "Open link" }));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalledTimes(1);

    rerenderDialog({ isOpen: false });
    rerenderDialog({ isOpen: true });

    await act(async () => {
      pending.resolve();
      await Promise.resolve();
    });

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("dialog")).not.toBeNull();
  });

  it("clears copied and error state when reopened or switched to another URL", async () => {
    const writeText = vi.fn().mockRejectedValueOnce(new Error("Clipboard denied"));
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    const { rerenderDialog } = renderDialog();

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Copy link" })));
    expect(screen.getByRole("alert").textContent).toContain("Unable to copy link");

    rerenderDialog({ isOpen: false });
    rerenderDialog({ isOpen: true });
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
