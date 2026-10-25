import { useEffect, useRef } from "react";
import { nextSessionNeedingYou, type SessionActivityMap, stepSession } from "../sessionActivity";
import type { Session } from "../types";

export type SessionShortcut =
  | { kind: "index"; index: number }
  | { kind: "step"; step: 1 | -1 }
  | { kind: "needs-you" }
  | { kind: "new" };

export function isApplePlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  return /mac|iphone|ipad|ipod/i.test(navigator.platform || navigator.userAgent);
}

/** Display form of a shortcut, e.g. "Cmd+J" on macOS and "Ctrl+J" elsewhere. */
export function shortcutLabel(key: string, options: { shift?: boolean } = {}): string {
  const modifier = isApplePlatform() ? "Cmd" : "Ctrl";
  return `${modifier}+${options.shift ? "Shift+" : ""}${key}`;
}

/**
 * Cmd on macOS, Ctrl elsewhere. Ctrl on macOS is left alone because text
 * fields use Ctrl+N, Ctrl+J, and friends for emacs-style editing.
 */
export function readSessionShortcut(
  event: Pick<KeyboardEvent, "key" | "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">,
  apple = isApplePlatform()
): SessionShortcut | null {
  const primary = apple ? event.metaKey : event.ctrlKey;
  if (!primary || event.altKey) return null;

  if (event.shiftKey) {
    if (event.code === "BracketRight" || event.key === "]" || event.key === "}") {
      return { kind: "step", step: 1 };
    }
    if (event.code === "BracketLeft" || event.key === "[" || event.key === "{") {
      return { kind: "step", step: -1 };
    }
    return null;
  }

  const key = event.key.toLowerCase();
  if (key === "j") return { kind: "needs-you" };
  if (key === "n") return { kind: "new" };
  const digit = /^[1-9]$/.test(event.key) ? event.key : /^Digit([1-9])$/.exec(event.code)?.[1];
  return digit ? { kind: "index", index: Number(digit) - 1 } : null;
}

interface UseSessionShortcutsOptions {
  /** Sessions in the order the task rail shows them. */
  order: Session[];
  activity: SessionActivityMap;
  activeSessionId: string | null;
  /** True while a modal surface owns the keyboard. */
  paused: boolean;
  onSelect: (session: Session) => void;
  onNewSession: () => void;
}

/**
 * Global session switching: Cmd/Ctrl+1-9 opens the Nth session in rail order,
 * Cmd/Ctrl+Shift+] and [ step through sessions, Cmd/Ctrl+J opens the next
 * session that needs you, and Cmd/Ctrl+N starts a new session.
 */
export function useSessionShortcuts(options: UseSessionShortcutsOptions) {
  const optionsRef = useRef(options);
  optionsRef.current = options;

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      const { order, activity, activeSessionId, paused, onSelect, onNewSession } =
        optionsRef.current;
      if (paused) return;
      const shortcut = readSessionShortcut(event);
      if (!shortcut) return;

      event.preventDefault();
      if (shortcut.kind === "new") {
        onNewSession();
        return;
      }
      let target: Session | null;
      if (shortcut.kind === "index") target = order[shortcut.index] ?? null;
      else if (shortcut.kind === "step")
        target = stepSession(order, activeSessionId, shortcut.step);
      else target = nextSessionNeedingYou(order, activity, activeSessionId);
      if (target && target.id !== activeSessionId) onSelect(target);
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, []);
}
