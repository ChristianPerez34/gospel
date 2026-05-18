import { useState, useEffect, useCallback } from "react";
import "./SettingsModal.css";
import type { ProviderConfig } from "./ProviderSelector";
import { ProviderSelector } from "./ProviderSelector";

interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
  providers: ProviderConfig[];
  onProvidersChange: (providers: ProviderConfig[]) => void;
}

type TabKey = "general" | "models";

export function SettingsModal({ open, onClose, providers, onProvidersChange }: SettingsModalProps) {
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
      <div className="settings-modal settings-modal--with-tabs" onClick={(e) => e.stopPropagation()}>
        <div className="settings-modal__header">
          <h2 className="settings-modal__title">Settings</h2>
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
              <h3 className="settings-modal__section-title">Configured Providers</h3>
              <p className="settings-modal__status">
                <span className="settings-modal__status--not-configured">
                  Enable a provider and add its API key to get started.
                </span>
              </p>
              <ProviderSelector providers={providers} onProvidersChange={onProvidersChange} />
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
