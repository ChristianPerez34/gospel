import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ProviderConfig, ProviderId } from "../components/ProviderSelector";
import { type AvailableModelVariant, type ModelOption, modelOptionId } from "../types";

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
  available_models: AvailableModel[];
  empty_reason?: string | null;
  warnings: string[];
}

export interface SelectedModel {
  provider: string;
  model: string;
  variant?: string | null;
}

export interface AvailableModel {
  model: string;
  provider: string;
  variants?: AvailableModelVariant[];
}

const DEFAULT_STARTUP_MODEL = {
  provider: "chatgpt",
  model: "gpt-5.6-sol",
} as const;

export function defaultStartupModel(availableModels: AvailableModel[]): AvailableModel | undefined {
  return (
    availableModels.find(
      (model) =>
        model.provider.toLowerCase() === DEFAULT_STARTUP_MODEL.provider &&
        model.model === DEFAULT_STARTUP_MODEL.model
    ) ?? availableModels[0]
  );
}

function providerConfigFromAvailability(
  provider: ProviderAvailability,
  existing?: ProviderConfig
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
    apiKey: provider.credentialed ? "" : (existing?.apiKey ?? ""),
    enabled: provider.visible,
    status: existing?.status ?? (provider.credentialed ? "success" : "idle"),
    testMessage: existing?.testMessage ?? "",
    isOAuth: provider.auth_type === "oauth",
    isAuthenticated: provider.auth_type === "oauth" ? provider.credentialed : undefined,
  };
}

function buildModelOptions(models: AvailableModel[], providers: ProviderConfig[]): ModelOption[] {
  return models.map((m) => {
    const provider = providers.find((p) => p.id === (m.provider.toLowerCase() as ProviderId));
    return {
      id: modelOptionId(m.provider, m.model),
      name: m.model,
      provider: m.provider,
      model: m.model,
      variant: null,
      configured: provider?.credentialed ?? true,
      variants: m.variants ?? [],
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
  const [availabilitySnapshot, setAvailabilitySnapshot] =
    useState<ModelAvailabilitySnapshot | null>(null);
  const [isRefreshingModels, setIsRefreshingModels] = useState(false);
  const [availableModels, setAvailableModels] = useState<AvailableModel[]>([]);
  const [selectedModel, setSelectedModel] = useState<SelectedModel | null>(null);
  const isRefreshingModelsRef = useRef(false);
  const onErrorRef = useRef(onError);
  const onSuccessRef = useRef(onSuccess);
  onErrorRef.current = onError;
  onSuccessRef.current = onSuccess;

  const refreshModelAvailability = useCallback(async (forceRefresh = false) => {
    if (forceRefresh && isRefreshingModelsRef.current) return;
    if (forceRefresh) {
      setIsRefreshingModels(true);
      isRefreshingModelsRef.current = true;
    }
    try {
      const snapshot = await invoke<ModelAvailabilitySnapshot>("get_model_availability", {
        forceRefresh,
      });
      setAvailabilitySnapshot(snapshot);
      setAvailableModels(snapshot.available_models);
      setProviders((current) =>
        snapshot.providers.map((provider) =>
          providerConfigFromAvailability(
            provider,
            current.find((p) => p.id === provider.provider)
          )
        )
      );
      if (forceRefresh) {
        const failedProvider = snapshot.providers.find(
          (p) => p.error_kind || p.model_fetch_status === "failed"
        );
        if (failedProvider) {
          onErrorRef.current?.(
            `${failedProvider.display_name}: ${failedProvider.error_detail || "Model refresh failed."}`
          );
        } else {
          onSuccessRef.current?.("Models refreshed.");
        }
      }
    } catch (e) {
      if (forceRefresh) {
        onErrorRef.current?.(`Model refresh failed: ${e}`);
      } else {
        setAvailabilitySnapshot(null);
      }
    } finally {
      if (forceRefresh) {
        setIsRefreshingModels(false);
        isRefreshingModelsRef.current = false;
      }
    }
  }, []);

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
      if (prev) {
        const available = availableModels.find(
          (m) => m.model === prev.model && m.provider.toLowerCase() === prev.provider.toLowerCase()
        );
        if (available) {
          const variant =
            prev.variant && available.variants?.some((v) => v.id === prev.variant)
              ? prev.variant
              : null;
          return { provider: available.provider, model: available.model, variant };
        }
      }
      const defaultModel = defaultStartupModel(availableModels);
      return defaultModel
        ? { provider: defaultModel.provider, model: defaultModel.model, variant: null }
        : null;
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
