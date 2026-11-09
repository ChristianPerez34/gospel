import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface BountySignal {
  id: string;
  created_at: string;
  kind: string;
  endpoint: string;
  method: string;
  summary: string;
  hypothesis?: string | null;
  evidence_ids: string[];
  confidence: string;
  status: string;
}

export interface BountyVerdict {
  id: string;
  status: string;
  severity?: string | null;
  rationale?: string | null;
  source?: string;
}

export interface BountyFindings {
  runId: string | null;
  runs: string[];
  checkpointPending: boolean;
  signals: BountySignal[];
  verdicts: BountyVerdict[];
  report: string | null;
}

export function useBountyFindings(enabled: boolean, runId: string | null = null) {
  const [findings, setFindings] = useState<BountyFindings | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<BountyFindings>("list_bounty_findings", {
        runId,
      });
      setFindings(result);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [runId]);

  const approveCheckpoint = useCallback(async () => {
    const activeRunId = findings?.runId;
    if (!activeRunId) return;
    await invoke("approve_bounty_checkpoint", { runId: activeRunId });
    await reload();
  }, [findings?.runId, reload]);

  const setVerdict = useCallback(
    async (signalId: string, status: string) => {
      const activeRunId = findings?.runId;
      if (!activeRunId) return;
      await invoke("set_bounty_verdict", {
        runId: activeRunId,
        signalId,
        status,
      });
      await reload();
    },
    [findings?.runId, reload],
  );

  useEffect(() => {
    if (enabled) void reload();
  }, [enabled, reload]);

  return { findings, loading, error, reload, setVerdict, approveCheckpoint };
}
