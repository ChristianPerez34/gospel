import { writeText as writeTextViaPlugin } from "@tauri-apps/plugin-clipboard-manager";

const PATCH_MARKER = "__gospelClipboardFallback";

type PatchedFn = ((text: string) => Promise<void>) & { [PATCH_MARKER]?: true };

/**
 * Patch `navigator.clipboard.writeText` so callers (including third-party
 * components like Streamdown's code-block copy button) fall back to the
 * Tauri clipboard plugin when the WebView's async Clipboard API rejects.
 */
export function installClipboardPolyfill(): void {
  if (typeof navigator === "undefined") return;
  const nav = navigator as Navigator & { clipboard?: Clipboard };
  if ((nav.clipboard?.writeText as PatchedFn | undefined)?.[PATCH_MARKER]) return;

  const original = nav.clipboard?.writeText?.bind(nav.clipboard);
  const patched = (async (text: string): Promise<void> => {
    if (original) {
      try {
        await original(text);
        return;
      } catch {
        // Fall through to plugin in Tauri WebView
      }
    }
    await writeTextViaPlugin(text);
  }) as PatchedFn;
  patched[PATCH_MARKER] = true;

  try {
    if (nav.clipboard) {
      try {
        nav.clipboard.writeText = patched;
      } catch {
        Object.defineProperty(nav.clipboard, "writeText", {
          value: patched,
          configurable: true,
          writable: true,
        });
      }
    } else {
      Object.defineProperty(navigator, "clipboard", {
        value: { writeText: patched },
        configurable: true,
        writable: true,
      });
    }
  } catch {
    // Leave the Clipboard API untouched; direct callers keep old behavior.
  }
}
