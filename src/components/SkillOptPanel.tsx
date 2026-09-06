import { RefreshCw, X } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { useStagedSkills } from "../hooks/useStagedSkills";

interface SkillOptPanelProps {
  workspacePath: string;
  provider: string;
  model: string;
  variant?: string | null;
  onClose?: () => void;
}

export function SkillOptPanel({
  workspacePath,
  provider,
  model,
  variant = null,
  onClose,
}: SkillOptPanelProps) {
  const {
    stagedSkills,
    loading,
    optimizing,
    error,
    harvest,
    previewHarvest,
    adopt,
    optimize,
    reload,
  } = useStagedSkills(workspacePath);
  const [skillName, setSkillName] = useState("");
  const trimmedName = skillName.trim();
  const harvestReady =
    harvest?.skill === trimmedName && (harvest?.selectionCount ?? 0) > 0;
  const canOptimize =
    Boolean(workspacePath && trimmedName && provider && model && harvestReady) &&
    !loading &&
    !optimizing;

  useEffect(() => {
    void previewHarvest(trimmedName);
  }, [previewHarvest, trimmedName]);

  return (
    <aside
      className="fixed top-0 right-0 z-40 flex h-full w-[420px] max-w-[90vw] flex-col border-l border-border bg-card shadow-xl"
      aria-label="Skill optimization panel"
    >
      <header className="flex items-center justify-between gap-2 border-b border-border px-4 py-3">
        <div className="flex flex-col">
          <h2 className="text-sm font-semibold">Staged skills</h2>
          <p className="text-xs text-muted-foreground">.gospel/skill-opt (adopt to .agents)</p>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void reload()}
            disabled={loading || !workspacePath}
            aria-label="Refresh staged skills"
          >
            <RefreshCw className={loading ? "animate-spin" : ""} size={16} />
            Refresh
          </Button>
          {onClose && (
            <Button variant="ghost" size="sm" onClick={onClose} aria-label="Close skill opt panel">
              <X size={16} />
            </Button>
          )}
        </div>
      </header>

      <div className="flex-1 overflow-y-auto px-4 py-3 text-sm">
        <div className="mb-4 flex items-end gap-2">
          <label className="flex min-w-0 flex-1 flex-col gap-1">
            <span className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              Skill to optimize
            </span>
            <input
              className="rounded-md border border-border bg-background px-2 py-1"
              value={skillName}
              onChange={(event) => setSkillName(event.target.value)}
              aria-label="Skill to optimize"
            />
          </label>
          <Button
            size="sm"
            disabled={!canOptimize}
            onClick={() =>
              void optimize(skillName.trim(), { provider, model, variant: variant ?? null })
            }
            aria-label={`Optimize ${skillName.trim() || "skill"}`}
          >
            {optimizing ? "Optimizing…" : "Optimize"}
          </Button>
        </div>
        {trimmedName && harvest?.skill === trimmedName && (
          <p className="mb-3 text-xs text-muted-foreground">
            {harvest.selectionCount > 0
              ? `${harvest.trainCount} train · ${harvest.selectionCount} hold-out`
              : "Need 2+ slash-invoked turns before Optimize can run."}
          </p>
        )}
        {error && (
          <p className="mb-3 text-destructive" role="alert">
            {error}
          </p>
        )}
        {!provider || !model ? (
          <p className="mb-3 text-muted-foreground">Select a session model to optimize.</p>
        ) : (
          <p className="mb-3 text-xs text-muted-foreground">
            Model {provider}/{model}
            {variant ? ` (${variant})` : ""}
          </p>
        )}
        {!workspacePath && <p className="text-muted-foreground">No active workspace selected.</p>}
        {workspacePath && stagedSkills.length === 0 && !loading && (
          <p className="text-muted-foreground">No staged skills yet.</p>
        )}
        <ul className="space-y-3">
          {stagedSkills.map((skill) => (
            <li key={skill.name} className="flex items-center justify-between gap-2">
              <div>
                <p>{skill.name}</p>
                <p className="text-xs text-muted-foreground">
                  {skill.staged ? "Ready to adopt" : "Rejected edits only"}
                  {typeof skill.current_mean === "number" &&
                  typeof skill.candidate_mean === "number"
                    ? ` · ${skill.current_mean.toFixed(2)} → ${skill.candidate_mean.toFixed(2)}`
                    : ""}
                </p>
                {skill.diff ? (
                  <pre className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap text-xs text-muted-foreground">
                    {skill.diff}
                  </pre>
                ) : (
                  skill.preview && (
                    <pre className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap text-xs text-muted-foreground">
                      {skill.preview}
                    </pre>
                  )
                )}
              </div>
              {skill.staged && (
                <Button
                  size="sm"
                  onClick={() => void adopt(skill.name)}
                  aria-label={`Adopt ${skill.name}`}
                >
                  Adopt
                </Button>
              )}
            </li>
          ))}
        </ul>
      </div>
    </aside>
  );
}
