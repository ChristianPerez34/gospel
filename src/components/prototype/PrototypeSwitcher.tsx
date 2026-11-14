import { useEffect, useCallback } from "react";
import { ChevronLeft, ChevronRight, Layers } from "lucide-react";

export interface VariantOption {
  key: string;
  name: string;
}

interface PrototypeSwitcherProps {
  variants: VariantOption[];
  current: string;
  onChange: (key: string) => void;
}

export function PrototypeSwitcher({ variants, current, onChange }: PrototypeSwitcherProps) {
  // Hide in production
  if (import.meta.env.PROD) return null;

  const currentIndex = variants.findIndex((v) => v.key.toUpperCase() === current.toUpperCase());
  const safeIndex = currentIndex >= 0 ? currentIndex : 0;
  const currentVariant = variants[safeIndex] || variants[0];

  const handlePrev = useCallback(() => {
    const prevIndex = (safeIndex - 1 + variants.length) % variants.length;
    onChange(variants[prevIndex].key);
  }, [safeIndex, variants, onChange]);

  const handleNext = useCallback(() => {
    const nextIndex = (safeIndex + 1) % variants.length;
    onChange(variants[nextIndex].key);
  }, [safeIndex, variants, onChange]);

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      const activeEl = document.activeElement;
      if (
        activeEl instanceof HTMLInputElement ||
        activeEl instanceof HTMLTextAreaElement ||
        (activeEl instanceof HTMLElement && activeEl.isContentEditable)
      ) {
        return;
      }

      if (e.key === "ArrowLeft") {
        e.preventDefault();
        handlePrev();
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        handleNext();
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handlePrev, handleNext]);

  return (
    <div
      className="fixed bottom-5 left-1/2 -translate-x-1/2 z-[9999] flex items-center gap-1.5 px-3 py-1.5 bg-[#090a0f] text-white border border-[#3b3d4f] rounded-full shadow-[0_8px_30px_rgb(0_0_0/0.7)] backdrop-blur-md select-none font-sans text-xs"
      role="region"
      aria-label="UI Prototype Switcher"
    >
      <div className="flex items-center gap-1.5 pr-2 border-r border-[#2d2f3d] text-accent-structure">
        <Layers size={13} className="text-[#a594fd]" />
        <span className="font-semibold text-[10px] uppercase tracking-wider text-text-muted">
          PROTOTYPE
        </span>
      </div>

      <button
        type="button"
        onClick={handlePrev}
        aria-label="Previous variant"
        className="p-1 rounded-full text-text-muted hover:text-white hover:bg-[#20222e] transition-colors"
      >
        <ChevronLeft size={15} />
      </button>

      <div className="px-2 font-medium flex items-center gap-1.5">
        <span className="inline-flex items-center justify-center w-4 h-4 rounded bg-[#6e64cf] text-white text-[10px] font-bold">
          {currentVariant.key}
        </span>
        <span className="text-[#ededf0] whitespace-nowrap">{currentVariant.name}</span>
      </div>

      <button
        type="button"
        onClick={handleNext}
        aria-label="Next variant"
        className="p-1 rounded-full text-text-muted hover:text-white hover:bg-[#20222e] transition-colors"
      >
        <ChevronRight size={15} />
      </button>

      <div className="pl-1.5 text-[10px] text-text-muted font-mono hidden sm:inline-block">
        [← / →]
      </div>
    </div>
  );
}
