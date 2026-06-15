import { useEffect, useRef, type RefObject } from "react";

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "summary",
  "[tabindex]:not([tabindex='-1'])",
  "[contenteditable]",
].join(",");

function focusableElements(container: HTMLElement) {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter((element) => {
    if (element.getAttribute("aria-hidden") === "true") return false;
    if (element.hasAttribute("disabled")) return false;
    const style = window.getComputedStyle(element);
    if (style.display === "none" || style.visibility === "hidden") return false;
    return true;
  });
}

interface UseFocusTrapOptions {
  active: boolean;
  containerRef: RefObject<HTMLElement>;
  onEscape: () => void;
  restoreFocusRef?: RefObject<HTMLElement>;
  initialFocusRef?: RefObject<HTMLElement>;
  restoreFocusOnDeactivate?: boolean;
}

export function useFocusTrap({
  active,
  containerRef,
  onEscape,
  restoreFocusRef,
  initialFocusRef,
  restoreFocusOnDeactivate = true,
}: UseFocusTrapOptions) {
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);
  const restoreOnDeactivateRef = useRef(restoreFocusOnDeactivate);
  restoreOnDeactivateRef.current = restoreFocusOnDeactivate;

  useEffect(() => {
    if (!active) return;

    previouslyFocusedRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;

    const focusInitialElement = () => {
      const container = containerRef.current;
      if (!container) return;

      const target = initialFocusRef?.current ?? focusableElements(container)[0] ?? container;
      target.focus();
    };

    const frame = window.requestAnimationFrame(focusInitialElement);

    const handleKeyDown = (event: KeyboardEvent) => {
      const container = containerRef.current;
      if (!container) return;

      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onEscape();
        return;
      }

      if (event.key !== "Tab") return;

      const elements = focusableElements(container);
      if (elements.length === 0) {
        event.preventDefault();
        container.focus();
        return;
      }

      const first = elements[0]!;
      const last = elements[elements.length - 1]!;
      const activeElement = document.activeElement;

      if (!container.contains(activeElement)) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
        return;
      }

      if (event.shiftKey && activeElement === first) {
        event.preventDefault();
        last.focus();
        return;
      }

      if (!event.shiftKey && activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", handleKeyDown, true);

    return () => {
      window.cancelAnimationFrame(frame);
      document.removeEventListener("keydown", handleKeyDown, true);

      if (!restoreOnDeactivateRef.current) return;

      const restoreTarget = restoreFocusRef?.current ?? previouslyFocusedRef.current;
      restoreTarget?.focus();
    };
  }, [active, containerRef, initialFocusRef, onEscape, restoreFocusRef]);
}
