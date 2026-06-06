import { useState, useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ModelOption } from "../types";
import type { ProviderConfig, ProviderId } from "../components/ProviderSelector";

interface ProviderAvailability {
  provider: ProviderId;
  display_name: string;
  auth_type: "api_key" | "oauth";
  credentialed: boolean;
  visible: boolean;
  model_fetch_status: string;
  model_count: number;
  error_kind?: string | null;
  error_detail?: string | null;
}

export interface ModelAvailabilitySnapshot {
  providers: ProviderAvailability[];
  available_models: { model: string; provider: string }[];
  empty_reason?: string | null;
  warnings: string[];
}

export interface SelectedModel {
  provider: string;
  model: string;
}

function modelOptionId(provider: string, model: string) {
  return `${provider.toLowerCase()}::${model}`;
}

function providerConfigFromAvailability(
  provider: ProviderAvailability,
  existing?: ProviderConfig,
): ProviderConfig {
  return {
    id: provider.provider,
    name: provider.display_name,
    authType: provider.auth_type,
    credentialed: provider.credentialed,
    visible: provider.visible,
    modelFetchStatus: provider.model_fetch_status,
    modelCount: provider.model_count,
    errorKind: provider.error_kind ?? undefined,
    errorDetail: provider.error_detail ?? undefined,
    apiKey: provider.credentialed ? "" : existing?.apiKey ?? "",
    enabled: provider.visible,
    status: existing?.status ?? (provider.credentialed ? "success" : "idle"),
    testMessage: existing?.testMessage ?? "",
    isOAuth: provider.auth_type === "oauth",
    isAuthenticated: provider.auth_type === "oauth" ? provider.credentialed : undefined,
  };
}

function buildModelOptions(
  models: { model: string; provider: string }[],
  providers: ProviderConfig[],
): ModelOption[] {
  return models.map((m) => {
    const provider = providers.find((p) => p.id === m.provider.toLowerCase() as ProviderId);
    return {
      id: modelOptionId(m.provider, m.model),
      name: m.model,
      provider: m.provider,
      configured: provider?.credentialed ?? true,
    };
  });
}

interface UseModelAvailabilityOptions {
  onError?: (message: string) => void;
  onSuccess?: (message: string) => void;
}

export function useModelAvailability({ onError, onSuccess }: UseModelAvailabilityOptions = {}) {
  const [models, setModels] = useState<ModelOption[]>([]);
  const [providers, setProviders] = useState<ProviderConfig[]>([]);
  const [availabilitySnapshot, setAvailabilitySnapshot] = useState<ModelAvailabilitySnapshot | null>(null);
  const [isRefreshingModels, setIsRefreshingModels] = useState(false);
  const [availableModels, setAvailableModels] = useState<{ model: string; provider: string }[]>([]);
  const [selectedModel, setSelectedModel] = useState<SelectedModel | null>(null);
  const isRefreshingModelsRef = useRef(false);

  const refreshModelAvailability = useCallback(async (forceRefresh = false) => {
    if (forceRefresh && isRefreshingModelsRef.current) return;
    if (forceRefresh) {
      setIsRefreshingModels(true);
      isRefreshingModelsRef.current = true;
    }
    try {
      const snapshot = await invoke<ModelAvailabilitySnapshot>("get_model_availability", { forceRefresh });
      setAvailabilitySnapshot(snapshot);
      setAvailableModels(snapshot.available_models);
      setProviders((current) =>
        snapshot.providers.map((provider) =>
          providerConfigFromAvailability(provider, current.find((p) => p.id === provider.provider)),
        ),
      );
      if (forceRefresh) {
        const failedProvider = snapshot.providers.find(
          (p) => p.error_kind || p.model_fetch_status === "failed",
        );
        if (failedProvider) {
          onError?.(`${failedProvider.display_name}: ${failedProvider.error_detail || "Model refresh failed."}`);
        } else {
          onSuccess?.("Models refreshed.");
        }
      }
    } catch (e) {
      if (forceRefresh) {
        onError?.(`Model refresh failed: ${e}`);
      } else {
        setAvailabilitySnapshot(null);
      }
    } finally {
      if (forceRefresh) {
        setIsRefreshingModels(false);
        isRefreshingModelsRef.current = false;
      }
    }
  }, [onError, onSuccess]);

  useEffect(() => {
    void refreshModelAvailability();
  }, [refreshModelAvailability]);

  useEffect(() => {
    const nextModels = buildModelOptions(availableModels, providers);
    setModels(nextModels);
    if (nextModels.length === 0 || availableModels.length === 0) {
      setSelectedModel(null);
      return;
    }
    setSelectedModel((prev) => {
      if (
        prev &&
        availableModels.some(
          (m) =>
            m.model === prev.model && m.provider.toLowerCase() === prev.provider.toLowerCase(),
        )
      ) {
        return prev;
      }
      return { provider: availableModels[0].provider, model: availableModels[0].model };
    });
  }, [availableModels, providers]);

  return {
    models,
    providers,
    setProviders,
    availabilitySnapshot,
    isRefreshingModels,
    selectedModel,
    setSelectedModel,
    refreshModelAvailability,
  };
}
