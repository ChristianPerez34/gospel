import { useState, useCallback, useEffect, useRef } from "react";
import "./ProviderSelector.css";

export type ProviderId = "openai" | "anthropic" | "openrouter" | "local" | "custom";

export interface ProviderConfig {
  id: ProviderId;
  name: string;
  apiKey: string;
  enabled: boolean;
  status: "idle" | "testing" | "success" | "error";
  testMessage: string;
}

const DEFAULT_PROVIDERS: ProviderConfig[] = [
  { id: "openai", name: "OpenAI", apiKey: "", enabled: false, status: "idle", testMessage: "" },
  { id: "anthropic", name: "Anthropic", apiKey: "", enabled: false, status: "idle", testMessage: "" },
  { id: "openrouter", name: "OpenRouter", apiKey: "", enabled: false, status: "idle", testMessage: "" },
  { id: "local", name: "Local (Ollama / LM Studio)", apiKey: "", enabled: false, status: "idle", testMessage: "" },
  { id: "custom", name: "Custom", apiKey: "", enabled: false, status: "idle", testMessage: "" },
];

interface ProviderSelectorProps {
  providers: ProviderConfig[];
  onProvidersChange: (providers: ProviderConfig[]) => void;
}

export function ProviderSelector({ providers, onProvidersChange }: ProviderSelectorProps) {
  const [showKeyFor, setShowKeyFor] = useState<ProviderId | null>(null);
  const isOperationInProgress = useRef(false);

  useEffect(() => {
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const savedProviders = await invoke<ProviderConfig[]>("get_provider_configs");
        if (savedProviders && savedProviders.length > 0) {
          onProvidersChange(savedProviders);
        }
      } catch {
      }
    })();
  }, []);

  const updateProvider = useCallback(
    (id: ProviderId, patch: Partial<ProviderConfig>) => {
      const updated = providers.map((p) => (p.id === id ? { ...p, ...patch } : p));
      onProvidersChange(updated);
    },
    [providers, onProvidersChange]
  );

  const persistProvider = useCallback(
    async (id: ProviderId) => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const provider = providers.find((p) => p.id === id);
        if (provider) {
          await invoke("save_provider_config", { provider });
        }
      } catch (e) {
        console.error("Failed to persist provider config:", e);
      }
    },
    [providers]
  );

  const handleToggle = useCallback(
    (id: ProviderId) => {
      updateProvider(id, { enabled: !providers.find((p) => p.id === id)!.enabled, status: "idle", testMessage: "" });
      persistProvider(id);
    },
    [providers, updateProvider, persistProvider]
  );

  const handleSaveKey = useCallback(
    async (provider: ProviderConfig) => {
      if (!provider.apiKey.trim() || isOperationInProgress.current) return;
      isOperationInProgress.current = true;
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("set_api_key", { provider: provider.id, apiKey: provider.apiKey.trim() });
        updateProvider(provider.id, { status: "success", testMessage: "Key saved" });
        await persistProvider(provider.id);
        const result = await invoke<boolean>("test_connection", { provider: provider.id });
        if (result) {
          updateProvider(provider.id, { status: "success", testMessage: "Connected" });
        } else {
          updateProvider(provider.id, { status: "error", testMessage: "Key saved but connection failed" });
        }
      } catch (e) {
        updateProvider(provider.id, { status: "error", testMessage: `Failed to save: ${e}` });
      } finally {
        isOperationInProgress.current = false;
      }
    },
    [updateProvider, persistProvider]
  );

  const handleTest = useCallback(
    async (provider: ProviderConfig) => {
      if (!provider.enabled || (provider.id !== "local" && !provider.apiKey.trim()) || isOperationInProgress.current) return;
      isOperationInProgress.current = true;
      updateProvider(provider.id, { status: "testing", testMessage: "" });
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const result = await invoke<boolean>("test_connection", { provider: provider.id });
        if (result) {
          updateProvider(provider.id, { status: "success", testMessage: "Connected" });
        } else {
          updateProvider(provider.id, { status: "error", testMessage: "Connection failed" });
        }
      } catch (e) {
        updateProvider(provider.id, { status: "error", testMessage: `Connection failed: ${e}` });
      } finally {
        isOperationInProgress.current = false;
      }
    },
    [updateProvider]
  );

  return (
    <div className="provider-selector">
      {providers.map((provider) => {
        const showKey = showKeyFor === provider.id;
        const isIdle = provider.status === "idle";
        const isTesting = provider.status === "testing";
        const isSuccess = provider.status === "success";
        const isError = provider.status === "error";

        return (
          <div
            key={provider.id}
            className={`provider-card ${provider.enabled ? "provider-card--enabled" : ""} ${isError ? "provider-card--error" : ""}`}
          >
            <div className="provider-card__header">
              <div className="provider-card__identity">
                <span className="provider-card__name">{provider.name}</span>
                {!isIdle && (
                  <span
                    className={`provider-card__badge provider-card__badge--${provider.status}`}
                    aria-label={provider.testMessage}
                    title={provider.testMessage}
                  >
                    {isTesting ? "…" : isSuccess ? "✓" : isError ? "✕" : null}
                  </span>
                )}
              </div>

              <button
                className={`provider-card__toggle ${provider.enabled ? "provider-card__toggle--on" : ""}`}
                onClick={() => handleToggle(provider.id)}
                aria-pressed={provider.enabled}
                type="button"
                aria-label={provider.enabled ? `Disable ${provider.name}` : `Enable ${provider.name}`}
              >
                <span className="provider-card__toggle-knob" />
              </button>
            </div>

            {provider.enabled && (
              <div className="provider-card__body">
                {provider.id !== "local" && (
                  <div className="provider-card__input-row">
                    <div className="provider-card__input-wrapper">
                      <input
                        type={showKey ? "text" : "password"}
                        className="provider-card__input"
                        placeholder={`${provider.name} API key`}
                        value={provider.apiKey}
                        onChange={(e) => updateProvider(provider.id, { apiKey: e.target.value, status: "idle", testMessage: "" })}
                        aria-label={`${provider.name} API key`}
                      />
                      <button
                        className="provider-card__toggle-visibility"
                        onClick={() => setShowKeyFor((prev) => (prev === provider.id ? null : provider.id))}
                        aria-label={showKey ? "Hide API key" : "Show API key"}
                        type="button"
                      >
                        {showKey ? (
                          <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.2">
                            <path d="M2 8s2.5-4 6-4 6 4 6 4-2.5 4-6 4-6-4-6-4z" />
                            <circle cx="8" cy="8" r="1.5" />
                            <line x1="2" y1="14" x2="14" y2="2" />
                          </svg>
                        ) : (
                          <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.2">
                            <path d="M2 8s2.5-4 6-4 6 4 6 4-2.5 4-6 4-6-4-6-4z" />
                            <circle cx="8" cy="8" r="1.5" />
                          </svg>
                        )}
                      </button>
                    </div>
                    <button
                      className="provider-card__save-btn"
                      onClick={() => handleSaveKey(provider)}
                      disabled={!provider.apiKey.trim()}
                      type="button"
                    >
                      Save
                    </button>
                  </div>
                )}

                <div className="provider-card__test-row">
                  <button
                    className="provider-card__test-btn"
                    onClick={() => handleTest(provider)}
                    disabled={isTesting || !provider.enabled || (provider.id !== "local" && !provider.apiKey.trim())}
                    type="button"
                  >
                    {isTesting ? "Testing…" : "Test Connection"}
                  </button>
                  {!isIdle && (
                    <span
                      className={`provider-card__test-result provider-card__test-result--${provider.status}`}
                    >
                      {provider.testMessage}
                    </span>
                  )}
                </div>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
