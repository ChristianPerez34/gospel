import { Check, ChevronDown, ChevronUp, Copy } from "lucide-react";
import { useCallback, useState } from "react";
import { Button } from "@/components/ui/button";
import type { ApprovalDecision, ApprovalRisk, TurnBlock } from "../types";

type ApprovalBlock = Extract<TurnBlock, { kind: "approval" }>;

interface ApprovalCardProps {
  block: ApprovalBlock;
  onResolve?: (id: string, decision: ApprovalDecision) => Promise<void>;
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
  auto_approved: "Auto-allowed",
  denied: "Denied",
  auto_denied: "Auto-rejected",
  timed_out: "Timed out",
};

function statusClass(status: ApprovalBlock["status"]): string {
  switch (status) {
    case "approved":
    case "auto_approved":
      return "text-status-success";
    case "denied":
    case "auto_denied":
      return "text-status-error";
    case "timed_out":
      return "text-text-muted";
    default:
      return "text-text-primary";
  }
}

export function ApprovalCard({ block, onResolve }: ApprovalCardProps) {
  const [busy, setBusy] = useState<ApprovalDecision | null>(null);
  const [copied, setCopied] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const isPending = block.status === "pending";
  const canResolve = isPending && Boolean(onResolve) && busy === null;
  const isDestructive = block.risk === "destructive";
  const canAlwaysAllow =
    block.approvalKind === "command" && !isDestructive && block.workspaceScoped !== false;

  const isLongSummary = block.summary.length > 140 || block.summary.includes("\n");

  const resolve = async (decision: ApprovalDecision) => {
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

  const copySummary = useCallback(async () => {
    try {
      if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(block.summary);
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      }
    } catch (error) {
      console.error("[ApprovalCard] failed to copy summary", error);
    }
  }, [block.summary]);

  return (
    <li
      className="approval-card grid min-w-0 gap-2.5 rounded-lg border border-surface-overlay bg-surface-elevated p-3 transition-[opacity,transform,border-color] duration-200 ease-out"
      data-approval-id={block.id}
      data-approval-status={block.status}
      aria-label={`${block.title}: ${block.summary}`}
    >
      <div className="flex min-w-0 items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-1.5">
          <span
            className={`shrink-0 font-mono text-caption font-semibold uppercase tracking-[0.04em] ${RISK_ACCENT[block.risk]}`}
            aria-hidden="true"
          >
            {RISK_LABEL[block.risk]}
          </span>
          <span className="shrink-0 font-mono text-caption text-text-muted" aria-hidden="true">
            ·
          </span>
          <span className="truncate font-mono text-caption text-text-muted">{block.toolName}</span>
        </div>
        <span
          className={`shrink-0 font-mono text-caption ${statusClass(block.status)}`}
          aria-live="polite"
        >
          {STATUS_LABEL[block.status]}
        </span>
      </div>

      <div className="group/cmd relative min-w-0 rounded-md border border-surface-overlay/80 bg-surface-base p-2.5 font-mono text-body-sm text-text-primary">
        <div
          className={`${
            expanded ? "max-h-none" : "max-h-48"
          } overflow-y-auto overflow-x-hidden break-words [overflow-wrap:anywhere] whitespace-pre-wrap select-text ${
            isLongSummary ? "pr-14" : "pr-7"
          } font-mono text-mono transition-[max-height] duration-200 ease-out`}
          title={block.summary}
        >
          {block.summary}
        </div>
        <div className="absolute right-1.5 top-1.5 flex items-center gap-0.5">
          {isLongSummary && (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="flex h-6 w-6 items-center justify-center rounded text-text-muted opacity-80 transition-all duration-150 ease-out hover:bg-surface-overlay hover:text-text-primary hover:opacity-100 focus-visible:opacity-100 group-hover/cmd:opacity-100 active:scale-[0.95]"
              title={expanded ? "Collapse command" : "Expand full command"}
              aria-label={expanded ? "Collapse command" : "Expand full command"}
              aria-expanded={expanded}
            >
              {expanded ? (
                <ChevronUp className="h-3.5 w-3.5" />
              ) : (
                <ChevronDown className="h-3.5 w-3.5" />
              )}
            </button>
          )}
          <button
            type="button"
            onClick={copySummary}
            className="flex h-6 w-6 items-center justify-center rounded text-text-muted opacity-0 transition-all duration-150 ease-out hover:bg-surface-overlay hover:text-text-primary focus-visible:opacity-100 group-hover/cmd:opacity-100 active:scale-[0.95]"
            title={copied ? "Copied" : "Copy command"}
            aria-label={copied ? "Copied" : "Copy command"}
          >
            {copied ? (
              <Check className="h-3.5 w-3.5 text-status-success" />
            ) : (
              <Copy className="h-3.5 w-3.5" />
            )}
          </button>
        </div>
      </div>

      <p className="m-0 text-body-sm text-text-secondary">{block.reason}</p>

      {isPending && (
        <div className="flex flex-wrap items-center gap-2 pt-0.5">
          <Button
            size="sm"
            onClick={() => resolve("approve")}
            disabled={!canResolve}
            aria-busy={busy === "approve"}
            data-testid={`approval-allow-${block.id}`}
            className="transition-transform duration-150 ease-out active:scale-[0.97]"
          >
            Allow
          </Button>
          {canAlwaysAllow && (
            <Button
              size="sm"
              variant="secondary"
              onClick={() => resolve("always_allow")}
              disabled={!canResolve}
              aria-busy={busy === "always_allow"}
              data-testid={`approval-always-allow-${block.id}`}
              className="transition-transform duration-150 ease-out active:scale-[0.97]"
              title="Automatically execute this command in the future without asking"
            >
              Always Allow
            </Button>
          )}
          <Button
            size="sm"
            variant="outline"
            onClick={() => resolve("deny")}
            disabled={!canResolve}
            aria-busy={busy === "deny"}
            data-testid={`approval-deny-${block.id}`}
            className="transition-transform duration-150 ease-out active:scale-[0.97]"
          >
            Deny
          </Button>
          <span className="ml-auto font-mono text-caption text-text-muted">
            {isDestructive ? "Guarded · Auto-denies in 60s" : "Auto-denies in 60s"}
          </span>
        </div>
      )}
    </li>
  );
}
