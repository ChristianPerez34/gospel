import { useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import { normalize, resolve } from "@tauri-apps/api/path";
import { openPath } from "@tauri-apps/plugin-opener";
import { Button } from "@/components/ui/button";
import type {
  MultiReviewResult,
  ReviewComment,
  ReviewFocus,
  ReviewFocusFilter,
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
import { useFocusTrap } from "../hooks/useFocusTrap";
import { useReviewProgress } from "../hooks/useReviewProgress";
import { ReviewProgressView } from "./ReviewProgressView";

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
  triggerRef?: RefObject<HTMLElement>;
  trapPaused?: boolean;
}

const MODE_OPTIONS: Array<{ value: ReviewPanelMode; label: string }> = [
  { value: "local", label: "Local" },
  { value: "pr", label: "PR" },
  { value: "scan", label: "Scan" },
];

const FOCUS_OPTIONS: Array<{ value: ReviewFocus; label: string; className: string }> = [
  { value: "Security", label: "Security", className: "border-status-error text-status-error" },
  { value: "BugHunt", label: "Bug Hunt", className: "border-accent-data text-accent-data" },
  { value: "Architecture", label: "Architecture", className: "border-accent-structure text-accent-structure" },
  { value: "Performance", label: "Performance", className: "border-status-warning text-status-warning" },
  { value: "Style", label: "Style", className: "border-text-muted text-text-secondary" },
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

function focusOption(focus: ReviewFocus) {
  return FOCUS_OPTIONS.find((option) => option.value === focus);
}

function focusLabel(focus: ReviewFocus) {
  return focusOption(focus)?.label ?? focus;
}

function FocusBadge({ focus }: { focus: ReviewFocus }) {
  return (
    <Badge className={focusOption(focus)?.className ?? "border-text-muted text-text-secondary"}>
      {focusLabel(focus)}
    </Badge>
  );
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
  triggerRef,
  trapPaused = false,
}: ReviewPanelProps) {
  const panelRef = useRef<HTMLElement>(null);
  const [mode, setMode] = useState<ReviewPanelMode>("local");
  const [prNumber, setPrNumber] = useState("");
  const [selectedFocus, setSelectedFocus] = useState<ReviewFocus>("Security");
  const [focusFilter, setFocusFilter] = useState<ReviewFocusFilter>("All");
  const [result, setResult] = useState<ReviewResult | null>(null);
  const [multiResult, setMultiResult] = useState<MultiReviewResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcomes, setOutcomes] = useState<Record<string, ReviewOutcome>>({});
  const reviewProgress = useReviewProgress();

  const visibleComments = useMemo(() => {
    const comments = multiResult
      ? multiResult.results.flatMap((review) => review.comments)
      : result?.comments ?? [];
    return comments.filter(
      (comment) => !isHidden(comment) && (focusFilter === "All" || comment.focus === focusFilter),
    );
  }, [multiResult, result, focusFilter]);
  const activeSummary = multiResult?.summary ?? result?.summary;
  const totalFindings = multiResult
    ? multiResult.total_findings + multiResult.total_suppressed
    : (result?.comments.length ?? 0) + (result?.suppressed_count ?? 0);
  const totalSuppressed = multiResult?.total_suppressed ?? result?.suppressed_count ?? 0;
  const averageSnr = multiResult
    ? multiResult.results.length > 0
      ? multiResult.results.reduce((sum, review) => sum + review.snr_percent, 0) / multiResult.results.length
      : 0
    : result?.snr_percent ?? 0;
  const actionableCount = useMemo(
    () => visibleComments.filter(isActionableReviewFinding).length,
    [visibleComments],
  );
  const shownCount = visibleComments.length;
  const canRun = Boolean(provider && model && workspacePath && !loading);
  const canFix = Boolean(provider && model && workspacePath && canSendTurn && !loading && onFixFinding);

  useFocusTrap({
    active: open && !trapPaused,
    containerRef: panelRef,
    onEscape: onClose,
    restoreFocusRef: triggerRef,
    shouldRestoreFocusOnDeactivate: !trapPaused,
  });

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
    reviewProgress.reset();
    try {
      const review = await invoke<ReviewResult>("gospel_review", {
        config: {
          provider,
          model,
          mode,
          prNumber: mode === "pr" ? Number(prNumber) : null,
          focus: selectedFocus,
        },
      });
      setResult(review);
      setMultiResult(null);
      setFocusFilter("All");
      setOutcomes({});
      onSuccess?.(`${focusLabel(selectedFocus)} review complete.`);
    } catch (err) {
      const message = String(err);
      setError(message);
      onError?.(message);
    } finally {
      setLoading(false);
    }
  };

  const runMultiReview = async () => {
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
    reviewProgress.reset();
    try {
      const review = await invoke<MultiReviewResult>("gospel_multi_review", {
        provider,
        model,
        mode,
        prNumber: mode === "pr" ? Number(prNumber) : null,
        focuses: FOCUS_OPTIONS.map((option) => option.value),
      });
      setMultiResult(review);
      setResult(null);
      setFocusFilter("All");
      setOutcomes({});
      onSuccess?.("Multi-focus review complete.");
    } catch (err) {
      const message = String(err);
      setError(message);
      onError?.(message);
    } finally {
      setLoading(false);
    }
  };

  const reviewForComment = (comment: ReviewComment) =>
    multiResult?.results.find((review) => review.focus === comment.focus) ?? result;

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
      const message = `Failed to copy finding prompt: ${err}`;
      setError(message);
      onError?.(message);
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
      onClose();
    } catch (err) {
      const message = `Failed to start fix turn: ${err}`;
      setError(message);
      onError?.(message);
    }
  };

  return (
    <aside
      className="review-panel"
      ref={panelRef}
      role="dialog"
      aria-modal="true"
      aria-label="Code review"
      tabIndex={-1}
    >
      <header className="flex min-h-[52px] items-center justify-between border-b border-surface-overlay px-4">
        <div className="min-w-0">
          <h2 className="m-0 truncate text-heading-sm font-medium text-text-primary">
            Code review
          </h2>
          <p className="m-0 truncate font-mono text-caption text-text-muted">
            {result?.run_id ?? (multiResult ? "multi-focus" : workspacePath) ?? "No workspace"}
          </p>
        </div>
        <Button
          variant="ghost"
          size="icon"
          onClick={onClose}
          aria-label="Close review panel"
          title="Close"
        >
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden="true">
            <path d="M3.5 3.5 10.5 10.5M10.5 3.5 3.5 10.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
          </svg>
        </Button>
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
                  className={`min-h-11 min-w-[64px] px-3 font-mono text-caption transition-colors duration-150 ease-out-quart ${
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
              className="h-11 w-24 rounded-sm border border-surface-overlay bg-surface-base px-2 font-mono text-body-sm text-text-primary placeholder:text-text-muted"
              inputMode="numeric"
              value={prNumber}
              onChange={(event) => setPrNumber(event.target.value.replace(/[^\d]/g, ""))}
              placeholder="PR #"
              aria-label="Pull request number"
            />
          )}
          <select
            className="h-11 rounded-sm border border-surface-overlay bg-surface-base px-2 font-mono text-caption text-text-primary"
            value={selectedFocus}
            onChange={(event) => setSelectedFocus(event.target.value as ReviewFocus)}
            aria-label="Review focus"
          >
            {FOCUS_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
          <div className="ml-auto flex flex-wrap items-center gap-2">
            <Button
              variant="default"
              size="sm"
              className="font-mono font-semibold"
              onClick={runReview}
              disabled={!canRun}
            >
              <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                <path d="M8 2 13 4v3.5c0 3-1.9 5.2-5 6.5-3.1-1.3-5-3.5-5-6.5V4l5-2Z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
                <path d="m5.7 8.2 1.4 1.4 3.2-3.2" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
              {loading ? "Running" : "Run"}
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="font-mono font-semibold"
              onClick={runMultiReview}
              disabled={!canRun}
            >
              Run All
            </Button>
          </div>
        </div>

        {(result || multiResult) && (
          <div className="review-summary grid gap-2 rounded-md border border-surface-overlay bg-surface-base p-3">
            <div className="flex flex-wrap items-center gap-2">
              {result && <FocusBadge focus={result.focus} />}
              <Badge className="border-accent-action text-accent-action">
                {formatPercent(averageSnr)} SNR
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
                {totalSuppressed} suppressed
              </Badge>
              {multiResult && Object.keys(multiResult.errors).length > 0 && (
                <Badge className="border-status-warning text-status-warning">
                  {Object.keys(multiResult.errors).length} failed
                </Badge>
              )}
            </div>
            <p className="m-0 text-body-sm text-text-secondary">{activeSummary}</p>
          </div>
        )}

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
              const count = multiResult.results.find((review) => review.focus === option.value)?.comments.length ?? 0;
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
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        {!result && !multiResult && !loading && (
          <div className="grid h-full place-items-center text-center">
            <div className="max-w-[34ch]">
              <p className="m-0 text-body-sm text-text-secondary">
                Run a review to inspect findings for the active workspace.
              </p>
            </div>
          </div>
        )}

        {loading && (
          <ReviewProgressView
            pipeline={reviewProgress.pipeline}
            log={reviewProgress.log}
            variant="active"
          />
        )}

        {!loading && (reviewProgress.pipeline.done || reviewProgress.pipeline.failed) && (
          <div className="mb-1">
            <ReviewProgressView
              pipeline={reviewProgress.pipeline}
              log={reviewProgress.log}
              variant="collapsed"
            />
          </div>
        )}

        {(result || multiResult) && !loading && visibleComments.length === 0 && (
          <div className="rounded-md border border-surface-overlay bg-surface-base p-4 text-body-sm text-text-secondary">
            No visible findings.
          </div>
        )}

        {(result || multiResult) && !loading && visibleComments.length > 0 && (
          <ol className="m-0 grid list-none gap-3 p-0">
            {visibleComments.map((comment, index) => {
              const outcome = outcomes[comment.comment_id];
              return (
                <li
                  key={comment.comment_id || `${comment.file}-${comment.line_start}-${index}`}
                  className="review-finding rounded-md border border-surface-overlay bg-surface-base p-3"
                >
                  <div className="grid gap-2">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-mono text-caption text-text-muted">
                        [{index + 1}]
                      </span>
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
                          className={outcome === "accepted" ? "border-status-success text-status-success" : ""}
                          title="Accept finding"
                          aria-label={`Accept finding ${index + 1}`}
                          onClick={() => void recordOutcome(comment, "accepted")}
                        >
                          <ThumbUpIcon />
                        </Button>
                        <Button
                          variant={outcome === "rejected" ? "default" : "outline"}
                          size="icon"
                          className={outcome === "rejected" ? "border-status-error text-status-error" : ""}
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
