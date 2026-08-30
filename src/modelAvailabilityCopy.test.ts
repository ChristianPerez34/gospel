import { describe, expect, it } from "vitest";
import { noModelCopy } from "./modelAvailabilityCopy";

describe("noModelCopy", () => {
  it("directs entitlement failures to Settings instead of Sign in again", () => {
    const copy = noModelCopy({
      empty_reason: "model_fetch_failed",
      providers: [
        {
          display_name: "Grok",
          credentialed: true,
          visible: true,
          model_fetch_status: "failed",
          error_kind: "entitlement_failed",
          error_detail:
            "Account is signed in but not entitled for model access. Signing in again will not fix this — upgrade SuperGrok / X Premium+ or use an xAI API key.",
        },
      ],
    });

    expect(copy.title).toBe("Subscription not entitled");
    expect(copy.detail).toContain("Signing in again will not fix this");
    expect(copy.actionLabel).toBe("Open Settings");
  });

  it("still directs auth failures to Sign in again", () => {
    const copy = noModelCopy({
      empty_reason: "model_fetch_failed",
      providers: [
        {
          display_name: "Grok",
          credentialed: true,
          visible: true,
          model_fetch_status: "failed",
          error_kind: "auth_failed",
          error_detail: "Provider credentials need attention.",
        },
      ],
    });

    expect(copy.actionLabel).toBe("Sign in again");
  });
});
