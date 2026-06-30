import { useState, useCallback, useEffect, useRef } from "react";
import { Button } from "@/components/ui/button";

export type ProviderId = "openai" | "chatgpt" | "github_copilot" | "anthropic" | "gemini" | "groq" | "mistral";

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

interface OAuthChallenge {
  provider: ProviderId;
  verification_url: string;
  user_code: string;
}

interface OAuthCompletion {
  provider: ProviderId;
  success: boolean;
}

const OAUTH_PROVIDER_IDS: ProviderId[] = ["chatgpt", "github_copilot"];

function oauthCopy(provider: ProviderConfig) {
  switch (provider.id) {
    case "github_copilot":
      return {
        prompt: "Sign in with the GitHub account that has Copilot access",
        button: "Sign in with GitHub",
        connecting: "Connecting to GitHub...",
      };
    case "chatgpt":
    default:
      return {
        prompt: "Sign in with your ChatGPT Plus/Pro account",
        button: "Sign in with OpenAI",
        connecting: "Connecting...",
      };
  }
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
  const [oauthChallenge, setOauthChallenge] = useState<OAuthChallenge | null>(null);
  const isOperationInProgress = useRef(false);

  const providersRef = useRef(providers);
  providersRef.current = providers;

  const onProvidersChangeRef = useRef(onProvidersChange);
  onProvidersChangeRef.current = onProvidersChange;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const statuses = await Promise.all(
          OAUTH_PROVIDER_IDS.map(async (provider) => ({
            provider,
            status: await invoke<{ configured: boolean }>("is_provider_authenticated", { provider }),
          })),
        );
        if (!cancelled) {
          const current = providersRef.current;
          const configured = new Set(statuses.filter(({ status }) => status.configured).map(({ provider }) => provider));
          if (configured.size > 0) {
            const updated = current.map((p) =>
              configured.has(p.id)
                ? { ...p, isAuthenticated: true, credentialed: true, status: "success" as const, testMessage: "Authenticated" }
                : p
            );
            onProvidersChangeRef.current(updated);
          }
        }
      } catch {
      }
    })();

    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        const unlistenFn = await listen("provider-auth-complete", (event) => {
          const { provider, success } = event.payload as OAuthCompletion;
          const current = providersRef.current;
          if (success) {
            const updated = current.map((p) =>
              p.id === provider ? { ...p, isAuthenticated: true, credentialed: true, status: "success" as const, testMessage: "Authenticated" } : p
            );
            onProvidersChangeRef.current(updated);
            setOauthChallenge(null);
            void onRefreshAvailability();
          } else {
            const updated = current.map((p) =>
              p.id === provider ? { ...p, status: "error" as const, testMessage: "Authentication failed" } : p
            );
            onProvidersChangeRef.current(updated);
            void onRefreshAvailability();
          }
        });

        if (cancelled) {
          unlistenFn();
          return;
        }

        unlisten = unlistenFn;
      } catch (error) {
        console.error("Failed to setup auth listener:", error);
      }
    })();

    return () => {
      cancelled = true;
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
        const challenge = await invoke<{ verification_url: string; user_code: string }>("start_provider_oauth", { provider: provider.id });
        setOauthChallenge({ ...challenge, provider: provider.id });
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
        await invoke("logout_provider_oauth", { provider: provider.id });
        updateProvider(provider.id, { isAuthenticated: false, credentialed: false, status: "idle", testMessage: "" });
        setOauthChallenge((current) => (current?.provider === provider.id ? null : current));
        await onRefreshAvailability();
      } catch (e) {
        updateProvider(provider.id, { status: "error", testMessage: `Logout failed: ${e}` });
      }
    },
    [updateProvider, onRefreshAvailability]
  );

  return (
    <div className="flex flex-col gap-3">
      {providers.map((provider) => {
        const showKey = showKeyFor === provider.id;
        const editingKey = editingKeyFor === provider.id || !provider.credentialed;
        const isIdle = provider.status === "idle";
        const isTesting = provider.status === "testing";
        const isSuccess = provider.status === "success";
        const isError = provider.status === "error";
        const copy = provider.isOAuth ? oauthCopy(provider) : null;
        const providerChallenge = oauthChallenge?.provider === provider.id ? oauthChallenge : null;

        const cardBorder = isError
          ? "border-status-error"
          : provider.visible
          ? "border-accent-action"
          : "border-surface-overlay";

        return (
          <div
            key={provider.id}
            className={`bg-surface-elevated border rounded-md overflow-hidden transition-colors duration-150 ease-out-quart ${cardBorder}`}
          >
            <div className="flex items-center justify-between py-3 px-4 gap-3">
              <div className="flex items-center gap-2">
                <span className="font-body text-body-sm font-medium text-text-primary">{provider.name}</span>
                <span className="text-caption">
                  {providerAvailabilitySummary(provider)}
                </span>
                {!isIdle && (
                  <span
                    className={`inline-flex items-center justify-center w-4 h-4 text-caption font-semibold rounded-full leading-none ${
                      isSuccess
                        ? "text-status-success"
                        : isError
                        ? "text-status-error"
                        : "text-text-muted animate-pulse"
                    }`}
                    aria-label={provider.testMessage}
                    title={provider.testMessage}
                  >
                    {isTesting ? "…" : isSuccess ? "✓" : isError ? "✕" : null}
                  </span>
                )}
              </div>

              <button
                className={`hit-target relative w-9 h-5 border-none rounded-full cursor-pointer p-0 transition-colors duration-150 ease-out-quart shrink-0 ${
                  provider.visible ? "bg-accent-action" : "bg-surface-overlay"
                }`}
                onClick={() => handleToggle(provider.id)}
                aria-pressed={provider.visible}
                type="button"
                aria-label={provider.visible ? `Hide ${provider.name} from model picker` : `Show ${provider.name} in model picker`}
                title="Provider visibility"
              >
                <span
                  className={`absolute top-0.5 left-0.5 w-4 h-4 bg-text-inverse rounded-full transition-transform duration-150 ease-out-quart ${
                    provider.visible ? "translate-x-4" : ""
                  }`}
                />
              </button>
            </div>

            {(provider.visible || !provider.credentialed) && (
              <div className="px-4 pb-3 flex flex-col gap-2 animate-appear-body">
                {provider.isOAuth ? (
                  <div className="flex items-center justify-between gap-2 py-1">
                    {provider.isAuthenticated ? (
                      <>
                        <span className="text-caption text-status-success font-body font-medium">✓ Authenticated</span>
                        <Button
                          variant="secondary"
                          size="sm"
                          onClick={() => handleOAuthLogout(provider)}
                        >
                          Sign Out
                        </Button>
                      </>
                    ) : (
                      <>
                        <span className="text-caption text-text-muted font-body">{copy?.prompt}</span>
                        <Button
                          variant="default"
                          size="sm"
                          onClick={() => handleOAuthLogin(provider)}
                          disabled={isOperationInProgress.current}
                        >
                          {isOperationInProgress.current ? copy?.connecting : copy?.button}
                        </Button>
                      </>
                    )}
                    {providerChallenge && (
                      <div className="flex items-center gap-2 pt-2">
                        <span className="text-caption text-text-muted font-body">Your code:</span>
                        <code className="font-mono text-body-sm text-accent-action bg-surface-base py-0.5 px-2 rounded-sm tracking-[0.15em]">{providerChallenge.user_code}</code>
                      </div>
                    )}
                  </div>
                ) : editingKey ? (
                  <div className="flex items-center gap-2">
                    <div className="flex-1 relative flex items-center">
                      <input
                        type={showKey ? "text" : "password"}
                        className="w-full py-1.5 pr-8 pl-2.5 bg-surface-base border border-surface-overlay rounded-sm text-text-primary font-mono text-mono outline-none transition-colors duration-150 ease-out-quart placeholder:text-text-muted focus:border-accent-action"
                        placeholder={`${provider.name} API key`}
                        value={provider.apiKey}
                        onChange={(e) => updateProvider(provider.id, { apiKey: e.target.value, status: "idle", testMessage: "" })}
                        aria-label={`${provider.name} API key`}
                      />
                      <button
                        className="hit-target absolute right-1.5 border-none bg-transparent text-text-muted cursor-pointer p-0.5 rounded-sm flex items-center justify-center transition-colors duration-150 hover:text-text-primary"
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
                    <Button
                      variant="default"
                      size="sm"
                      onClick={() => handleSaveKey(provider)}
                      disabled={!provider.apiKey.trim()}
                    >
                      Save
                    </Button>
                  </div>
                ) : (
                  <div className="flex items-center justify-between gap-2 py-1">
                    <span className="text-caption text-status-success font-body font-medium">Key saved</span>
                    <div className="flex items-center gap-2">
                      <Button
                        variant="default"
                        size="sm"
                        onClick={() => setEditingKeyFor(provider.id)}
                      >
                        Replace
                      </Button>
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => handleRemoveKey(provider)}
                        disabled={isOperationInProgress.current}
                      >
                        Remove
                      </Button>
                    </div>
                  </div>
                )}

                {!isIdle && (
                  <span className={`text-caption font-body ${
                    isSuccess
                      ? "text-status-success"
                      : isError
                      ? "text-status-error"
                      : "text-text-muted"
                  }`}>
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
