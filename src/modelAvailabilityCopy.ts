export interface ProviderAvailabilityCopySource {
  display_name: string;
  credentialed: boolean;
  visible: boolean;
  model_fetch_status: string;
  error_kind?: string | null;
  error_detail?: string | null;
}

export interface ModelAvailabilityCopySource {
  empty_reason?: string | null;
  providers: ProviderAvailabilityCopySource[];
}

export interface NoModelCopy {
  title: string;
  detail: string;
  actionLabel: string;
}

export function noModelCopy(snapshot: ModelAvailabilityCopySource | null): NoModelCopy {
  if (!snapshot) {
    return {
      title: "Loading models",
      detail: "Checking provider credentials and model availability.",
      actionLabel: "Open Settings",
    };
  }

  switch (snapshot.empty_reason) {
    case "no_credentialed_providers":
      return {
        title: "Add provider credentials",
        detail: "No provider has credentials yet. Add an API key or sign in to ChatGPT Plus/Pro.",
        actionLabel: "Open Settings",
      };
    case "all_credentialed_providers_hidden":
      return {
        title: "Show a provider",
        detail: "All credentialed providers are hidden from the model picker.",
        actionLabel: "Open Settings",
      };
    case "model_fetch_failed": {
      const failed = snapshot.providers.find((p) => p.visible && p.credentialed && p.model_fetch_status === "failed");
      return {
        title: "Could not load models",
        detail: failed?.error_detail || "A credentialed provider failed while loading models.",
        actionLabel: failed?.error_kind === "auth_failed" ? "Sign in again" : "Open Settings",
      };
    }
    case "no_visible_provider_models":
      return {
        title: "No models returned",
        detail: "Visible credentialed providers loaded successfully but returned no selectable models.",
        actionLabel: "Open Settings",
      };
    default:
      return {
        title: "No available models",
        detail: "Open Settings to add credentials or show a provider in the model picker.",
        actionLabel: "Open Settings",
      };
  }
}
