import { useState, useEffect, useCallback } from "react";
import type { ProviderConfig } from "./ProviderSelector";
import { ProviderSelector } from "./ProviderSelector";

interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
  providers: ProviderConfig[];
  onProvidersChange: (providers: ProviderConfig[]) => void;
  onRefreshAvailability: (forceRefresh?: boolean) => Promise<void>;
  isRefreshingModels: boolean;
}

type TabKey = "general" | "models";

export function SettingsModal({ open, onClose, providers, onProvidersChange, onRefreshAvailability, isRefreshingModels }: SettingsModalProps) {
  const [activeTab, setActiveTab] = useState<TabKey>("models");

  useEffect(() => {
    if (!open) return;
    setActiveTab("models");
  }, [open]);

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
    <div className="fixed inset-0 bg-scrim z-[100] flex items-center justify-center animate-fade-in" onClick={onClose}>
      <div
        className="w-[520px] max-w-[90vw] max-h-[80vh] flex flex-col bg-surface-elevated border border-surface-overlay rounded-lg shadow-[0_16px_48px_rgba(0,0,0,0.4)] animate-slide-up"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-modal-title"
      >
        <div className="flex items-center justify-between px-5 py-4 border-b border-surface-overlay shrink-0">
          <h2 className="font-sans text-base font-semibold text-text-primary m-0" id="settings-modal-title">Settings</h2>
          <button
            className="text-text-muted p-1 rounded-sm flex items-center justify-center transition-colors duration-150 hover:text-text-primary hover:bg-surface-overlay"
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
            className={`py-2.5 px-3.5 bg-transparent border-none border-b-2 font-body text-[13px] font-medium cursor-pointer transition-colors duration-150 ${
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
            className={`py-2.5 px-3.5 bg-transparent border-none border-b-2 font-body text-[13px] font-medium cursor-pointer transition-colors duration-150 ${
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
                  className="py-1.5 px-2.5 border border-surface-overlay rounded-md text-accent-action text-xs font-medium disabled:opacity-50 disabled:cursor-not-allowed"
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
            <div className="flex flex-col gap-3">
              <p className="m-0 text-[13px] text-text-muted">General settings coming soon.</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
