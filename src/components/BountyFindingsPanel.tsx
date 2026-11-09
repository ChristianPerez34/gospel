import { RefreshCw, X } from "lucide-react";
import { useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { useBountyFindings, type BountyVerdict } from "../hooks/useBountyFindings";

interface BountyFindingsPanelProps {
  onClose?: () => void;
}

const STATUS_ORDER = ["confirmed", "needs_review", "candidate", "rejected"];
const VERDICT_STATUSES = ["confirmed", "needs_review", "rejected"];

function verdictFor(verdicts: BountyVerdict[], id: string) {
  return verdicts.find((verdict) => verdict.id === id) ?? null;
}

export function BountyFindingsPanel({ onClose }: BountyFindingsPanelProps) {
  const [selectedRun, setSelectedRun] = useState<string | null>(null);
  const { findings, loading, error, reload, setVerdict, approveCheckpoint } =
    useBountyFindings(true, selectedRun);

  const rows = useMemo(() => {
    const signals = findings?.signals ?? [];
    return [...signals].sort((a, b) => {
      const verdictA = verdictFor(findings?.verdicts ?? [], a.id);
      const verdictB = verdictFor(findings?.verdicts ?? [], b.id);
      const rank = (status: string | undefined) => {
        const index = STATUS_ORDER.indexOf(status ?? "");
        return index === -1 ? STATUS_ORDER.length : index;
      };
      const diff = rank(verdictA?.status ?? a.status) - rank(verdictB?.status ?? b.status);
      return diff !== 0 ? diff : b.created_at.localeCompare(a.created_at);
    });
  }, [findings]);

  return (
    <aside
      className="fixed top-0 right-0 z-40 flex h-full w-[420px] max-w-[90vw] flex-col border-l border-border bg-card shadow-xl"
      aria-label="Bounty findings panel"
    >
      <header className="flex items-center justify-between gap-2 border-b border-border px-4 py-3">
        <div className="flex min-w-0 flex-col">
          <h2 className="text-sm font-semibold">Bounty findings</h2>
          {(findings?.runs.length ?? 0) > 1 ? (
            <select
              className="mt-0.5 max-w-full truncate rounded border border-border bg-background text-xs text-muted-foreground"
              value={selectedRun ?? findings?.runId ?? ""}
              onChange={(event) => setSelectedRun(event.target.value)}
              aria-label="Select run"
            >
              {(findings?.runs ?? []).map((run) => (
                <option key={run} value={run}>
                  {run}
                </option>
              ))}
            </select>
          ) : (
            <p className="truncate text-xs text-muted-foreground">
              {findings?.runId
                ? `.gospel/bounty/runs/${findings.runId}`
                : ".gospel/bounty/runs"}
            </p>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void reload()}
            disabled={loading}
            aria-label="Refresh findings"
          >
            <RefreshCw className={loading ? "animate-spin" : ""} size={16} />
            Refresh
          </Button>
          {onClose && (
            <Button
              variant="ghost"
              size="sm"
              onClick={onClose}
              aria-label="Close findings panel"
            >
              <X size={16} />
            </Button>
          )}
        </div>
      </header>

      <div className="flex-1 overflow-y-auto px-4 py-3 text-sm">
        {error && (
          <p className="mb-3 text-destructive" role="alert">
            {error}
          </p>
        )}
        {findings?.checkpointPending && (
          <div className="mb-4 rounded-md border border-amber-500/40 bg-amber-500/10 p-3">
            <p className="mb-2 text-xs font-semibold uppercase tracking-wide text-amber-600">
              Threat-model checkpoint pending
            </p>
            <p className="mb-2 text-xs text-muted-foreground">
              Broad testing is frozen: http_request is refused until you approve
              the checkpoint. Review threat-model.md, then approve.
            </p>
            <Button
              size="sm"
              onClick={() => void approveCheckpoint()}
              aria-label="Approve threat-model checkpoint"
            >
              Approve checkpoint
            </Button>
          </div>
        )}
        {!loading && rows.length === 0 && !error && (
          <p className="text-muted-foreground">
            No signals yet. Run a sweep with @bug-bounty-sweep to produce findings.
          </p>
        )}
        {findings?.report && (
          <details className="mb-4 rounded-md border border-border bg-background p-3">
            <summary className="cursor-pointer text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              Sweep report
            </summary>
            <pre className="mt-2 whitespace-pre-wrap text-xs">{findings.report}</pre>
          </details>
        )}
        <ul className="flex flex-col gap-2">
          {rows.map((signal) => {
            const verdict = verdictFor(findings?.verdicts ?? [], signal.id);
            const effectiveStatus = verdict?.status ?? signal.status;
            return (
              <li
                key={signal.id}
                className="rounded-md border border-border bg-background p-3"
              >
                <div className="mb-1 flex flex-wrap items-center gap-2 text-xs">
                  <span className="font-mono text-muted-foreground">{signal.kind}</span>
                  <span
                    className={`rounded px-1.5 py-0.5 font-medium ${
                      effectiveStatus === "confirmed"
                        ? "bg-destructive/15 text-destructive"
                        : effectiveStatus === "needs_review"
                          ? "bg-amber-500/15 text-amber-600"
                          : effectiveStatus === "rejected"
                            ? "bg-muted text-muted-foreground"
                            : "bg-primary/15 text-primary"
                    }`}
                  >
                    {effectiveStatus}
                    {verdict?.source === "user" ? " · you" : ""}
                  </span>
                  {verdict?.severity && (
                    <span className="text-muted-foreground">{verdict.severity}</span>
                  )}
                  <span className="ml-auto text-muted-foreground">
                    {signal.confidence}
                  </span>
                </div>
                <div className="mb-1 font-mono text-xs break-all">
                  {signal.method} {signal.endpoint}
                </div>
                <p className="text-sm">{signal.summary}</p>
                {verdict?.rationale && (
                  <p className="mt-1 text-xs text-muted-foreground">
                    triage: {verdict.rationale}
                  </p>
                )}
                <div className="mt-2 flex items-center justify-between gap-2">
                  <p className="text-xs text-muted-foreground">
                    {signal.evidence_ids.length > 0
                      ? `${signal.evidence_ids.length} evidence`
                      : "no evidence linked"}
                  </p>
                  <div className="flex items-center gap-1">
                    {VERDICT_STATUSES.map((status) => (
                      <Button
                        key={status}
                        variant="ghost"
                        size="sm"
                        className={`h-6 px-1.5 text-xs ${
                          effectiveStatus === status ? "bg-muted" : ""
                        }`}
                        onClick={() => void setVerdict(signal.id, status)}
                        aria-label={`Mark ${signal.id} ${status}`}
                      >
                        {status.replace("_", " ")}
                      </Button>
                    ))}
                  </div>
                </div>
              </li>
            );
          })}
        </ul>
      </div>
    </aside>
  );
}
