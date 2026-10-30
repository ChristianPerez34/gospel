import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useAutonomyPolicy } from "./useAutonomyPolicy";

vi.mock("@tauri-apps/api/core", () => {
  let mockPolicy = {
    mode: "ask",
    allowed_patterns: [] as string[],
    reject_patterns: ["rm *"],
  };

  return {
    invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "get_approval_policy") return mockPolicy;
      if (cmd === "set_autonomy_mode") {
        mockPolicy = { ...mockPolicy, mode: args?.mode as "ask" | "auto" };
        return mockPolicy;
      }
      if (cmd === "add_approval_allow_rule") {
        mockPolicy = {
          ...mockPolicy,
          allowed_patterns: [...mockPolicy.allowed_patterns, args?.pattern as string],
        };
        return mockPolicy;
      }
      if (cmd === "add_approval_reject_rule") {
        mockPolicy = {
          ...mockPolicy,
          reject_patterns: [...mockPolicy.reject_patterns, args?.pattern as string],
        };
        return mockPolicy;
      }
      if (cmd === "clear_approval_rules") {
        mockPolicy = { ...mockPolicy, allowed_patterns: [] };
        return mockPolicy;
      }
      return null;
    }),
  };
});

describe("useAutonomyPolicy", () => {
  it("initializes with policy from invoke and allows updating mode and rules", async () => {
    const { result } = renderHook(() => useAutonomyPolicy());

    await act(async () => {});

    expect(result.current.mode).toBe("ask");
    expect(result.current.policy.reject_patterns).toContain("rm *");

    await act(async () => {
      await result.current.setMode("auto");
    });
    expect(result.current.mode).toBe("auto");

    await act(async () => {
      await result.current.addAllowRule("bun test");
    });
    expect(result.current.policy.allowed_patterns).toContain("bun test");

    await act(async () => {
      await result.current.clearRules();
    });
    expect(result.current.policy.allowed_patterns).toHaveLength(0);
  });
});
