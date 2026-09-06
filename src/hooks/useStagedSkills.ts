import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type { SkillSummary } from "./useSkills";

export interface SkillHarvestPreview {
  skill: string;
  trainCount: number;
  selectionCount: number;
}

export interface StagedSkillSummary {
  name: string;
  staged: boolean;
  has_rejected: boolean;
  current_mean?: number | null;
  candidate_mean?: number | null;
  preview?: string | null;
  diff?: string | null;
}

export function useStagedSkills(workspacePath?: string) {
  const [stagedSkills, setStagedSkills] = useState<StagedSkillSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [optimizing, setOptimizing] = useState(false);
  const [harvest, setHarvest] = useState<SkillHarvestPreview | null>(null);
  const requestIdRef = useRef(0);

  const fetchStaged = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    try {
      const result = await invoke<StagedSkillSummary[]>("list_staged_skills");
      if (requestId === requestIdRef.current) {
        setStagedSkills(result);
        setError(null);
      }
    } catch (error) {
      console.warn("Failed to load staged skills:", error);
      if (requestId === requestIdRef.current) {
        setStagedSkills([]);
        setError(String(error));
      }
    } finally {
      if (requestId === requestIdRef.current) {
        setLoading(false);
      }
    }
  }, []);

  const previewHarvest = useCallback(async (skillName: string) => {
    const trimmed = skillName.trim();
    if (!trimmed) {
      setHarvest(null);
      return;
    }
    try {
      const result = await invoke<{
        skill: string;
        train: unknown[];
        selection: unknown[];
      }>("harvest_skill_optimization_tasks", { skillName: trimmed });
      setHarvest({
        skill: result.skill,
        trainCount: result.train.length,
        selectionCount: result.selection.length,
      });
    } catch (error) {
      setHarvest(null);
      setError(String(error));
    }
  }, []);

  const adopt = useCallback(
    async (skillName: string) => {
      try {
        await invoke<SkillSummary>("adopt_staged_skill", { skillName });
        setError(null);
        await fetchStaged();
      } catch (error) {
        setError(String(error));
        throw error;
      }
    },
    [fetchStaged],
  );

  const optimize = useCallback(
    async (
      skillName: string,
      selection: { provider: string; model: string; variant?: string | null },
    ) => {
      setOptimizing(true);
      try {
        await invoke("optimize_skill_from_sessions", {
          skillName,
          provider: selection.provider,
          model: selection.model,
          variant: selection.variant ?? null,
        });
        setError(null);
        await fetchStaged();
      } catch (error) {
        setError(String(error));
        throw error;
      } finally {
        setOptimizing(false);
      }
    },
    [fetchStaged],
  );

  // The backend resolves staging against the active workspace; the path signals that change.
  // biome-ignore lint/correctness/useExhaustiveDependencies: Workspace path is an intentional trigger.
  useEffect(() => {
    fetchStaged();
  }, [fetchStaged, workspacePath]);

  return {
    stagedSkills,
    loading,
    optimizing,
    error,
    harvest,
    previewHarvest,
    adopt,
    optimize,
    reload: fetchStaged,
  };
}
