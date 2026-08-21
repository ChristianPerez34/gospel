import { describe, expect, it } from "vitest";
import { oauthCopy, oauthProviderIds } from "./oauthProviders";
import type { ProviderConfig } from "./ProviderSelector";

function provider(id: ProviderConfig["id"], authType: ProviderConfig["authType"]): ProviderConfig {
  return {
    id,
    name: id,
    authType,
    credentialed: false,
    visible: true,
    modelFetchStatus: "not_credentialed",
    modelCount: 0,
    apiKey: "",
    enabled: true,
    status: "idle",
    testMessage: "",
    isOAuth: authType === "oauth",
  };
}

describe("oauthProviderIds", () => {
  it("lists Credentialed Provider OAuth rows from backend auth type", () => {
    expect(
      oauthProviderIds([
        provider("openai", "api_key"),
        provider("chatgpt", "oauth"),
        provider("github_copilot", "oauth"),
        provider("anthropic", "api_key"),
      ])
    ).toEqual(["chatgpt", "github_copilot"]);
  });

  it("does not invent OAuth providers that the availability snapshot omitted", () => {
    expect(oauthProviderIds([provider("openai", "api_key")])).toEqual([]);
  });

  it("lists a table-driven OAuth id from the availability snapshot", () => {
    expect(
      oauthProviderIds([
        provider("openai", "api_key"),
        provider("chatgpt", "oauth"),
        provider("github_copilot", "oauth"),
        provider("new_oauth", "oauth"),
      ])
    ).toEqual(["chatgpt", "github_copilot", "new_oauth"]);
  });
});

describe("oauthCopy", () => {
  it("uses ChatGPT metadata for chatgpt", () => {
    expect(oauthCopy(provider("chatgpt", "oauth"))).toEqual({
      prompt: "Sign in with your ChatGPT Plus/Pro account",
      button: "Sign in with OpenAI",
      connecting: "Connecting...",
    });
  });

  it("keeps GitHub Copilot-specific copy", () => {
    expect(oauthCopy(provider("github_copilot", "oauth"))).toEqual({
      prompt: "Sign in with the GitHub account that has Copilot access",
      button: "Sign in with GitHub",
      connecting: "Connecting to GitHub...",
    });
  });

  it("uses Grok metadata for grok", () => {
    expect(oauthCopy(provider("grok", "oauth"))).toEqual({
      prompt: "Sign in with your SuperGrok or X Premium+ account",
      button: "Sign in with Grok",
      connecting: "Connecting to Grok...",
    });
  });

  it("uses provider-neutral copy for unrecognized OAuth ids", () => {
    expect(oauthCopy(provider("new_oauth", "oauth"))).toEqual({
      prompt: "Sign in to continue",
      button: "Sign in",
      connecting: "Connecting...",
    });
  });
});
