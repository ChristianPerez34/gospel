import { useState } from "react";
import { Button } from "@/components/ui/button";
import type { ApprovalRisk, TurnBlock } from "../types";

type ApprovalBlock = Extract<TurnBlock, { kind: "approval" }>;

interface ApprovalCardProps {
  block: ApprovalBlock;
  onResolve?: (id: string, decision: "approve" | "deny") => Promise<void>;
}

const RISK_LABEL: Record<ApprovalRisk, string> = {
  mutating: "Mutating",
  destructive: "Destructive",
  external_access: "External access",
};

const RISK_ACCENT: Record<ApprovalRisk, string> = {
  mutating: "text-accent-data",
  destructive: "text-status-error",
  external_access: "text-accent-signal",
};

const STATUS_LABEL: Record<ApprovalBlock["status"], string> = {
  pending: "Awaiting your decision",
  approved: "Allowed",
  denied: "Denied",
  timed_out: "Timed out",
};

function statusClass(status: ApprovalBlock["status"]): string {
  switch (status) {
    case "approved":
      return "text-status-success";
    case "denied":
      return "text-status-error";
    case "timed_out":
      return "text-text-muted";
    default:
      return "text-text-primary";
  }
}

export function ApprovalCard({ block, onResolve }: ApprovalCardProps) {
  const [busy, setBusy] = useState<"approve" | "deny" | null>(null);
  const isPending = block.status === "pending";
  const canResolve = isPending && Boolean(onResolve) && busy === null;

  const resolve = async (decision: "approve" | "deny") => {
    if (!onResolve) return;
    setBusy(decision);
    try {
      await onResolve(block.id, decision);
    } catch (error) {
      // The broker times out server-side; surface a console hint and let
      // the user retry. The card stays in `pending` until the next event.
      console.error("[ApprovalCard] failed to resolve approval", error);
    } finally {
      setBusy(null);
    }
  };

  return (
    <li
      className="approval-card grid gap-2 rounded-sm border border-surface-overlay bg-surface-elevated p-3"
      data-approval-id={block.id}
      data-approval-status={block.status}
      aria-label={`${block.title}: ${block.summary}`}
    >
      <div className="flex items-center gap-2">
        <span
          className={`shrink-0 font-mono text-caption font-semibold uppercase tracking-[0.04em] ${RISK_ACCENT[block.risk]}`}
          aria-hidden="true"
        >
          {RISK_LABEL[block.risk]}
        </span>
        <span
          className="min-w-0 truncate font-mono text-body-sm text-text-primary"
          title={block.summary}
        >
          {block.summary}
        </span>
        <span
          className={`ml-auto shrink-0 font-mono text-caption ${statusClass(block.status)}`}
          aria-live="polite"
        >
          {STATUS_LABEL[block.status]}
        </span>
      </div>
      <p className="m-0 text-body-sm text-text-secondary">
        <span className="font-mono text-caption text-text-muted">{block.toolName}</span>
        <span aria-hidden="true"> · </span>
        {block.reason}
      </p>
      {isPending && (
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            onClick={() => resolve("approve")}
            disabled={!canResolve}
            aria-busy={busy === "approve"}
            data-testid={`approval-allow-${block.id}`}
          >
            Allow
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => resolve("deny")}
            disabled={!canResolve}
            aria-busy={busy === "deny"}
            data-testid={`approval-deny-${block.id}`}
          >
            Deny
          </Button>
          <span className="font-mono text-caption text-text-muted">Auto-denies in 60s</span>
        </div>
      )}
    </li>
  );
}
