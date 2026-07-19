import { invoke } from "@tauri-apps/api/core";
import { normalize, resolve } from "@tauri-apps/api/path";
import { openPath } from "@tauri-apps/plugin-opener";
import { type ReactNode, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  buildExternalAgentFindingPrompt,
  buildGospelFixFindingPrompt,
  isActionableReviewFinding,
} from "../reviewPrompts";
import type {
  MultiReviewResult,
  ReviewComment,
  ReviewFocusFilter,
  ReviewOutcome,
  ReviewOutcomeOutput,
  ReviewResult,
  SignalTier,
} from "../types";
import { FOCUS_OPTIONS } from "../utils/focus";
import { FocusBadge } from "./FocusBadge";

interface ReviewResultsProps {
  result: ReviewResult | null;
  multiResult: MultiReviewResult | null;
  workspacePath?: string;
  canFix: boolean;
  onFixFinding?: (prompt: string) => Promise<void> | void;
  onFixStarted?: () => void;
  onError?: (message: string) => void;
  onSuccess?: (message: string) => void;
}

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
  if (/^[A-Za-z]:\/+$/u.test(key)) {
    key = `${key.slice(0, 2)}/`;
  } else if (key.length > 1) {
    key = key.replace(/\/+$/u, "");
  }
  return /^[A-Za-z]:\//u.test(key) ? key.toLowerCase() : key;
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

function outcomeKey(review: ReviewResult | null, comment: ReviewComment) {
  return `${review?.run_id ?? "single"}:${comment.comment_id}`;
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

function Badge({ children, className }: { children: ReactNode; className: string }) {
  return (
    <span
      className={`inline-flex h-5 items-center rounded-sm border px-1.5 font-mono text-caption ${className}`}
    >
      {children}
    </span>
  );
}

export function ReviewResults({
  result,
  multiResult,
  workspacePath,
  canFix,
  onFixFinding,
  onFixStarted,
  onError,
  onSuccess,
}: ReviewResultsProps) {
  const [focusFilter, setFocusFilter] = useState<ReviewFocusFilter>("All");
  const [outcomes, setOutcomes] = useState<Record<string, ReviewOutcome>>({});
  const [error, setError] = useState<string | null>(null);

  const visibleComments = useMemo(() => {
    const comments = multiResult
      ? multiResult.results.flatMap((review) => review.comments)
      : (result?.comments ?? []);
    return comments.filter(
      (comment) => !isHidden(comment) && (focusFilter === "All" || comment.focus === focusFilter)
    );
  }, [multiResult, result, focusFilter]);

  const activeSummary = multiResult?.summary ?? result?.summary;
  const totalFindings = multiResult
    ? multiResult.total_findings + multiResult.total_suppressed
    : (result?.comments.length ?? 0) + (result?.suppressed_count ?? 0);
  const totalSuppressed = multiResult?.total_suppressed ?? result?.suppressed_count ?? 0;
  const averageSnr = multiResult
    ? multiResult.results.length > 0
      ? multiResult.results.reduce((sum, review) => sum + review.snr_percent, 0) /
        multiResult.results.length
      : 0
    : (result?.snr_percent ?? 0);
  const actionableCount = useMemo(
    () => visibleComments.filter(isActionableReviewFinding).length,
    [visibleComments]
  );

  if (!result && !multiResult) return null;

  const reviewForComment = (comment: ReviewComment) =>
    multiResult?.results.find((review) => review.focus === comment.focus) ?? result;

  const reportError = (message: string) => {
    setError(message);
    onError?.(message);
  };

  const recordOutcome = async (comment: ReviewComment, outcome: ReviewOutcome) => {
    const sourceReview = reviewForComment(comment);
    if (!sourceReview) return;

    try {
      await invoke<ReviewOutcomeOutput>("gospel_record_review_outcome", {
        runId: sourceReview.run_id,
        commentId: comment.comment_id,
        outcome,
      });
      setOutcomes((current) => ({
        ...current,
        [outcomeKey(sourceReview, comment)]: outcome,
      }));
      onSuccess?.(outcome === "accepted" ? "Finding accepted." : "Finding rejected.");
    } catch (err) {
      reportError(String(err));
    }
  };

  const openFile = async (comment: ReviewComment) => {
    try {
      const filePath = await absoluteFilePath(workspacePath, comment.file);
      if (!filePath) {
        reportError(`Refusing to open ${comment.file}: path is outside the workspace.`);
        return;
      }
      await openPath(filePath);
    } catch (err) {
      reportError(`Failed to open ${comment.file}: ${err}`);
    }
  };

  const copyFindingPrompt = async (comment: ReviewComment, index: number) => {
    const sourceReview = reviewForComment(comment);
    if (!sourceReview) return;

    const prompt = buildExternalAgentFindingPrompt({
      comment,
      review: sourceReview,
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
      reportError(`Failed to copy finding prompt: ${err}`);
    }
  };

  const fixFinding = async (comment: ReviewComment, index: number) => {
    const sourceReview = reviewForComment(comment);
    if (!sourceReview || !onFixFinding) return;

    const prompt = buildGospelFixFindingPrompt({
      comment,
      review: sourceReview,
      index: index + 1,
      workspacePath,
    });

    try {
      await onFixFinding(prompt);
      onFixStarted?.();
    } catch (err) {
      reportError(`Failed to start fix turn: ${err}`);
    }
  };

  return (
    <div className="review-results grid gap-3">
      <div className="review-summary grid gap-2 rounded-md border border-surface-overlay bg-surface-base p-3">
        <div className="flex flex-wrap items-center gap-2">
          {result && <FocusBadge focus={result.focus} />}
          <Badge className="border-accent-action text-accent-action">
            {formatPercent(averageSnr)} SNR
          </Badge>
          <Badge className="border-text-muted text-text-secondary">{totalFindings} total</Badge>
          <Badge className="border-accent-signal text-accent-signal">
            {visibleComments.length} shown
          </Badge>
          <Badge className="border-accent-data text-accent-data">
            {actionableCount} actionable
          </Badge>
          <Badge className="border-text-muted text-text-muted">{totalSuppressed} suppressed</Badge>
          {multiResult && Object.keys(multiResult.errors).length > 0 && (
            <Badge className="border-status-warning text-status-warning">
              {Object.keys(multiResult.errors).length} failed
            </Badge>
          )}
        </div>
        <p className="m-0 text-body-sm text-text-secondary">{activeSummary}</p>
      </div>

      {multiResult && (
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            className={`min-h-8 rounded-sm border px-3 font-mono text-caption transition-colors ${
              focusFilter === "All"
                ? "border-accent-action bg-surface-overlay text-accent-action"
                : "border-surface-overlay text-text-muted hover:bg-surface-overlay hover:text-text-secondary"
            }`}
            onClick={() => setFocusFilter("All")}
            aria-pressed={focusFilter === "All"}
          >
            All ({multiResult.results.reduce((sum, review) => sum + review.comments.length, 0)})
          </button>
          {FOCUS_OPTIONS.map((option) => {
            const count =
              multiResult.results.find((review) => review.focus === option.value)?.comments
                .length ?? 0;
            return (
              <button
                key={option.value}
                type="button"
                className={`min-h-8 rounded-sm border px-3 font-mono text-caption transition-colors ${
                  focusFilter === option.value
                    ? "border-accent-action bg-surface-overlay text-accent-action"
                    : "border-surface-overlay text-text-muted hover:bg-surface-overlay hover:text-text-secondary"
                }`}
                onClick={() => setFocusFilter(option.value)}
                aria-pressed={focusFilter === option.value}
              >
                {option.label} ({count})
              </button>
            );
          })}
        </div>
      )}

      {error && (
        <div className="whitespace-pre-wrap rounded-md border border-status-error bg-surface-base p-3 text-body-sm text-status-error">
          {error}
        </div>
      )}

      {visibleComments.length === 0 ? (
        <div className="rounded-md border border-surface-overlay bg-surface-base p-4 text-body-sm text-text-secondary">
          No visible findings.
        </div>
      ) : (
        <ol className="m-0 grid list-none gap-3 p-0">
          {visibleComments.map((comment, index) => {
            const sourceReview = reviewForComment(comment);
            const outcome = outcomes[outcomeKey(sourceReview, comment)];
            return (
              <li
                key={comment.comment_id || `${comment.file}-${comment.line_start}-${index}`}
                className="review-finding rounded-md border border-surface-overlay bg-surface-base p-3"
              >
                <div className="grid gap-2">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-mono text-caption text-text-muted">[{index + 1}]</span>
                    {multiResult && <FocusBadge focus={comment.focus} />}
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
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={() => void fixFinding(comment, index)}
                          disabled={!canFix}
                          title="Fix issue"
                          aria-label={`Fix issue ${index + 1}`}
                        >
                          Fix issue
                        </Button>
                      )}
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => void copyFindingPrompt(comment, index)}
                        title="Copy to agent"
                        aria-label={`Copy finding ${index + 1} to agent`}
                      >
                        Copy to agent
                      </Button>
                      <Button
                        variant={outcome === "accepted" ? "default" : "outline"}
                        size="icon"
                        className={
                          outcome === "accepted" ? "border-status-success text-status-success" : ""
                        }
                        title="Accept finding"
                        aria-label={`Accept finding ${index + 1}`}
                        onClick={() => void recordOutcome(comment, "accepted")}
                      >
                        <ThumbUpIcon />
                      </Button>
                      <Button
                        variant={outcome === "rejected" ? "default" : "outline"}
                        size="icon"
                        className={
                          outcome === "rejected" ? "border-status-error text-status-error" : ""
                        }
                        title="Reject finding"
                        aria-label={`Reject finding ${index + 1}`}
                        onClick={() => void recordOutcome(comment, "rejected")}
                      >
                        <ThumbDownIcon />
                      </Button>
                    </div>
                  </div>

                  <button
                    type="button"
                    className="min-h-11 justify-self-start truncate rounded-sm font-mono text-caption text-accent-action hover:underline"
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
                    <summary className="min-h-11 cursor-pointer px-2 py-3 font-mono text-caption text-text-muted transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary">
                      Evidence and fix
                    </summary>
                    <div className="grid gap-2 border-t border-surface-overlay p-2">
                      <pre className="m-0 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded-sm bg-surface-elevated p-2 font-mono text-mono text-text-primary">
                        {comment.evidence}
                      </pre>
                      {comment.suggestion && (
                        <p className="m-0 text-body-sm text-text-secondary">{comment.suggestion}</p>
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
  );
}
