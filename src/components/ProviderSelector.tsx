import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { oauthCopy, oauthProviderIds } from "./oauthProviders";

export type ProviderId = string;

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

function providerAvailabilitySummary(provider: ProviderConfig) {
  if (!provider.credentialed) return "Not credentialed";
  if (!provider.visible) return "Hidden";
  if (provider.modelFetchStatus === "failed") return provider.errorDetail || "Model load failed";
  if (provider.modelFetchStatus === "stale") return `${provider.modelCount} stale models`;
  if (provider.modelFetchStatus === "empty") return "No models returned";
  return `${provider.modelCount} models`;
}

// Best-effort visibility persist: callers await it to preserve write ordering,
// failures only log since the credential op itself already succeeded.
async function persistVisibility(provider: ProviderId, visible: boolean): Promise<void> {
  try {
    await invoke("set_provider_visibility", { provider, visible });
  } catch (e) {
    console.error("Failed to persist provider visibility:", e);
  }
}

export function ProviderSelector({
  providers,
  onProvidersChange,
  onRefreshAvailability,
}: ProviderSelectorProps) {
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
        const oauthIds = oauthProviderIds(providersRef.current);
        const statuses = await Promise.all(
          oauthIds.map(async (provider) => ({
            provider,
            status: await invoke<{ configured: boolean }>("is_provider_authenticated", {
              provider,
            }),
          }))
        );
        if (!cancelled) {
          const snapshot = providersRef.current;
          const configured = new Set(
            statuses.filter(({ status }) => status?.configured).map(({ provider }) => provider)
          );
          if (configured.size > 0) {
            const updated = snapshot.map((p) =>
              configured.has(p.id)
                ? {
                    ...p,
                    isAuthenticated: true,
                    credentialed: true,
                    // Preserve an existing hidden preference; only default
                    // newly-discovered providers to visible.
                    ...(p.credentialed ? {} : { visible: true, enabled: true }),
                    status: "success" as const,
                    testMessage: "Authenticated",
                  }
                : p
            );
            onProvidersChangeRef.current(updated);
          }
        }
      } catch {}
    })();

    (async () => {
      try {
        const unlistenFn = await listen("provider-auth-complete", async (event) => {
          const { provider, success } = event.payload as OAuthCompletion;
          const preAuth = providersRef.current;
          if (success) {
            // Only default newly-credentialed providers to visible; a re-auth
            // must not clobber an existing hidden preference.
            const wasCredentialed = preAuth.find((p) => p.id === provider)?.credentialed ?? false;
            if (!wasCredentialed) {
              await persistVisibility(provider, true);
            }
            const fresh = providersRef.current;
            const updated = fresh.map((p) =>
              p.id === provider
                ? {
                    ...p,
                    isAuthenticated: true,
                    credentialed: true,
                    ...(wasCredentialed ? {} : { visible: true, enabled: true }),
                    status: "success" as const,
                    testMessage: "Authenticated",
                  }
                : p
            );
            onProvidersChangeRef.current(updated);
            setOauthChallenge(null);
            void onRefreshAvailability();
          } else {
            const updated = preAuth.map((p) =>
              p.id === provider
                ? { ...p, status: "error" as const, testMessage: "Authentication failed" }
                : p
            );
            onProvidersChangeRef.current(updated);
            void onRefreshAvailability();
          }
        });

        if (typeof unlistenFn === "function") {
          if (cancelled) {
            unlistenFn();
            return;
          }
          unlisten = unlistenFn;
        }
      } catch (error) {
        console.error("Failed to setup auth listener:", error);
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [onRefreshAvailability]);

  const updateProvider = useCallback(
    (id: ProviderId, patch: Partial<ProviderConfig>) => {
      const updated = providers.map((p) => (p.id === id ? { ...p, ...patch } : p));
      onProvidersChange(updated);
    },
    [providers, onProvidersChange]
  );

  const handleToggle = useCallback(
    async (id: ProviderId) => {
      const provider = providers.find((p) => p.id === id);
      if (!provider?.credentialed) return;
      const nextVisible = !provider.visible;
      updateProvider(id, {
        visible: nextVisible,
        enabled: nextVisible,
        status: "idle",
        testMessage: "",
      });
      try {
        await invoke("set_provider_visibility", { provider: id, visible: nextVisible });
        await onRefreshAvailability();
      } catch (e) {
        updateProvider(id, {
          visible: provider.visible,
          enabled: provider.visible,
          status: "error",
          testMessage: `Visibility update failed: ${e}`,
        });
      }
    },
    [providers, updateProvider, onRefreshAvailability]
  );

  const handleSaveKey = useCallback(
    async (provider: ProviderConfig) => {
      if (!provider.apiKey.trim() || isOperationInProgress.current) return;
      isOperationInProgress.current = true;
      try {
        await invoke("set_api_key", { provider: provider.id, apiKey: provider.apiKey.trim() });
        // Best-effort: the key is saved even if persisting visibility fails.
        await persistVisibility(provider.id, true);
        updateProvider(provider.id, {
          apiKey: "",
          credentialed: true,
          visible: true,
          enabled: true,
          status: "success",
          testMessage: "Key saved",
        });
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

  const handleOAuthLogout = useCallback(
    async (provider: ProviderConfig) => {
      try {
        await invoke("logout_provider_oauth", { provider: provider.id });
        // Best-effort: the session is ended even if persisting visibility fails.
        await persistVisibility(provider.id, false);
        updateProvider(provider.id, {
          isAuthenticated: false,
          credentialed: false,
          visible: false,
          enabled: false,
          status: "idle",
          testMessage: "",
        });
        setOauthChallenge((current) => (current?.provider === provider.id ? null : current));
        await onRefreshAvailability();
      } catch (e) {
        updateProvider(provider.id, { status: "error", testMessage: `Logout failed: ${e}` });
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
        await invoke("delete_api_key", { provider: provider.id });
        // Best-effort: the key is deleted even if persisting visibility fails.
        await persistVisibility(provider.id, false);
        updateProvider(provider.id, {
          apiKey: "",
          credentialed: false,
          visible: false,
          enabled: false,
          status: "idle",
          testMessage: "",
        });
        await onRefreshAvailability();
      } catch (e) {
        updateProvider(provider.id, { status: "error", testMessage: `Failed to remove key: ${e}` });
      } finally {
        isOperationInProgress.current = false;
      }
    },
    [updateProvider, onRefreshAvailability, handleOAuthLogout]
  );

  const handleOAuthLogin = useCallback(
    async (provider: ProviderConfig) => {
      if (isOperationInProgress.current) return;
      isOperationInProgress.current = true;
      updateProvider(provider.id, { status: "testing", testMessage: "Starting authentication..." });
      try {
        const challenge = await invoke<{ verification_url: string; user_code: string }>(
          "start_provider_oauth",
          { provider: provider.id }
        );
        setOauthChallenge({ ...challenge, provider: provider.id });
        updateProvider(provider.id, {
          status: "testing",
          testMessage: `Enter code: ${challenge.user_code}`,
        });
      } catch (e) {
        updateProvider(provider.id, { status: "error", testMessage: `OAuth failed: ${e}` });
        await onRefreshAvailability();
      } finally {
        isOperationInProgress.current = false;
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

        const isConfigured = provider.credentialed;
        const isVisible = isConfigured && provider.visible;

        const cardBorder = isError
          ? "border-status-error"
          : isVisible
            ? "border-accent-action"
            : "border-surface-overlay";

        const toggleLabel = !isConfigured
          ? `Add credentials to enable ${provider.name}`
          : isVisible
            ? `Hide ${provider.name} from model picker`
            : `Show ${provider.name} in model picker`;

        return (
          <div
            key={provider.id}
            className={`bg-surface-elevated border rounded-md overflow-hidden transition-colors duration-150 ease-out-quart ${cardBorder}`}
          >
            <div className="flex items-center justify-between py-3 px-4 gap-3">
              <div className="flex items-center gap-2">
                <span className="font-body text-body-sm font-medium text-text-primary">
                  {provider.name}
                </span>
                <span
                  className={`text-caption ${
                    !isConfigured
                      ? "text-text-muted"
                      : !isVisible
                        ? "text-text-muted italic"
                        : isError
                          ? "text-status-error"
                          : "text-text-secondary"
                  }`}
                >
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
                    title={provider.testMessage}
                  >
                    <span aria-hidden="true">
                      {isTesting ? "…" : isSuccess ? "✓" : isError ? "✕" : null}
                    </span>
                    <span className="sr-only">{provider.testMessage}</span>
                  </span>
                )}
              </div>

              <button
                className={`hit-target relative w-9 h-5 border-none rounded-full p-0 transition-all duration-150 ease-out-quart shrink-0 ${
                  isVisible ? "bg-accent-action" : "bg-surface-overlay"
                } ${
                  !isConfigured ? "opacity-40 cursor-not-allowed" : "cursor-pointer active:scale-95"
                }`}
                onClick={() => handleToggle(provider.id)}
                aria-pressed={isVisible}
                disabled={!isConfigured}
                type="button"
                aria-label={toggleLabel}
                title={toggleLabel}
              >
                <span
                  className={`absolute top-0.5 left-0.5 w-4 h-4 bg-text-inverse rounded-full transition-transform duration-150 ease-out-quart ${
                    isVisible ? "translate-x-4" : ""
                  }`}
                />
              </button>
            </div>

            {(isVisible || !isConfigured) && (
              <div className="px-4 pb-3 flex flex-col gap-2">
                {provider.isOAuth ? (
                  <div className="flex items-center justify-between gap-2 py-1">
                    {provider.isAuthenticated ? (
                      <>
                        <span className="text-caption text-status-success font-body font-medium">
                          ✓ Authenticated
                        </span>
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
                        <span className="text-caption text-text-muted font-body">
                          {copy?.prompt}
                        </span>
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
                        <code className="font-mono text-body-sm text-accent-action bg-surface-base py-0.5 px-2 rounded-sm tracking-[0.15em]">
                          {providerChallenge.user_code}
                        </code>
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
                        onChange={(e) =>
                          updateProvider(provider.id, {
                            apiKey: e.target.value,
                            status: "idle",
                            testMessage: "",
                          })
                        }
                        aria-label={`${provider.name} API key`}
                      />
                      <button
                        className="hit-target absolute right-1.5 border-none bg-transparent text-text-muted cursor-pointer p-0.5 rounded-sm flex items-center justify-center transition-colors duration-150 hover:text-text-primary"
                        onClick={() =>
                          setShowKeyFor((prev) => (prev === provider.id ? null : provider.id))
                        }
                        aria-label={showKey ? "Hide API key" : "Show API key"}
                        type="button"
                      >
                        {showKey ? (
                          <svg
                            width="14"
                            height="14"
                            viewBox="0 0 16 16"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="1.2"
                            aria-hidden="true"
                          >
                            <path d="M2 8s2.5-4 6-4 6 4 6 4-2.5 4-6 4-6-4-6-4z" />
                            <circle cx="8" cy="8" r="1.5" />
                            <line x1="2" y1="14" x2="14" y2="2" />
                          </svg>
                        ) : (
                          <svg
                            width="14"
                            height="14"
                            viewBox="0 0 16 16"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="1.2"
                            aria-hidden="true"
                          >
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
                    <span className="text-caption text-status-success font-body font-medium">
                      Key saved
                    </span>
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
                  <span
                    className={`text-caption font-body ${
                      isSuccess
                        ? "text-status-success"
                        : isError
                          ? "text-status-error"
                          : "text-text-muted"
                    }`}
                  >
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
