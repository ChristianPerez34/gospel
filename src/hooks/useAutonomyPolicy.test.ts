import { invoke } from "@tauri-apps/api/core";
import { act, renderHook } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AutonomyPolicyProvider, useAutonomyPolicy } from "./useAutonomyPolicy";

const mockState = vi.hoisted(() => ({
  policy: {
    mode: "ask" as "ask" | "auto",
    allowed_patterns: [] as string[],
    reject_patterns: ["rm *"],
  },
}));

vi.mock("@tauri-apps/api/core", () => {
  return {
    invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "get_approval_policy") return mockState.policy;
      if (cmd === "set_autonomy_mode") {
        mockState.policy = { ...mockState.policy, mode: args?.mode as "ask" | "auto" };
        return mockState.policy;
      }
      if (cmd === "add_approval_allow_rule") {
        mockState.policy = {
          ...mockState.policy,
          allowed_patterns: [...mockState.policy.allowed_patterns, args?.pattern as string],
        };
        return mockState.policy;
      }
      if (cmd === "add_approval_reject_rule") {
        mockState.policy = {
          ...mockState.policy,
          reject_patterns: [...mockState.policy.reject_patterns, args?.pattern as string],
        };
        return mockState.policy;
      }
      if (cmd === "remove_approval_rule") {
        const pattern = args?.pattern as string;
        mockState.policy = {
          ...mockState.policy,
          allowed_patterns: mockState.policy.allowed_patterns.filter((p) => p !== pattern),
          reject_patterns: mockState.policy.reject_patterns.filter((p) => p !== pattern),
        };
        return mockState.policy;
      }
      if (cmd === "clear_approval_rules") {
        mockState.policy = { ...mockState.policy, allowed_patterns: [] };
        return mockState.policy;
      }
      return null;
    }),
  };
});

describe("useAutonomyPolicy", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockState.policy = {
      mode: "ask",
      allowed_patterns: [],
      reject_patterns: ["rm *"],
    };
  });
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

  it("shares mode updates across consumers in one provider and fetches policy once", async () => {
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(AutonomyPolicyProvider, null, children);

    const { result } = renderHook(
      () => ({
        consumerA: useAutonomyPolicy(),
        consumerB: useAutonomyPolicy(),
      }),
      { wrapper }
    );

    await act(async () => {});

    const fetchCalls = vi
      .mocked(invoke)
      .mock.calls.filter(([cmd]) => cmd === "get_approval_policy");
    expect(fetchCalls).toHaveLength(1);
    expect(result.current.consumerA.mode).toBe("ask");
    expect(result.current.consumerB.mode).toBe("ask");

    await act(async () => {
      await result.current.consumerA.setMode("auto");
    });

    expect(result.current.consumerA.mode).toBe("auto");
    expect(result.current.consumerB.mode).toBe("auto");
  });

  it("propagates rule mutations across consumers in one provider", async () => {
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(AutonomyPolicyProvider, null, children);

    const { result } = renderHook(
      () => ({
        consumerA: useAutonomyPolicy(),
        consumerB: useAutonomyPolicy(),
      }),
      { wrapper }
    );

    await act(async () => {});

    await act(async () => {
      await result.current.consumerA.addAllowRule("bun run test");
    });
    expect(result.current.consumerB.policy.allowed_patterns).toContain("bun run test");

    await act(async () => {
      await result.current.consumerA.addRejectRule("sudo *");
    });
    expect(result.current.consumerB.policy.reject_patterns).toContain("sudo *");

    await act(async () => {
      await result.current.consumerA.removeRule("sudo *");
    });
    expect(result.current.consumerB.policy.reject_patterns).not.toContain("sudo *");

    await act(async () => {
      await result.current.consumerA.clearRules();
    });
    expect(result.current.consumerB.policy.allowed_patterns).toEqual([]);
  });

  it("reuses the parent context for nested AutonomyPolicyProviders", async () => {
    let outerPolicy: ReturnType<typeof useAutonomyPolicy> | undefined;
    let innerPolicy: ReturnType<typeof useAutonomyPolicy> | undefined;

    function OuterConsumer({ children }: { children: ReactNode }) {
      outerPolicy = useAutonomyPolicy();
      return createElement(AutonomyPolicyProvider, null, children);
    }

    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(AutonomyPolicyProvider, null, createElement(OuterConsumer, null, children));

    renderHook(
      () => {
        innerPolicy = useAutonomyPolicy();
        return innerPolicy;
      },
      { wrapper }
    );

    await act(async () => {});

    const fetchCalls = vi
      .mocked(invoke)
      .mock.calls.filter(([cmd]) => cmd === "get_approval_policy");
    expect(fetchCalls).toHaveLength(1);
    expect(outerPolicy?.mode).toBe("ask");
    expect(innerPolicy?.mode).toBe("ask");

    await act(async () => {
      await innerPolicy?.setMode("auto");
    });

    expect(outerPolicy?.mode).toBe("auto");
    expect(innerPolicy?.mode).toBe("auto");
  });
});
