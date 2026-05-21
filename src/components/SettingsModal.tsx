import { useState, useEffect, useCallback } from "react";
import "./SettingsModal.css";
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
    <div className="settings-modal__overlay" onClick={onClose}>
      <div className="settings-modal settings-modal--with-tabs" onClick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-labelledby="settings-modal-title">
        <div className="settings-modal__header">
          <h2 className="settings-modal__title" id="settings-modal-title">Settings</h2>
          <button className="settings-modal__close" onClick={onClose} aria-label="Close settings" type="button">
            <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
              <path d="M4.5 4.5L11.5 11.5M11.5 4.5L4.5 11.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" fill="none" />
            </svg>
          </button>
        </div>

        <div className="settings-modal__tabs">
          <button
            className={`settings-modal__tab ${activeTab === "general" ? "settings-modal__tab--active" : ""}`}
            onClick={() => setActiveTab("general")}
            type="button"
          >
            General
          </button>
          <button
            className={`settings-modal__tab ${activeTab === "models" ? "settings-modal__tab--active" : ""}`}
            onClick={() => setActiveTab("models")}
            type="button"
          >
            Models
          </button>
        </div>

        <div className="settings-modal__body">
          {activeTab === "models" ? (
            <div className="settings-modal__section">
              <div className="settings-modal__section-header">
                <h3 className="settings-modal__section-title">Credentialed Providers</h3>
                <button
                  className="settings-modal__refresh-btn"
                  type="button"
                  onClick={() => void onRefreshAvailability(true)}
                  disabled={isRefreshingModels}
                >
                  {isRefreshingModels ? "Refreshing..." : "Refresh models"}
                </button>
              </div>
              <p className="settings-modal__status">
                <span className="settings-modal__status--not-configured">
                  Provider rows are loaded from backend availability. Add credentials to make models selectable.
                </span>
              </p>
              <ProviderSelector providers={providers} onProvidersChange={onProvidersChange} onRefreshAvailability={onRefreshAvailability} />
            </div>
          ) : (
            <div className="settings-modal__section">
              <p className="settings-modal__status settings-modal__status--not-configured">General settings coming soon.</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
