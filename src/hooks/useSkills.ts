import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";

export interface SkillSummary {
  name: string;
  description: string;
  source: "Workspace" | "Global";
  scripts: string[];
  user_invocable: boolean;
  argument_hint?: string;
}

export function useSkills(workspacePath?: string) {
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const requestIdRef = useRef(0);

  const fetchSkills = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    try {
      const result = await invoke<SkillSummary[]>("list_skills");
      if (requestId === requestIdRef.current) {
        setSkills(result.filter((s) => s.user_invocable));
      }
    } catch (e) {
      console.warn("Failed to load skills:", e);
      if (requestId === requestIdRef.current) {
        setSkills([]);
      }
    } finally {
      if (requestId === requestIdRef.current) {
        setLoading(false);
      }
    }
  }, []);

  const reloadSkills = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    try {
      const result = await invoke<SkillSummary[]>("reload_skills");
      if (requestId === requestIdRef.current) {
        setSkills(result.filter((s) => s.user_invocable));
      }
    } catch (e) {
      console.warn("Failed to reload skills:", e);
    } finally {
      if (requestId === requestIdRef.current) {
        setLoading(false);
      }
    }
  }, []);

  // The backend resolves skills against the active workspace; the path signals that change.
  // biome-ignore lint/correctness/useExhaustiveDependencies: Workspace path is an intentional trigger.
  useEffect(() => {
    fetchSkills();
  }, [fetchSkills, workspacePath]);

  return { skills, loading, reloadSkills };
}
