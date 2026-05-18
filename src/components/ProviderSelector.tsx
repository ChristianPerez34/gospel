import { useState, useCallback, useEffect, useRef } from "react";
import "./ProviderSelector.css";

export type ProviderId = "openai" | "anthropic" | "openrouter" | "local" | "custom" | "chatgpt";

export interface ProviderConfig {
  id: ProviderId;
  name: string;
  apiKey: string;
  enabled: boolean;
  status: "idle" | "testing" | "success" | "error";
  testMessage: string;
  isOAuth?: boolean;
  isAuthenticated?: boolean;
}

interface ProviderSelectorProps {
  providers: ProviderConfig[];
  onProvidersChange: (providers: ProviderConfig[]) => void;
}

export function ProviderSelector({ providers, onProvidersChange }: ProviderSelectorProps) {
  const [showKeyFor, setShowKeyFor] = useState<ProviderId | null>(null);
  const [oauthChallenge, setOauthChallenge] = useState<{ verification_url: string; user_code: string } | null>(null);
  const isOperationInProgress = useRef(false);

  // Ref to always have the latest providers in event listeners (avoids stale closures)
  const providersRef = useRef(providers);
  providersRef.current = providers;

  const onProvidersChangeRef = useRef(onProvidersChange);
  onProvidersChangeRef.current = onProvidersChange;

  useEffect(() => {
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const savedProviders = await invoke<ProviderConfig[]>("get_provider_configs");
        if (savedProviders && savedProviders.length > 0) {
          onProvidersChangeRef.current(savedProviders);
        }
      } catch {
      }
    })();

    // Check ChatGPT auth status
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const status = await invoke<{ configured: boolean }>("is_chatgpt_authenticated");
        if (status.configured) {
          const current = providersRef.current;
          const chatgptProvider = current.find(p => p.id === "chatgpt");
          if (chatgptProvider) {
            const updated = current.map((p) =>
              p.id === "chatgpt" ? { ...p, isAuthenticated: true, status: "success" as const, testMessage: "Authenticated" } : p
            );
            onProvidersChangeRef.current(updated);
          }
        }
      } catch {
      }
    })();

    // Listen for auth completion event
    let unlisten: (() => void) | undefined;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen("chatgpt-auth-complete", (event) => {
        const success = event.payload as boolean;
        const current = providersRef.current;
        if (success) {
          const updated = current.map((p) =>
            p.id === "chatgpt" ? { ...p, isAuthenticated: true, status: "success" as const, testMessage: "Authenticated" } : p
          );
          onProvidersChangeRef.current(updated);
          setOauthChallenge(null);
        } else {
          const updated = current.map((p) =>
            p.id === "chatgpt" ? { ...p, status: "error" as const, testMessage: "Authentication failed" } : p
          );
          onProvidersChangeRef.current(updated);
        }
      });
    })();

    return () => {
      unlisten?.();
    };
  }, []);

  const updateProvider = useCallback(
    (id: ProviderId, patch: Partial<ProviderConfig>) => {
      const updated = providers.map((p) => (p.id === id ? { ...p, ...patch } : p));
      onProvidersChange(updated);
    },
    [providers, onProvidersChange]
  );

  const persistProvider = useCallback(
    async (provider: ProviderConfig | null) => {
      if (!provider) return;
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("save_provider_config", { provider });
      } catch (e) {
        console.error("Failed to persist provider config:", e);
      }
    },
    []
  );

  const handleToggle = useCallback(
    (id: ProviderId) => {
      const provider = providers.find((p) => p.id === id)!;
      updateProvider(id, { enabled: !provider.enabled, status: "idle", testMessage: "" });
      persistProvider({ ...provider, enabled: !provider.enabled });
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
        const updated = { ...provider, status: "success" as const, testMessage: "Key saved" };
        updateProvider(provider.id, { status: "success", testMessage: "Key saved" });
        persistProvider(updated);
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

  const handleOAuthLogin = useCallback(
    async (provider: ProviderConfig) => {
      if (isOperationInProgress.current) return;
      isOperationInProgress.current = true;
      updateProvider(provider.id, { status: "testing", testMessage: "Starting authentication..." });
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const challenge = await invoke<{ verification_url: string; user_code: string }>("start_chatgpt_oauth");
        setOauthChallenge(challenge);
        updateProvider(provider.id, { status: "testing", testMessage: `Enter code: ${challenge.user_code}` });
      } catch (e) {
        updateProvider(provider.id, { status: "error", testMessage: `OAuth failed: ${e}` });
      } finally {
        isOperationInProgress.current = false;
      }
    },
    [updateProvider]
  );

  const handleOAuthLogout = useCallback(
    async (provider: ProviderConfig) => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("logout_chatgpt");
        updateProvider(provider.id, { isAuthenticated: false, status: "idle", testMessage: "" });
      } catch (e) {
        updateProvider(provider.id, { status: "error", testMessage: `Logout failed: ${e}` });
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
                {provider.isOAuth ? (
                  <div className="provider-card__oauth-row">
                    {provider.isAuthenticated ? (
                      <>
                        <span className="provider-card__oauth-status">✓ Authenticated</span>
                        <button
                          className="provider-card__oauth-btn provider-card__oauth-btn--logout"
                          onClick={() => handleOAuthLogout(provider)}
                          type="button"
                        >
                          Sign Out
                        </button>
                      </>
                    ) : (
                      <>
                        <span className="provider-card__oauth-hint">Sign in with your ChatGPT Plus/Pro account</span>
                        <button
                          className="provider-card__oauth-btn"
                          onClick={() => handleOAuthLogin(provider)}
                          disabled={isOperationInProgress.current}
                          type="button"
                        >
                          {isOperationInProgress.current ? "Connecting..." : "Sign in with OpenAI"}
                        </button>
                      </>
                    )}
                    {oauthChallenge && (
                      <div className="provider-card__oauth-code">
                        <span className="provider-card__oauth-code-label">Your code:</span>
                        <code className="provider-card__oauth-code-value">{oauthChallenge.user_code}</code>
                      </div>
                    )}
                  </div>
                ) : provider.id !== "local" && (
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
                    disabled={isTesting || !provider.enabled || (provider.isOAuth ? !provider.isAuthenticated : provider.id !== "local" && !provider.apiKey.trim())}
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
