import type { ProviderConfig, ProviderId } from "./ProviderSelector";

export interface OauthCopy {
  prompt: string;
  button: string;
  connecting: string;
}

export const oauthProviders: Record<string, OauthCopy> = {
  chatgpt: {
    prompt: "Sign in with your ChatGPT Plus/Pro account",
    button: "Sign in with OpenAI",
    connecting: "Connecting...",
  },
  github_copilot: {
    prompt: "Sign in with the GitHub account that has Copilot access",
    button: "Sign in with GitHub",
    connecting: "Connecting to GitHub...",
  },
};

const NEUTRAL_OAUTH_COPY: OauthCopy = {
  prompt: "Sign in to continue",
  button: "Sign in",
  connecting: "Connecting...",
};

export function oauthCopy(provider: Pick<ProviderConfig, "id">): OauthCopy {
  return oauthProviders[provider.id] ?? NEUTRAL_OAUTH_COPY;
}

export function oauthProviderIds(providers: ProviderConfig[]): ProviderId[] {
  return providers
    .filter((provider) => provider.authType === "oauth")
    .map((provider) => provider.id);
}
