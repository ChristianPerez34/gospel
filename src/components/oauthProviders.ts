import type { ProviderConfig, ProviderId } from "./ProviderSelector";

export function oauthProviderIds(providers: ProviderConfig[]): ProviderId[] {
  return providers
    .filter((provider) => provider.authType === "oauth")
    .map((provider) => provider.id);
}
