import { useState, useCallback, useEffect, useRef } from "react";
import "./ProviderSelector.css";

export type ProviderId = "openai" | "chatgpt" | "anthropic" | "gemini" | "groq" | "mistral";

export interface ProviderConfig {
  id: ProviderId;
  name: string;
  authType: "api_key" | "oauth";
  credentialed: boolean;
  visible: boolean;
  modelFetchStatus: string;
  modelCount: number;
  errorKind?: string;
  errorDetail?: string;
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
  onRefreshAvailability: (forceRefresh?: boolean) => Promise<void>;
}

function providerAvailabilitySummary(provider: ProviderConfig) {
  if (!provider.credentialed) return "Not credentialed";
  if (!provider.visible) return "Hidden";
  if (provider.modelFetchStatus === "failed") return provider.errorDetail || "Model load failed";
  if (provider.modelFetchStatus === "stale") return `${provider.modelCount} stale models`;
  if (provider.modelFetchStatus === "empty") return "No models returned";
  return `${provider.modelCount} models`;
}

export function ProviderSelector({ providers, onProvidersChange, onRefreshAvailability }: ProviderSelectorProps) {
  const [showKeyFor, setShowKeyFor] = useState<ProviderId | null>(null);
  const [editingKeyFor, setEditingKeyFor] = useState<ProviderId | null>(null);
  const [oauthChallenge, setOauthChallenge] = useState<{ verification_url: string; user_code: string } | null>(null);
  const isOperationInProgress = useRef(false);

  // Ref to always have the latest providers in event listeners (avoids stale closures)
  const providersRef = useRef(providers);
  providersRef.current = providers;

  const onProvidersChangeRef = useRef(onProvidersChange);
  onProvidersChangeRef.current = onProvidersChange;

  useEffect(() => {
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
              p.id === "chatgpt" ? { ...p, isAuthenticated: true, credentialed: true, status: "success" as const, testMessage: "Authenticated" } : p
            );
            onProvidersChangeRef.current(updated);
          }
        }
      } catch {
      }
    })();

    // Listen for auth completion event
    let unlisten: (() => void) | undefined;
    const setupAuthListener = async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen("chatgpt-auth-complete", (event) => {
          const success = event.payload as boolean;
          const current = providersRef.current;
          if (success) {
            const updated = current.map((p) =>
              p.id === "chatgpt" ? { ...p, isAuthenticated: true, credentialed: true, status: "success" as const, testMessage: "Authenticated" } : p
            );
            onProvidersChangeRef.current(updated);
            setOauthChallenge(null);
            void onRefreshAvailability();
          } else {
            const updated = current.map((p) =>
              p.id === "chatgpt" ? { ...p, status: "error" as const, testMessage: "Authentication failed" } : p
            );
            onProvidersChangeRef.current(updated);
            void onRefreshAvailability();
          }
        });
      } catch (error) {
        console.error("Failed to setup auth listener:", error);
      }
    };
    setupAuthListener();

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  const updateProvider = useCallback(
    (id: ProviderId, patch: Partial<ProviderConfig>) => {
      const updated = providers.map((p) => (p.id === id ? { ...p, ...patch } : p));
      onProvidersChange(updated);
    },
    [providers, onProvidersChange]
  );

  const handleToggle = useCallback(
    async (id: ProviderId) => {
      const provider = providers.find((p) => p.id === id)!;
      if (!provider.credentialed) return;
      const nextVisible = !provider.visible;
      updateProvider(id, { visible: nextVisible, enabled: nextVisible, status: "idle", testMessage: "" });
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("set_provider_visibility", { provider: id, visible: nextVisible });
        await onRefreshAvailability();
      } catch (e) {
        updateProvider(id, { visible: provider.visible, enabled: provider.visible, status: "error", testMessage: `Visibility update failed: ${e}` });
      }
    },
    [providers, updateProvider, onRefreshAvailability]
  );

  const handleSaveKey = useCallback(
    async (provider: ProviderConfig) => {
      if (!provider.apiKey.trim() || isOperationInProgress.current) return;
      isOperationInProgress.current = true;
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("set_api_key", { provider: provider.id, apiKey: provider.apiKey.trim() });
        updateProvider(provider.id, { apiKey: "", credentialed: true, status: "success", testMessage: "Key saved" });
        setEditingKeyFor(null);
        await onRefreshAvailability();
      } catch (e) {
        updateProvider(provider.id, { status: "error", testMessage: `Failed to save: ${e}` });
      } finally {
        isOperationInProgress.current = false;
      }
    },
    [updateProvider, onRefreshAvailability]
  );

  const handleRemoveKey = useCallback(
    async (provider: ProviderConfig) => {
      if (isOperationInProgress.current) return;
      if (provider.isOAuth) {
        await handleOAuthLogout(provider);
        return;
      }
      isOperationInProgress.current = true;
      updateProvider(provider.id, { status: "testing", testMessage: "Removing key..." });
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("delete_api_key", { provider: provider.id });
        updateProvider(provider.id, { apiKey: "", credentialed: false, status: "idle", testMessage: "" });
        await onRefreshAvailability();
      } catch (e) {
        updateProvider(provider.id, { status: "error", testMessage: `Failed to remove key: ${e}` });
      } finally {
        isOperationInProgress.current = false;
      }
    },
    [updateProvider, onRefreshAvailability]
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
        await onRefreshAvailability();
      } finally {
        isOperationInProgress.current = false;
      }
    },
    [updateProvider, onRefreshAvailability]
  );

  const handleOAuthLogout = useCallback(
    async (provider: ProviderConfig) => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("logout_chatgpt");
        updateProvider(provider.id, { isAuthenticated: false, credentialed: false, status: "idle", testMessage: "" });
        await onRefreshAvailability();
      } catch (e) {
        updateProvider(provider.id, { status: "error", testMessage: `Logout failed: ${e}` });
      }
    },
    [updateProvider, onRefreshAvailability]
  );

  return (
    <div className="provider-selector">
      {providers.map((provider) => {
        const showKey = showKeyFor === provider.id;
        const editingKey = editingKeyFor === provider.id || !provider.credentialed;
        const isIdle = provider.status === "idle";
        const isTesting = provider.status === "testing";
        const isSuccess = provider.status === "success";
        const isError = provider.status === "error";

        return (
          <div
            key={provider.id}
            className={`provider-card ${provider.visible ? "provider-card--enabled" : ""} ${isError ? "provider-card--error" : ""}`}
          >
            <div className="provider-card__header">
              <div className="provider-card__identity">
                <span className="provider-card__name">{provider.name}</span>
                <span className="provider-card__test-result">
                  {providerAvailabilitySummary(provider)}
                </span>
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
                className={`provider-card__toggle ${provider.visible ? "provider-card__toggle--on" : ""}`}
                onClick={() => handleToggle(provider.id)}
                aria-pressed={provider.visible}
                type="button"
                aria-label={provider.visible ? `Hide ${provider.name} from model picker` : `Show ${provider.name} in model picker`}
                title="Provider visibility"
              >
                <span className="provider-card__toggle-knob" />
              </button>
            </div>

            {(provider.visible || !provider.credentialed) && (
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
                ) : editingKey ? (
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
                ) : (
                  <div className="provider-card__oauth-row">
                    <span className="provider-card__oauth-status">Key saved</span>
                    <button
                      className="provider-card__oauth-btn"
                      onClick={() => setEditingKeyFor(provider.id)}
                      type="button"
                    >
                      Replace
                    </button>
                    <button
                      className="provider-card__oauth-btn provider-card__oauth-btn--logout"
                      onClick={() => handleRemoveKey(provider)}
                      disabled={isOperationInProgress.current}
                      type="button"
                    >
                      Remove
                    </button>
                  </div>
                )}

                {!isIdle && (
                  <span className={`provider-card__test-result provider-card__test-result--${provider.status}`}>
                    {provider.testMessage}
                  </span>
                )}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
