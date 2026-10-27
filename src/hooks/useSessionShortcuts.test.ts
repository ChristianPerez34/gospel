import { cleanup, fireEvent, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, type MockInstance, vi } from "vitest";
import type { SessionActivityMap } from "../sessionActivity";
import type { Session } from "../types";
import { readSessionShortcut, shortcutLabel, useSessionShortcuts } from "./useSessionShortcuts";

type ShortcutKey = Parameters<typeof readSessionShortcut>[0];

function key(init: Partial<ShortcutKey>): ShortcutKey {
  return {
    key: "",
    code: "",
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    ...init,
  };
}

function makeSession(id: string): Session {
  return {
    id,
    title: id,
    provider: "openai",
    model: "gpt-4o",
    timestamp: new Date("2026-01-01T10:00:00Z"),
    messages: [],
    status: "idle",
  };
}

describe("readSessionShortcut", () => {
  it("reads Cmd shortcuts on macOS", () => {
    expect(readSessionShortcut(key({ key: "1", code: "Digit1", metaKey: true }), true)).toEqual({
      kind: "index",
      index: 0,
    });
    expect(readSessionShortcut(key({ key: "9", code: "Digit9", metaKey: true }), true)).toEqual({
      kind: "index",
      index: 8,
    });
    expect(
      readSessionShortcut(
        key({ key: "}", code: "BracketRight", metaKey: true, shiftKey: true }),
        true
      )
    ).toEqual({ kind: "step", step: 1 });
    expect(
      readSessionShortcut(
        key({ key: "{", code: "BracketLeft", metaKey: true, shiftKey: true }),
        true
      )
    ).toEqual({ kind: "step", step: -1 });
    expect(readSessionShortcut(key({ key: "j", code: "KeyJ", metaKey: true }), true)).toEqual({
      kind: "needs-you",
    });
    expect(readSessionShortcut(key({ key: "n", code: "KeyN", metaKey: true }), true)).toEqual({
      kind: "new",
    });
  });

  it("leaves Ctrl alone on macOS so text fields keep their editing keys", () => {
    expect(readSessionShortcut(key({ key: "n", code: "KeyN", ctrlKey: true }), true)).toBeNull();
    expect(readSessionShortcut(key({ key: "1", code: "Digit1", ctrlKey: true }), true)).toBeNull();
  });

  it("reads Ctrl shortcuts on other platforms", () => {
    expect(readSessionShortcut(key({ key: "2", code: "Digit2", ctrlKey: true }), false)).toEqual({
      kind: "index",
      index: 1,
    });
    expect(readSessionShortcut(key({ key: "J", code: "KeyJ", ctrlKey: true }), false)).toEqual({
      kind: "needs-you",
    });
    expect(readSessionShortcut(key({ key: "2", code: "Digit2", metaKey: true }), false)).toBeNull();
  });

  it("falls back to the key code when the digit row types symbols", () => {
    expect(readSessionShortcut(key({ key: "&", code: "Digit1", metaKey: true }), true)).toEqual({
      kind: "index",
      index: 0,
    });
  });

  it("ignores other combinations", () => {
    expect(readSessionShortcut(key({ key: "1", code: "Digit1" }), true)).toBeNull();
    expect(readSessionShortcut(key({ key: "0", code: "Digit0", metaKey: true }), true)).toBeNull();
    expect(readSessionShortcut(key({ key: "k", code: "KeyK", metaKey: true }), true)).toBeNull();
    expect(
      readSessionShortcut(key({ key: "1", code: "Digit1", metaKey: true, altKey: true }), true)
    ).toBeNull();
    expect(
      readSessionShortcut(key({ key: "J", code: "KeyJ", metaKey: true, shiftKey: true }), true)
    ).toBeNull();
  });
});

describe("shortcutLabel", () => {
  it("names the platform's primary modifier", () => {
    const platform = vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
    try {
      expect(shortcutLabel("J")).toBe("Cmd+J");
      platform.mockReturnValue("Win32");
      expect(shortcutLabel("]", { shift: true })).toBe("Ctrl+Shift+]");
    } finally {
      platform.mockRestore();
    }
  });
});

describe("useSessionShortcuts", () => {
  const order = [makeSession("a"), makeSession("b"), makeSession("c")];
  let platform: MockInstance;

  beforeEach(() => {
    platform = vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
  });

  afterEach(() => {
    cleanup();
    platform.mockRestore();
  });

  function renderShortcuts(options: { activity?: SessionActivityMap; paused?: boolean } = {}) {
    const onSelect = vi.fn();
    const onNewSession = vi.fn();
    renderHook(() =>
      useSessionShortcuts({
        getOrder: () => order,
        activity: options.activity ?? {},
        activeSessionId: "a",
        paused: options.paused ?? false,
        onSelect,
        onNewSession,
      })
    );
    const selected = () => onSelect.mock.calls.map(([session]) => (session as Session).id);
    return { selected, onNewSession };
  }

  /** Returns false when a handler prevented the default action. */
  function press(init: Partial<ShortcutKey>, target: Document | HTMLElement = document) {
    return fireEvent.keyDown(target, { metaKey: true, ...init });
  }

  it("opens sessions by rail position and steps through them", () => {
    const { selected } = renderShortcuts();

    expect(press({ key: "3", code: "Digit3" })).toBe(false);
    press({ key: "}", code: "BracketRight", shiftKey: true });
    press({ key: "{", code: "BracketLeft", shiftKey: true });
    // The open session and a slot past the end do nothing.
    press({ key: "1", code: "Digit1" });
    press({ key: "5", code: "Digit5" });

    expect(selected()).toEqual(["c", "b", "c"]);
  });

  it("jumps to the next session that needs you and starts new sessions", () => {
    const { selected, onNewSession } = renderShortcuts({
      activity: { c: { state: "new-reply", needsYou: true } },
    });

    press({ key: "j", code: "KeyJ" });
    expect(press({ key: "n", code: "KeyN" })).toBe(false);

    expect(selected()).toEqual(["c"]);
    expect(onNewSession).toHaveBeenCalledTimes(1);
  });

  it("stands down while paused", () => {
    const { selected, onNewSession } = renderShortcuts({ paused: true });

    expect(press({ key: "2", code: "Digit2" })).toBe(true);
    press({ key: "n", code: "KeyN" });

    expect(selected()).toEqual([]);
    expect(onNewSession).not.toHaveBeenCalled();
  });

  it("leaves keys another handler already took", () => {
    const { selected } = renderShortcuts();
    const input = document.createElement("input");
    input.addEventListener("keydown", (event) => event.preventDefault());
    document.body.append(input);
    try {
      press({ key: "2", code: "Digit2" }, input);
    } finally {
      input.remove();
    }

    expect(selected()).toEqual([]);
  });
});
