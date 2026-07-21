import { invoke } from "@tauri-apps/api/core";
import { RefreshCw, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import type { PlanFile } from "../types";

interface PlanPanelProps {
  workspacePath: string;
  onClose?: () => void;
}

/**
 * Read-only preview of `.gospel/PLAN.md` for the active workspace.
 *
 * Spike artifact (Plan 021). Gated behind the `?panel=plan` debug flag in
 * `App.tsx`; not wired as a default-toggled overlay. Calls the
 * `read_harness_plan` Tauri command on workspace change and via an explicit
 * Refresh button. Editing and file-watch live updates are out of scope for
 * the spike — see the plan's Spike Findings / Open Questions.
 */
export function PlanPanel({ workspacePath, onClose }: PlanPanelProps) {
  const [plan, setPlan] = useState<PlanFile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    if (!workspacePath) {
      setPlan(null);
      setError(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const p = await invoke<PlanFile>("read_harness_plan", {
        activeWorkspacePath: workspacePath,
      });
      setPlan(p);
    } catch (e) {
      setError(String(e));
      setPlan(null);
    } finally {
      setLoading(false);
    }
  }, [workspacePath]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <aside
      className="fixed top-0 right-0 z-40 h-full w-[420px] max-w-[90vw] border-l border-border bg-card shadow-xl flex flex-col"
      aria-label="Plan panel (debug preview)"
    >
      <header className="flex items-center justify-between gap-2 border-b border-border px-4 py-3">
        <div className="flex flex-col">
          <h2 className="text-sm font-semibold">Plan</h2>
          <p className="text-xs text-muted-foreground">
            .gospel/PLAN.md (read-only spike)
          </p>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void refresh()}
            disabled={loading || !workspacePath}
            aria-label="Refresh plan"
          >
            <RefreshCw className={loading ? "animate-spin" : ""} size={16} />
            Refresh
          </Button>
          {onClose && (
            <Button
              variant="ghost"
              size="sm"
              onClick={onClose}
              aria-label="Close plan panel"
            >
              <X size={16} />
            </Button>
          )}
        </div>
      </header>

      <div className="flex-1 overflow-y-auto px-4 py-3 text-sm">
        {!workspacePath && (
          <p className="text-muted-foreground">
            No active workspace selected.
          </p>
        )}
        {error && (
          <p className="text-destructive" role="alert">
            Failed to read plan: {error}
          </p>
        )}
        {plan && !plan.hasPlanFile && (
          <p className="text-muted-foreground">No plan yet.</p>
        )}
        {plan && plan.hasPlanFile && (
          <div className="space-y-5">
            <section>
              <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                Goal
              </h3>
              <p className="mt-1 whitespace-pre-wrap">
                {plan.goal ?? "—"}
              </p>
            </section>

            <section>
              <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                Steps
              </h3>
              {plan.steps.length === 0 ? (
                <p className="mt-1 text-muted-foreground">—</p>
              ) : (
                <ul className="mt-1 space-y-1">
                  {plan.steps.map((step, i) => (
                    <li key={step.text + i} className="flex gap-2">
                      <span aria-hidden>{step.done ? "☑" : "☐"}</span>
                      <span
                        className={
                          step.done ? "line-through text-muted-foreground" : ""
                        }
                      >
                        {step.text}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </section>

            <section>
              <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                Evidence / Verification
              </h3>
              {plan.evidence.length === 0 ? (
                <p className="mt-1 text-muted-foreground">—</p>
              ) : (
                <ul className="mt-1 list-disc pl-5 space-y-1">
                  {plan.evidence.map((e, i) => (
                    <li key={e + i} className="whitespace-pre-wrap">
                      {e}
                    </li>
                  ))}
                </ul>
              )}
            </section>

            <section>
              <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                Open Questions / Risks
              </h3>
              {plan.openQuestions.length === 0 ? (
                <p className="mt-1 text-muted-foreground">—</p>
              ) : (
                <ul className="mt-1 list-disc pl-5 space-y-1">
                  {plan.openQuestions.map((q, i) => (
                    <li key={q + i} className="whitespace-pre-wrap">
                      {q}
                    </li>
                  ))}
                </ul>
              )}
            </section>

            <section>
              <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                Next Action
              </h3>
              <p className="mt-1 whitespace-pre-wrap">
                {plan.nextAction ?? "—"}
              </p>
            </section>
          </div>
        )}
      </div>
    </aside>
  );
}