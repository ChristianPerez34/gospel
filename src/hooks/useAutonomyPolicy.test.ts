import { invoke } from "@tauri-apps/api/core";
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
  const ruleKinds = [
    { method: "addAllowRule", command: "add_approval_allow_rule", field: "allowed_patterns" },
    { method: "addRejectRule", command: "add_approval_reject_rule", field: "reject_patterns" },
  ] as const;

  it.each(ruleKinds)("uses the backend policy for $method", async ({ method, command, field }) => {
    const { result } = renderHook(() => useAutonomyPolicy());
    await act(async () => {});
    const updated = { ...result.current.policy, [field]: ["canonical rule"] };
    vi.mocked(invoke).mockResolvedValueOnce(updated);

    await act(async () => {
      await result.current[method]("raw rule");
    });

    expect(invoke).toHaveBeenLastCalledWith(command, { pattern: "raw rule" });
    expect(result.current.policy).toEqual(updated);
  });

  it.each(ruleKinds)("preserves the other rules in the $method fallback", async ({
    method,
    field,
  }) => {
    const { result } = renderHook(() => useAutonomyPolicy());
    await act(async () => {});
    const previous = result.current.policy;
    vi.mocked(invoke).mockResolvedValueOnce(null);

    await act(async () => {
      await result.current[method]("new rule");
    });

    expect(result.current.policy).toEqual({
      ...previous,
      [field]: [...previous[field], "new rule"],
    });
  });

  it.each(ruleKinds)("keeps policy unchanged when $method fails", async ({ method }) => {
    const { result } = renderHook(() => useAutonomyPolicy());
    await act(async () => {});
    const previous = result.current.policy;
    const error = new Error("Policy update failed");
    vi.mocked(invoke).mockRejectedValueOnce(error);
    const diagnostic = vi.spyOn(console, "error").mockImplementation(() => {});

    try {
      await act(async () => {
        await result.current[method]("new rule");
      });

      expect(result.current.policy).toEqual(previous);
      expect(diagnostic).toHaveBeenCalledWith(expect.any(String), error);
    } finally {
      diagnostic.mockRestore();
    }
  });

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
