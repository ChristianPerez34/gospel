import type { ProviderConfig, ProviderId } from "./ProviderSelector";

export function oauthProviderIds(providers: ProviderConfig[]): ProviderId[] {
  return providers
    .filter((provider) => provider.authType === "oauth")
    .map((provider) => provider.id);
}

export async function registeredOauthProviderIds(
  invoke: <T>(cmd: string) => Promise<T>
): Promise<ProviderId[]> {
  const ids = await invoke<string[]>("list_oauth_providers");
  return ids as ProviderId[];
}
