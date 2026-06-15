import { useState, useEffect, useCallback, useRef } from "react";
import type { ProviderConfig } from "./ProviderSelector";
import { ProviderSelector } from "./ProviderSelector";
import type { ThemePreference } from "../types";

function themeIndex(value: ThemePreference) {
  switch (value) {
    case "dark":
      return 0;
    case "light":
      return 1;
    case "system":
      return 2;
  }
}

interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
  providers: ProviderConfig[];
  onProvidersChange: (providers: ProviderConfig[]) => void;
  onRefreshAvailability: (forceRefresh?: boolean) => Promise<void>;
  isRefreshingModels: boolean;
  initialTab?: TabKey;
  themePreference: ThemePreference;
  onThemePreferenceChange: (theme: ThemePreference) => void;
}

type TabKey = "general" | "models";

const THEME_OPTIONS: Array<{ value: ThemePreference; label: string; detail: string }> = [
  { value: "dark", label: "Dark", detail: "Default focus theme" },
  { value: "light", label: "Light", detail: "Bright surfaces" },
  { value: "system", label: "System", detail: "Follow OS" },
];

export function SettingsModal({
  open,
  onClose,
  providers,
  onProvidersChange,
  onRefreshAvailability,
  isRefreshingModels,
  initialTab = "models",
  themePreference,
  onThemePreferenceChange,
}: SettingsModalProps) {
  const [activeTab, setActiveTab] = useState<TabKey>(initialTab);
  const themeOptionRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const activeThemeIndex = themeIndex(themePreference);

  useEffect(() => {
    if (!open) return;
    setActiveTab(initialTab);
  }, [initialTab, open]);

  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (e.key === "Escape" && open) {
      onClose();
    }
  }, [open, onClose]);

  useEffect(() => {
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 bg-scrim z-[var(--z-dialog)] flex items-center justify-center animate-fade-in" onClick={onClose}>
      <div
        className="w-[520px] max-w-[90vw] max-h-[80vh] flex flex-col bg-surface-elevated border border-surface-overlay rounded-lg shadow-[var(--shadow-dialog)] animate-slide-up"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-modal-title"
      >
        <div className="flex items-center justify-between px-5 py-4 border-b border-surface-overlay shrink-0">
          <h2 className="font-sans text-base font-semibold text-text-primary m-0" id="settings-modal-title">Settings</h2>
          <button
            className="hit-target text-text-muted w-8 h-8 rounded-sm flex items-center justify-center transition-colors duration-150 hover:text-text-primary hover:bg-surface-overlay"
            onClick={onClose}
            aria-label="Close settings"
            type="button"
          >
            <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
              <path d="M4.5 4.5L11.5 11.5M11.5 4.5L4.5 11.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" fill="none" />
            </svg>
          </button>
        </div>

        <div className="flex gap-0 px-5 border-b border-surface-overlay shrink-0">
          <button
            className={`min-h-11 px-3.5 bg-transparent border-b-2 font-body text-[13px] font-medium cursor-pointer transition-colors duration-150 ${
              activeTab === "general"
                ? "text-accent-action border-b-accent-action"
                : "text-text-muted border-b-transparent hover:text-text-secondary"
            }`}
            onClick={() => setActiveTab("general")}
            type="button"
          >
            General
          </button>
          <button
            className={`min-h-11 px-3.5 bg-transparent border-b-2 font-body text-[13px] font-medium cursor-pointer transition-colors duration-150 ${
              activeTab === "models"
                ? "text-accent-action border-b-accent-action"
                : "text-text-muted border-b-transparent hover:text-text-secondary"
            }`}
            onClick={() => setActiveTab("models")}
            type="button"
          >
            Models
          </button>
        </div>

        <div className="p-5 overflow-y-auto min-h-0">
          {activeTab === "models" ? (
            <div className="flex flex-col gap-3">
              <div className="flex items-center justify-between gap-3">
                <h3 className="font-sans text-[13px] font-semibold text-text-secondary uppercase tracking-[0.04em] m-0">Credentialed Providers</h3>
                <button
                  className="min-h-11 px-3 border border-surface-overlay rounded-md text-accent-action text-xs font-medium disabled:opacity-50 disabled:cursor-not-allowed"
                  type="button"
                  onClick={() => void onRefreshAvailability(true)}
                  disabled={isRefreshingModels}
                >
                  {isRefreshingModels ? "Refreshing..." : "Refresh models"}
                </button>
              </div>
              <p className="m-0 text-[13px] text-text-muted">
                Provider rows are loaded from backend availability. Add credentials to make models selectable.
              </p>
              <ProviderSelector providers={providers} onProvidersChange={onProvidersChange} onRefreshAvailability={onRefreshAvailability} />
            </div>
          ) : (
            <div className="flex flex-col gap-4">
              <section className="grid gap-2">
                <div>
                  <h3 className="m-0 text-heading-sm font-medium text-text-primary">Theme</h3>
                  <p className="m-0 text-body-sm text-text-muted">Dark stays the default for focused coding sessions.</p>
                </div>
                <div className="grid grid-cols-3 overflow-hidden rounded-md border border-surface-overlay" role="radiogroup" aria-label="Theme preference">
                  {THEME_OPTIONS.map((option, index) => {
                    const selected = option.value === themePreference;
                    return (
                      <button
                        key={option.value}
                        type="button"
                        role="radio"
                        aria-checked={selected}
                        tabIndex={selected ? 0 : -1}
                        ref={(el) => {
                          themeOptionRefs.current[index] = el;
                        }}
                        className={`min-h-11 px-3 py-2 text-left transition-colors duration-150 ease-out-quart ${
                          selected
                            ? "bg-surface-overlay text-accent-action"
                            : "text-text-muted hover:bg-surface-overlay hover:text-text-secondary"
                        }`}
                        onClick={() => onThemePreferenceChange(option.value)}
                        onKeyDown={(event) => {
                          if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
                          event.preventDefault();
                          const nextIndex =
                            event.key === "ArrowLeft"
                              ? (activeThemeIndex - 1 + THEME_OPTIONS.length) % THEME_OPTIONS.length
                              : (activeThemeIndex + 1) % THEME_OPTIONS.length;
                          const nextValue = THEME_OPTIONS[nextIndex]!.value;
                          onThemePreferenceChange(nextValue);
                          themeOptionRefs.current[nextIndex]?.focus();
                        }}
                      >
                        <span className="block text-body-sm font-medium">{option.label}</span>
                        <span className="block font-mono text-caption text-text-muted">{option.detail}</span>
                      </button>
                    );
                  })}
                </div>
              </section>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
