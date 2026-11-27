import { invoke } from "@tauri-apps/api/core";
import {
  createContext,
  createElement,
  Fragment,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import type { ApprovalPolicy, AutonomyMode } from "../types";

export interface UseAutonomyPolicyReturn {
  policy: ApprovalPolicy;
  mode: AutonomyMode;
  loading: boolean;
  setMode: (mode: AutonomyMode) => Promise<void>;
  addAllowRule: (pattern: string) => Promise<void>;
  addRejectRule: (pattern: string) => Promise<void>;
  removeRule: (pattern: string) => Promise<void>;
  clearRules: () => Promise<void>;
  refresh: () => Promise<void>;
}

const DEFAULT_POLICY: ApprovalPolicy = {
  mode: "ask",
  allowed_patterns: [],
  reject_patterns: ["rm *", "git push *--force*", "git reset*", "git clean*"],
};

const AutonomyPolicyContext = createContext<UseAutonomyPolicyReturn | null>(null);

function useAutonomyPolicyState(enabled: boolean): UseAutonomyPolicyReturn {
  const [policy, setPolicy] = useState<ApprovalPolicy>(DEFAULT_POLICY);
  const [loading, setLoading] = useState(enabled);

  const refresh = useCallback(async () => {
    try {
      const current = await invoke<ApprovalPolicy>("get_approval_policy");
      if (current && typeof current === "object" && "mode" in current) {
        setPolicy(current);
      }
    } catch (e) {
      console.error("[useAutonomyPolicy] failed to fetch policy:", e);
    }
  }, []);

  useEffect(() => {
    if (!enabled) return;
    setLoading(true);
    void refresh().finally(() => setLoading(false));
  }, [enabled, refresh]);

  const setMode = useCallback(async (mode: AutonomyMode) => {
    try {
      const updated = await invoke<ApprovalPolicy>("set_autonomy_mode", { mode });
      if (updated && typeof updated === "object" && "mode" in updated) {
        setPolicy(updated);
      } else {
        setPolicy((prev) => ({ ...(prev ?? DEFAULT_POLICY), mode }));
      }
    } catch (e) {
      console.error("[useAutonomyPolicy] failed to set mode:", e);
    }
  }, []);

  const addRule = useCallback(async (kind: "allow" | "reject", pattern: string) => {
    try {
      const updated = await invoke<ApprovalPolicy>(`add_approval_${kind}_rule`, { pattern });
      if (updated && typeof updated === "object") {
        setPolicy(updated);
      } else {
        const field = kind === "allow" ? "allowed_patterns" : "reject_patterns";
        setPolicy((prev) => ({
          ...(prev ?? DEFAULT_POLICY),
          [field]: [...(prev?.[field] ?? []), pattern],
        }));
      }
    } catch (e) {
      console.error(`[useAutonomyPolicy] failed to add ${kind} rule:`, e);
    }
  }, []);

  const addAllowRule = useCallback((pattern: string) => addRule("allow", pattern), [addRule]);

  const addRejectRule = useCallback((pattern: string) => addRule("reject", pattern), [addRule]);

  const removeRule = useCallback(async (pattern: string) => {
    try {
      const updated = await invoke<ApprovalPolicy>("remove_approval_rule", { pattern });
      if (updated && typeof updated === "object") {
        setPolicy(updated);
      }
    } catch (e) {
      console.error("[useAutonomyPolicy] failed to remove rule:", e);
    }
  }, []);

  const clearRules = useCallback(async () => {
    try {
      const updated = await invoke<ApprovalPolicy>("clear_approval_rules");
      if (updated && typeof updated === "object") {
        setPolicy(updated);
      } else {
        setPolicy((prev) => ({ ...(prev ?? DEFAULT_POLICY), allowed_patterns: [] }));
      }
    } catch (e) {
      console.error("[useAutonomyPolicy] failed to clear rules:", e);
    }
  }, []);

  const safePolicy = policy ?? DEFAULT_POLICY;

  return useMemo(
    () => ({
      policy: safePolicy,
      mode: safePolicy.mode ?? "ask",
      loading,
      setMode,
      addAllowRule,
      addRejectRule,
      removeRule,
      clearRules,
      refresh,
    }),
    [safePolicy, loading, setMode, addAllowRule, addRejectRule, removeRule, clearRules, refresh]
  );
}

export function AutonomyPolicyProvider({ children }: { children: ReactNode }) {
  const parent = useContext(AutonomyPolicyContext);
  const value = useAutonomyPolicyState(parent === null);
  if (parent !== null) {
    return createElement(Fragment, null, children);
  }
  return createElement(AutonomyPolicyContext.Provider, { value }, children);
}

export function useAutonomyPolicy(): UseAutonomyPolicyReturn {
  const context = useContext(AutonomyPolicyContext);
  const fallback = useAutonomyPolicyState(context === null);
  return context ?? fallback;
}
