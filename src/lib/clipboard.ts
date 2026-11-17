import { writeText as writeTextViaPlugin } from "@tauri-apps/plugin-clipboard-manager";

const PATCH_MARKER = "__gospelClipboardFallback";

type PatchedFn = ((text: string) => Promise<void>) & { [PATCH_MARKER]?: true };

/**
 * Legacy copy path for contexts where neither the async Clipboard API nor
 * the Tauri clipboard plugin can write (e.g. a plain browser preview).
 */
function legacyCopy(text: string): void {
  if (typeof document === "undefined" || typeof document.execCommand !== "function") {
    throw new Error("Copy failed");
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  textarea.setSelectionRange(0, textarea.value.length);
  const ok = document.execCommand("copy");
  document.body.removeChild(textarea);
  if (!ok) throw new Error("Copy failed");
}

/**
 * Shared write chain: async Clipboard API, then the Tauri
 * clipboard-manager plugin (the reliable path inside the app's WebView),
 * then the legacy textarea fallback.
 */
async function copyChain(
  nativeWrite: ((text: string) => Promise<void>) | undefined,
  text: string
): Promise<void> {
  if (nativeWrite) {
    try {
      await nativeWrite(text);
      return;
    } catch {
      // Fall through to the plugin below.
    }
  }
  try {
    await writeTextViaPlugin(text);
    return;
  } catch {
    // Fall through to the legacy path below.
  }
  legacyCopy(text);
}

/**
 * Copy text, trying each backend in turn. Prefer this over calling
 * `navigator.clipboard.writeText` directly.
 */
export async function copyToClipboard(text: string): Promise<void> {
  const native =
    typeof navigator !== "undefined"
      ? navigator.clipboard?.writeText?.bind(navigator.clipboard)
      : undefined;
  await copyChain(native, text);
}

/**
 * Patch `navigator.clipboard.writeText` with the same fallback chain so
 * third-party copy buttons that call it directly (Streamdown's code-block
 * copy button) keep working in the Tauri WebView. Idempotent; call once
 * at startup.
 */
export function installClipboardPolyfill(): void {
  if (typeof navigator === "undefined") return;
  const nav = navigator as Navigator & { clipboard?: Clipboard };
  if ((nav.clipboard?.writeText as PatchedFn | undefined)?.[PATCH_MARKER]) return;

  const original = nav.clipboard?.writeText?.bind(nav.clipboard);
  const patched = (async (text: string): Promise<void> => {
    await copyChain(original, text);
  }) as PatchedFn;
  patched[PATCH_MARKER] = true;

  try {
    if (nav.clipboard) {
      nav.clipboard.writeText = patched;
    } else {
      Object.defineProperty(navigator, "clipboard", {
        value: { writeText: patched },
        configurable: true,
      });
    }
  } catch {
    try {
      Object.defineProperty(navigator, "clipboard", {
        value: { writeText: patched },
        configurable: true,
      });
    } catch {
      // Leave the Clipboard API untouched; direct callers keep old behavior.
    }
  }
}
