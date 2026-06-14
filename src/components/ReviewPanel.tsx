import { useMemo, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { normalize, resolve } from "@tauri-apps/api/path";
import { openPath } from "@tauri-apps/plugin-opener";
import type {
  ReviewComment,
  ReviewOutcome,
  ReviewOutcomeOutput,
  ReviewResult,
  SignalTier,
} from "../types";
import {
  buildExternalAgentFindingPrompt,
  buildGospelFixFindingPrompt,
  isActionableReviewFinding,
} from "../reviewPrompts";

type ReviewPanelMode = "local" | "pr" | "scan";

interface ReviewPanelProps {
  open: boolean;
  provider?: string;
  model?: string;
  workspacePath?: string;
  canSendTurn?: boolean;
  onClose: () => void;
  onError?: (message: string) => void;
  onSuccess?: (message: string) => void;
  onFixFinding?: (prompt: string) => Promise<void> | void;
}

const MODE_OPTIONS: Array<{ value: ReviewPanelMode; label: string }> = [
  { value: "local", label: "Local" },
  { value: "pr", label: "PR" },
  { value: "scan", label: "Scan" },
];

const SEVERITY_CLASS: Record<string, string> = {
  Critical: "border-status-error text-status-error",
  High: "border-accent-data text-accent-data",
  Medium: "border-status-warning text-status-warning",
  Low: "border-accent-structure text-accent-structure",
  Info: "border-text-muted text-text-secondary",
};

const TIER_LABEL: Record<SignalTier, string> = {
  tier_1: "Tier 1",
  tier_2: "Tier 2",
  noise: "Noise",
  unclassified: "Unclassified",
};

const TIER_CLASS: Record<SignalTier, string> = {
  tier_1: "border-status-error text-status-error",
  tier_2: "border-accent-signal text-accent-signal",
  noise: "border-text-muted text-text-muted",
  unclassified: "border-surface-overlay text-text-secondary",
};

function formatPercent(value: number) {
  return Number.isInteger(value) ? `${value}%` : `${value.toFixed(1)}%`;
}

function fileRange(comment: ReviewComment) {
  if (comment.line_start === comment.line_end) {
    return `${comment.file}:${comment.line_start}`;
  }
  return `${comment.file}:${comment.line_start}-${comment.line_end}`;
}

function pathComparisonKey(value: string) {
  let key = value.replace(/[\\/]+/g, "/");
  if (/^[A-Za-z]:\/+$/.test(key)) {
    key = `${key.slice(0, 2)}/`;
  } else if (key.length > 1) {
    key = key.replace(/\/+$/, "");
  }
  return /^[A-Za-z]:\//.test(key) ? key.toLowerCase() : key;
}

function isInsideWorkspace(workspaceRoot: string, filePath: string) {
  const root = pathComparisonKey(workspaceRoot);
  const target = pathComparisonKey(filePath);
  if (root === "/") return target.startsWith("/");
  return target === root || target.startsWith(`${root}/`);
}

async function absoluteFilePath(workspacePath: string | undefined, file: string) {
  if (!workspacePath) return null;
  const workspaceRoot = await normalize(workspacePath);
  const resolved = await resolve(workspaceRoot, file);
  const normalized = await normalize(resolved);
  return isInsideWorkspace(workspaceRoot, normalized) ? normalized : null;
}

function isHidden(comment: ReviewComment) {
  return (comment.signal_tier as string) === "hidden";
}

function ThumbUpIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path
        d="M5.5 7.1 7.7 2.5c.3-.6 1.2-.4 1.2.3v3h3.2c.8 0 1.4.7 1.2 1.5l-.8 4.5c-.1.7-.7 1.2-1.4 1.2H5.5V7.1Z"
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinejoin="round"
      />
      <path
        d="M2.6 7.2h2.9V13H2.6V7.2Z"
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function ThumbDownIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path
        d="M10.5 8.9 8.3 13.5c-.3.6-1.2.4-1.2-.3v-3H3.9c-.8 0-1.4-.7-1.2-1.5l.8-4.5C3.6 3.5 4.2 3 4.9 3h5.6v5.9Z"
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinejoin="round"
      />
      <path
        d="M13.4 8.8h-2.9V3h2.9v5.8Z"
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function Badge({
  children,
  className,
}: {
  children: ReactNode;
  className: string;
}) {
  return (
    <span
      className={`inline-flex h-5 items-center rounded-sm border px-1.5 font-mono text-caption ${className}`}
    >
      {children}
    </span>
  );
}

export function ReviewPanel({
  open,
  provider,
  model,
  workspacePath,
  canSendTurn = true,
  onClose,
  onError,
  onSuccess,
  onFixFinding,
}: ReviewPanelProps) {
  const [mode, setMode] = useState<ReviewPanelMode>("local");
  const [prNumber, setPrNumber] = useState("");
  const [result, setResult] = useState<ReviewResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcomes, setOutcomes] = useState<Record<string, ReviewOutcome>>({});

  const visibleComments = useMemo(
    () => result?.comments.filter((comment) => !isHidden(comment)) ?? [],
    [result],
  );
  const totalFindings = visibleComments.length + (result?.suppressed_count ?? 0);
  const actionableCount = useMemo(
    () => visibleComments.filter(isActionableReviewFinding).length,
    [visibleComments],
  );
  const shownCount = visibleComments.length;
  const canRun = Boolean(provider && model && workspacePath && !loading);
  const canFix = Boolean(provider && model && workspacePath && canSendTurn && !loading && onFixFinding);

  if (!open) return null;

  const runReview = async () => {
    if (!provider || !model || !workspacePath) {
      const message = "Select a workspace and model before running a review.";
      setError(message);
      onError?.(message);
      return;
    }

    if (mode === "pr" && !Number(prNumber)) {
      const message = "Enter a PR number.";
      setError(message);
      onError?.(message);
      return;
    }

    setLoading(true);
    setError(null);
    try {
      const review = await invoke<ReviewResult>("gospel_review", {
        config: {
          provider,
          model,
          mode,
          prNumber: mode === "pr" ? Number(prNumber) : null,
        },
      });
      setResult(review);
      setOutcomes({});
      onSuccess?.("Security review complete.");
    } catch (err) {
      const message = String(err);
      setError(message);
      onError?.(message);
    } finally {
      setLoading(false);
    }
  };

  const recordOutcome = async (comment: ReviewComment, outcome: ReviewOutcome) => {
    if (!result) return;

    try {
      await invoke<ReviewOutcomeOutput>("gospel_record_review_outcome", {
        runId: result.run_id,
        commentId: comment.comment_id,
        outcome,
      });
      setOutcomes((current) => ({
        ...current,
        [comment.comment_id]: outcome,
      }));
      onSuccess?.(outcome === "accepted" ? "Finding accepted." : "Finding rejected.");
    } catch (err) {
      const message = String(err);
      setError(message);
      onError?.(message);
    }
  };

  const openFile = async (comment: ReviewComment) => {
    try {
      const filePath = await absoluteFilePath(workspacePath, comment.file);
      if (!filePath) {
        const message = `Refusing to open ${comment.file}: path is outside the workspace.`;
        setError(message);
        onError?.(message);
        return;
      }
      await openPath(filePath);
    } catch (err) {
      const message = `Failed to open ${comment.file}: ${err}`;
      setError(message);
      onError?.(message);
    }
  };

  const copyFindingPrompt = async (comment: ReviewComment, index: number) => {
    if (!result) return;

    const prompt = buildExternalAgentFindingPrompt({
      comment,
      review: result,
      index: index + 1,
      workspacePath,
    });

    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error("Clipboard is unavailable.");
      }
      await navigator.clipboard.writeText(prompt);
      onSuccess?.("Finding prompt copied.");
    } catch (err) {
      const message = `Failed to copy finding prompt: ${err}`;
      setError(message);
      onError?.(message);
    }
  };

  const fixFinding = async (comment: ReviewComment, index: number) => {
    if (!result || !onFixFinding) return;

    const prompt = buildGospelFixFindingPrompt({
      comment,
      review: result,
      index: index + 1,
      workspacePath,
    });

    try {
      await onFixFinding(prompt);
      onClose();
    } catch (err) {
      const message = `Failed to start fix turn: ${err}`;
      setError(message);
      onError?.(message);
    }
  };

  return (
    <aside className="absolute inset-y-0 right-0 z-[--z-panels] flex w-full max-w-[560px] flex-col border-l border-surface-overlay bg-surface-elevated animate-fade-slide-in">
      <header className="flex min-h-[52px] items-center justify-between border-b border-surface-overlay px-4">
        <div className="min-w-0">
          <h2 className="m-0 truncate text-heading-sm font-medium text-text-primary">
            Security review
          </h2>
          <p className="m-0 truncate font-mono text-caption text-text-muted">
            {result?.run_id ?? workspacePath ?? "No workspace"}
          </p>
        </div>
        <button
          type="button"
          className="flex h-7 w-7 items-center justify-center rounded-sm text-text-muted transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary"
          onClick={onClose}
          aria-label="Close review panel"
          title="Close"
        >
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden="true">
            <path d="M3.5 3.5 10.5 10.5M10.5 3.5 3.5 10.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
          </svg>
        </button>
      </header>

      <div className="grid gap-3 border-b border-surface-overlay px-4 py-3">
        <div className="flex flex-wrap items-center gap-2">
          <div className="inline-grid grid-cols-3 overflow-hidden rounded-sm border border-surface-overlay">
            {MODE_OPTIONS.map((option) => {
              const active = option.value === mode;
              return (
                <button
                  key={option.value}
                  type="button"
                  className={`min-w-[64px] px-3 py-1.5 font-mono text-caption transition-colors duration-150 ease-out-quart ${
                    active
                      ? "bg-surface-overlay text-accent-action"
                      : "text-text-muted hover:bg-surface-overlay hover:text-text-secondary"
                  }`}
                  onClick={() => setMode(option.value)}
                  aria-pressed={active}
                >
                  {option.label}
                </button>
              );
            })}
          </div>
          {mode === "pr" && (
            <input
              className="h-8 w-24 rounded-sm border border-surface-overlay bg-surface-base px-2 font-mono text-body-sm text-text-primary placeholder:text-text-muted"
              inputMode="numeric"
              value={prNumber}
              onChange={(event) => setPrNumber(event.target.value.replace(/[^\d]/g, ""))}
              placeholder="PR #"
              aria-label="Pull request number"
            />
          )}
          <button
            type="button"
            className="ml-auto inline-flex h-8 items-center gap-2 rounded-sm bg-accent-action px-3 font-mono text-caption font-semibold text-text-inverse transition-opacity duration-150 ease-out-quart hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-35"
            onClick={runReview}
            disabled={!canRun}
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
              <path d="M8 2 13 4v3.5c0 3-1.9 5.2-5 6.5-3.1-1.3-5-3.5-5-6.5V4l5-2Z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
              <path d="m5.7 8.2 1.4 1.4 3.2-3.2" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
            {loading ? "Running" : "Run"}
          </button>
        </div>

        {result && (
          <div className="grid gap-2 rounded-md border border-surface-overlay bg-surface-base p-3">
            <div className="flex flex-wrap items-center gap-2">
              <Badge className="border-accent-action text-accent-action">
                {formatPercent(result.snr_percent)} SNR
              </Badge>
              <Badge className="border-text-muted text-text-secondary">
                {totalFindings} total
              </Badge>
              <Badge className="border-accent-signal text-accent-signal">
                {shownCount} shown
              </Badge>
              <Badge className="border-accent-data text-accent-data">
                {actionableCount} actionable
              </Badge>
              <Badge className="border-text-muted text-text-muted">
                {result.suppressed_count} suppressed
              </Badge>
            </div>
            <p className="m-0 text-body-sm text-text-secondary">{result.summary}</p>
          </div>
        )}

        {error && (
          <div className="rounded-md border border-status-error bg-surface-base p-3 text-body-sm text-status-error">
            {error}
          </div>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        {!result && !loading && (
          <div className="grid h-full place-items-center text-center">
            <div className="max-w-[34ch]">
              <p className="m-0 text-body-sm text-text-secondary">
                Run a review to inspect findings for the active workspace.
              </p>
            </div>
          </div>
        )}

        {loading && (
          <div className="grid gap-2">
            {Array.from({ length: 4 }).map((_, index) => (
              <div
                key={index}
                className="h-24 animate-pulse rounded-md bg-surface-overlay"
              />
            ))}
          </div>
        )}

        {result && !loading && visibleComments.length === 0 && (
          <div className="rounded-md border border-surface-overlay bg-surface-base p-4 text-body-sm text-text-secondary">
            No visible findings.
          </div>
        )}

        {result && !loading && visibleComments.length > 0 && (
          <ol className="m-0 grid list-none gap-3 p-0">
            {visibleComments.map((comment, index) => {
              const outcome = outcomes[comment.comment_id];
              return (
                <li
                  key={comment.comment_id || `${comment.file}-${comment.line_start}-${index}`}
                  className="rounded-md border border-surface-overlay bg-surface-base p-3"
                >
                  <div className="grid gap-2">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-mono text-caption text-text-muted">
                        [{index + 1}]
                      </span>
                      <Badge className={SEVERITY_CLASS[comment.severity] ?? SEVERITY_CLASS.Info}>
                        {comment.severity}
                      </Badge>
                      <Badge className={TIER_CLASS[comment.signal_tier]}>
                        {TIER_LABEL[comment.signal_tier]}
                      </Badge>
                      {comment.cwe_id && (
                        <Badge className="border-surface-overlay text-text-secondary">
                          {comment.cwe_id}
                        </Badge>
                      )}
                      <div className="ml-auto flex flex-wrap items-center justify-end gap-1">
                        {isActionableReviewFinding(comment) && (
                          <button
                            type="button"
                            className="inline-flex h-7 items-center rounded-sm border border-surface-overlay px-2 font-mono text-caption text-text-muted transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-accent-action disabled:cursor-not-allowed disabled:opacity-35"
                            onClick={() => void fixFinding(comment, index)}
                            disabled={!canFix}
                            title="Fix issue"
                            aria-label={`Fix issue ${index + 1}`}
                          >
                            Fix issue
                          </button>
                        )}
                        <button
                          type="button"
                          className="inline-flex h-7 items-center rounded-sm border border-surface-overlay px-2 font-mono text-caption text-text-muted transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-accent-action"
                          onClick={() => void copyFindingPrompt(comment, index)}
                          title="Copy to agent"
                          aria-label={`Copy finding ${index + 1} to agent`}
                        >
                          Copy to agent
                        </button>
                        <button
                          type="button"
                          className={`flex h-7 w-7 items-center justify-center rounded-sm border transition-colors duration-150 ease-out-quart ${
                            outcome === "accepted"
                              ? "border-status-success text-status-success"
                              : "border-surface-overlay text-text-muted hover:bg-surface-overlay hover:text-status-success"
                          }`}
                          title="Accept finding"
                          aria-label={`Accept finding ${index + 1}`}
                          onClick={() => void recordOutcome(comment, "accepted")}
                        >
                          <ThumbUpIcon />
                        </button>
                        <button
                          type="button"
                          className={`flex h-7 w-7 items-center justify-center rounded-sm border transition-colors duration-150 ease-out-quart ${
                            outcome === "rejected"
                              ? "border-status-error text-status-error"
                              : "border-surface-overlay text-text-muted hover:bg-surface-overlay hover:text-status-error"
                          }`}
                          title="Reject finding"
                          aria-label={`Reject finding ${index + 1}`}
                          onClick={() => void recordOutcome(comment, "rejected")}
                        >
                          <ThumbDownIcon />
                        </button>
                      </div>
                    </div>

                    <button
                      type="button"
                      className="justify-self-start truncate rounded-sm font-mono text-caption text-accent-action hover:underline"
                      onClick={() => void openFile(comment)}
                      title={fileRange(comment)}
                    >
                      {fileRange(comment)}
                    </button>

                    <div>
                      <h3 className="m-0 text-heading-sm font-medium text-text-primary">
                        {comment.title}
                      </h3>
                      <p className="m-0 mt-1 text-body-sm text-text-secondary">
                        {comment.description}
                      </p>
                    </div>

                    <details className="group rounded-sm border border-surface-overlay">
                      <summary className="cursor-pointer px-2 py-1.5 font-mono text-caption text-text-muted transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary">
                        Evidence and fix
                      </summary>
                      <div className="grid gap-2 border-t border-surface-overlay p-2">
                        <pre className="m-0 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded-sm bg-surface-elevated p-2 font-mono text-mono text-text-primary">
                          {comment.evidence}
                        </pre>
                        {comment.suggestion && (
                          <p className="m-0 text-body-sm text-text-secondary">
                            {comment.suggestion}
                          </p>
                        )}
                        {comment.verification_plan && (
                          <p className="m-0 font-mono text-caption text-text-muted">
                            {comment.verification_plan}
                          </p>
                        )}
                      </div>
                    </details>
                  </div>
                </li>
              );
            })}
          </ol>
        )}
      </div>
    </aside>
  );
}
