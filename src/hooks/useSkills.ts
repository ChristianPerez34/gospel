import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

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

  const fetchSkills = useCallback(async () => {
    setLoading(true);
    try {
      const result = await invoke<SkillSummary[]>("list_skills");
      setSkills(result.filter((s) => s.user_invocable));
    } catch (e) {
      console.warn("Failed to load skills:", e);
      setSkills([]);
    } finally {
      setLoading(false);
    }
  }, []);

  const reloadSkills = useCallback(async () => {
    setLoading(true);
    try {
      const result = await invoke<SkillSummary[]>("reload_skills");
      setSkills(result.filter((s) => s.user_invocable));
    } catch (e) {
      console.warn("Failed to reload skills:", e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchSkills();
  }, [fetchSkills, workspacePath]);

  return { skills, loading, reloadSkills };
}
