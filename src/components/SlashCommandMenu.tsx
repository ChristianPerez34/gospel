import { useMemo } from "react";
import type { SkillSummary } from "../hooks/useSkills";
import { levenshtein } from "../utils/levenshtein";

interface SlashCommandMenuProps {
  skills: SkillSummary[];
  filter: string;
  visible: boolean;
  onSelect: (name: string) => void;
  onReload?: () => void;
}

export function SlashCommandMenu({
  skills,
  filter,
  visible,
  onSelect,
  onReload,
}: SlashCommandMenuProps) {
  const { filtered, suggestion } = useMemo(() => {
    const lower = filter.toLowerCase();
    const matched = skills.filter((s) =>
      s.name.toLowerCase().startsWith(lower)
    );

    if (matched.length > 0 || lower.length === 0) {
      return { filtered: matched, suggestion: null as string | null };
    }

    let bestName = "";
    let bestDist = Infinity;
    for (const s of skills) {
      const dist = levenshtein(lower, s.name.toLowerCase());
      if (dist < bestDist) {
        bestDist = dist;
        bestName = s.name;
      }
    }

    if (bestDist <= 3 && bestName) {
      return { filtered: [], suggestion: bestName };
    }

    return { filtered: [], suggestion: null };
  }, [skills, filter]);

  if (!visible) return null;

  if (skills.length === 0) {
    return (
      <div className="absolute bottom-full left-0 mb-1 w-[480px] max-w-full bg-surface-elevated border border-surface-overlay rounded-md shadow-lg z-[--z-palette] p-3">
        <div className="text-text-muted text-body-sm">
          No skills found. Add a SKILL.md to{" "}
          <code className="text-text-secondary">&lt;workspace&gt;/.agents/skills/&lt;name&gt;/</code>{" "}
          to get started.
        </div>
        {onReload && (
          <button
            className="mt-2 min-h-11 text-accent-action text-caption hover:underline"
            onClick={onReload}
            type="button"
          >
            Reload skills
          </button>
        )}
      </div>
    );
  }

  if (suggestion) {
    return (
      <div className="absolute bottom-full left-0 mb-1 w-[480px] max-w-full bg-surface-elevated border border-surface-overlay rounded-md shadow-lg z-[--z-palette]">
        <button
          className="flex min-h-11 items-center justify-between w-full px-3 text-left hover:bg-surface-overlay transition-colors duration-150"
          onClick={() => onSelect(suggestion)}
          type="button"
        >
          <span className="text-body-sm text-text-primary">
            Did you mean: <span className="font-semibold">/{suggestion}</span>?
          </span>
          <span className="font-mono text-caption text-text-muted">Tab</span>
        </button>
      </div>
    );
  }

  if (filtered.length === 0) return null;

  return (
    <div className="absolute bottom-full left-0 mb-1 w-[480px] max-w-full max-h-[200px] overflow-y-auto bg-surface-elevated border border-surface-overlay rounded-md shadow-lg z-[--z-palette]">
      {filtered.map((skill) => (
        <button
          key={skill.name}
          className="flex min-h-11 flex-col justify-center w-full px-3 text-left hover:bg-surface-overlay transition-colors duration-150"
          onClick={() => onSelect(skill.name)}
          type="button"
        >
          <span className="text-body-sm text-text-primary font-mono">
            /{skill.name}
          </span>
          <span className="text-caption text-text-muted line-clamp-1">
            {skill.description}
          </span>
        </button>
      ))}
    </div>
  );
}
