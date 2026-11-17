const PATCH_MARKER = "__gospelClipboardFallback";

type PatchedFn = ((text: string) => Promise<void>) & { [PATCH_MARKER]?: true };

/**
 * Legacy copy path for WebViews where the async Clipboard API is missing
 * or rejects (e.g. the Tauri WebView backing this app).
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
 * Copy text, falling back to a hidden-textarea `execCommand("copy")` when
 * the async Clipboard API is unavailable or rejects. Prefer this over
 * calling `navigator.clipboard.writeText` directly.
 */
export async function copyToClipboard(text: string): Promise<void> {
  try {
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return;
    }
  } catch {
    // Fall through to the legacy path below.
  }
  legacyCopy(text);
}

/**
 * Patch `navigator.clipboard.writeText` with the legacy fallback so
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
    if (original) {
      try {
        await original(text);
        return;
      } catch {
        // Fall through to the legacy path below.
      }
    }
    legacyCopy(text);
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
