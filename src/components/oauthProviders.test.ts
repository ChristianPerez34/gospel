import { describe, expect, it } from "vitest";
import { oauthProviderIds } from "./oauthProviders";
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
