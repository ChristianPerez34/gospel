import { invoke } from "@tauri-apps/api/core";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { useConstellation } from "../hooks/useConstellation";
import type { UseReviewProgress } from "../hooks/useReviewProgress";
import type {
  CurrentTurn,
  Message,
  ReviewFocus,
  ReviewResult,
  MultiReviewResult,
} from "../types";
import { FOCUS_OPTIONS, focusLabel } from "../utils/focus";
import { ConstellationCanvas } from "./ConstellationCanvas";
import { ReviewerPanelCard } from "./ReviewerPanelCard";

// ── Props ───────────────────────────────────────────────────────────────────

interface WorkbenchLayoutProps {
  /** Chat messages (finalized). */
  messages: Message[];
  /** Live turn (streaming). */
  currentTurn: CurrentTurn | null;
  /** Whether the agent is currently streaming. */
  isStreaming: boolean;
  /** Review progress state from useReviewProgress. */
  reviewProgress: UseReviewProgress;
  /** Review trigger props. */
  reviewProvider?: string;
  reviewModel?: string;
  workspacePath?: string;
  canSendTurn: boolean;
  onFixFinding?: (prompt: string) => Promise<void> | void;
  onError?: (message: string) => void;
  onSuccess?: (message: string) => void;
  /** Conversation tab content (ChatView + InputBar). */
  conversationSlot: ReactNode;
  /** Whether to collapse the left column (focus mode). */
  focusMode: boolean;
  /** Approval resolver from session manager. */
  onResolveApproval?: (id: string, decision: "approve" | "deny") => Promise<void>;
}

type LeftTab = "conversation" | "reviewers";
type ReviewMode = "local" | "pr" | "scan";

// ── Component ───────────────────────────────────────────────────────────────

export function WorkbenchLayout({
  messages,
  currentTurn,
  isStreaming,
  reviewProgress,
  reviewProvider,
  reviewModel,
  workspacePath,
  canSendTurn,
  onFixFinding,
  onError,
  onSuccess,
  conversationSlot,
  focusMode,
  onResolveApproval,
}: WorkbenchLayoutProps) {
  const [leftTab, setLeftTab] = useState<LeftTab>("conversation");
  const [colW, setColW] = useState(380);
  const [activeReviewer, setActiveReviewer] = useState<string | null>(null);

  // Review trigger state (extracted from ReviewPanel)
  const [reviewMode, setReviewMode] = useState<ReviewMode>("local");
  const [prNumber, setPrNumber] = useState("");
  const [selectedFocus, setSelectedFocus] = useState<ReviewFocus>("Security");
  const [reviewLoading, setReviewLoading] = useState(false);
  const [reviewError, setReviewError] = useState<string | null>(null);
  const [reviewResult, setReviewResult] = useState<ReviewResult | null>(null);
  const [multiReviewResult, setMultiReviewResult] =
    useState<MultiReviewResult | null>(null);

  const constellation = useConstellation(
    messages,
    currentTurn,
    reviewProgress,
    isStreaming
  );

  // Auto-switch to Reviewers tab when review starts.
  const switchedRef = useRef(false);
  useEffect(() => {
    if (constellation.reviewActive && !switchedRef.current) {
      switchedRef.current = true;
      setLeftTab("reviewers");
    }
    if (!constellation.reviewActive) switchedRef.current = false;
  }, [constellation.reviewActive]);

  // Draggable splitter
  const draggingRef = useRef(false);
  const onSplitterDown = useCallback(() => {
    draggingRef.current = true;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }, []);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!draggingRef.current) return;
      const w = Math.max(280, Math.min(640, e.clientX));
      setColW(w);
    };
    const onUp = () => {
      if (!draggingRef.current) return;
      draggingRef.current = false;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);

  // Review trigger handlers (extracted from ReviewPanel)
  const canRun = Boolean(
    reviewProvider && reviewModel && workspacePath && !reviewLoading
  );

  const runReview = useCallback(async () => {
    if (!reviewProvider || !reviewModel || !workspacePath) {
      const msg = "Select a workspace and model before running a review.";
      setReviewError(msg);
      onError?.(msg);
      return;
    }
    if (reviewMode === "pr" && !Number(prNumber)) {
      const msg = "Enter a PR number.";
      setReviewError(msg);
      onError?.(msg);
      return;
    }
    setReviewLoading(true);
    setReviewError(null);
    reviewProgress.reset();
    try {
      const review = await invoke<ReviewResult>("gospel_review", {
        config: {
          provider: reviewProvider,
          model: reviewModel,
          mode: reviewMode,
          prNumber: reviewMode === "pr" ? Number(prNumber) : null,
          focus: selectedFocus,
        },
      });
      setReviewResult(review);
      setMultiReviewResult(null);
      onSuccess?.(`${focusLabel(selectedFocus)} review complete.`);
    } catch (err) {
      const msg = String(err);
      setReviewError(msg);
      onError?.(msg);
    } finally {
      setReviewLoading(false);
    }
  }, [
    reviewProvider,
    reviewModel,
    workspacePath,
    reviewMode,
    prNumber,
    selectedFocus,
    reviewProgress,
    onError,
    onSuccess,
  ]);

  const runMultiReview = useCallback(async () => {
    if (!reviewProvider || !reviewModel || !workspacePath) {
      const msg = "Select a workspace and model before running a review.";
      setReviewError(msg);
      onError?.(msg);
      return;
    }
    if (reviewMode === "pr" && !Number(prNumber)) {
      const msg = "Enter a PR number.";
      setReviewError(msg);
      onError?.(msg);
      return;
    }
    setReviewLoading(true);
    setReviewError(null);
    reviewProgress.reset();
    try {
      const review = await invoke<MultiReviewResult>("gospel_multi_review", {
        provider: reviewProvider,
        model: reviewModel,
        mode: reviewMode,
        prNumber: reviewMode === "pr" ? Number(prNumber) : null,
        focuses: FOCUS_OPTIONS.map((o) => o.value),
      });
      setMultiReviewResult(review);
      setReviewResult(null);
      onSuccess?.("Multi-focus review complete.");
    } catch (err) {
      const msg = String(err);
      setReviewError(msg);
      onError?.(msg);
    } finally {
      setReviewLoading(false);
    }
  }, [
    reviewProvider,
    reviewModel,
    workspacePath,
    reviewMode,
    prNumber,
    reviewProgress,
    onError,
    onSuccess,
  ]);

  const handleApprove = useCallback(
    (id: string) => {
      void onResolveApproval?.(id, "approve");
    },
    [onResolveApproval]
  );

  const visibleReviewers = constellation.reviewerNodes;
  const verdictCount = visibleReviewers.filter((r) => r.status === "done").length;
  const liveCount = visibleReviewers.filter((r) => r.status === "active").length;

  if (focusMode) {
    // Focus mode: canvas only, no left column.
    return (
      <div className="workbench-layout workbench-focus">
        <ConstellationCanvas
          toolNodes={constellation.toolNodes}
          reviewerNodes={constellation.reviewerNodes}
          reviewActive={constellation.reviewActive}
          agentRunning={constellation.agentRunning}
          onApprove={handleApprove}
        />
      </div>
    );
  }

  return (
    <div className="workbench-layout">
      {/* Left: tabbed column */}
      <aside
        className="workbench-left-column"
        style={{ width: colW }}
      >
        <div className="workbench-tab-bar">
          <button
            type="button"
            className={`workbench-tab-btn ${leftTab === "conversation" ? "is-active" : ""}`}
            onClick={() => setLeftTab("conversation")}
          >
            Conversation
            {messages.length > 0 && (
              <span className="workbench-tab-badge">{messages.length}</span>
            )}
          </button>
          <button
            type="button"
            className={`workbench-tab-btn ${leftTab === "reviewers" ? "is-active" : ""}`}
            onClick={() => setLeftTab("reviewers")}
          >
            Reviewers
            {constellation.reviewActive && (
              <span className="workbench-tab-badge-live">{liveCount} live</span>
            )}
          </button>
        </div>

        {leftTab === "conversation" ? (
          <div className="workbench-conversation-tab">{conversationSlot}</div>
        ) : (
          <ReviewersTab
            reviewers={visibleReviewers}
            reviewActive={constellation.reviewActive}
            verdictCount={verdictCount}
            activeReviewer={activeReviewer}
            onHover={setActiveReviewer}
            onLeave={() => setActiveReviewer(null)}
            // Review trigger
            reviewMode={reviewMode}
            onReviewModeChange={setReviewMode}
            prNumber={prNumber}
            onPrNumberChange={setPrNumber}
            selectedFocus={selectedFocus}
            onSelectedFocusChange={setSelectedFocus}
            canRun={canRun}
            reviewLoading={reviewLoading}
            onRunReview={runReview}
            onRunMultiReview={runMultiReview}
            reviewError={reviewError}
            // Results
            reviewResult={reviewResult}
            multiReviewResult={multiReviewResult}
            canFix={canSendTurn && !reviewLoading && Boolean(onFixFinding)}
            onFixFinding={onFixFinding}
            workspacePath={workspacePath}
          />
        )}
      </aside>

      {/* Draggable splitter */}
      <div
        className="workbench-splitter"
        onMouseDown={onSplitterDown}
        aria-hidden
      />

      {/* Right: constellation canvas */}
      <div className="workbench-canvas-wrap">
        <ConstellationCanvas
          toolNodes={constellation.toolNodes}
          reviewerNodes={constellation.reviewerNodes}
          reviewActive={constellation.reviewActive}
          agentRunning={constellation.agentRunning}
          onApprove={handleApprove}
        />
      </div>
    </div>
  );
}

// ── Reviewers tab ───────────────────────────────────────────────────────────

interface ReviewersTabProps {
  reviewers: ReturnType<typeof useConstellation>["reviewerNodes"];
  reviewActive: boolean;
  verdictCount: number;
  activeReviewer: string | null;
  onHover: (id: string) => void;
  onLeave: () => void;
  // Review trigger
  reviewMode: ReviewMode;
  onReviewModeChange: (mode: ReviewMode) => void;
  prNumber: string;
  onPrNumberChange: (value: string) => void;
  selectedFocus: ReviewFocus;
  onSelectedFocusChange: (focus: ReviewFocus) => void;
  canRun: boolean;
  reviewLoading: boolean;
  onRunReview: () => void;
  onRunMultiReview: () => void;
  reviewError: string | null;
  // Results
  reviewResult: ReviewResult | null;
  multiReviewResult: MultiReviewResult | null;
  canFix: boolean;
  onFixFinding?: (prompt: string) => Promise<void> | void;
  workspacePath?: string;
}

function ReviewersTab({
  reviewers,
  reviewActive,
  verdictCount,
  activeReviewer,
  onHover,
  onLeave,
  reviewMode,
  onReviewModeChange,
  prNumber,
  onPrNumberChange,
  selectedFocus,
  onSelectedFocusChange,
  canRun,
  reviewLoading,
  onRunReview,
  onRunMultiReview,
  reviewError,
}: ReviewersTabProps) {
  return (
    <div className="workbench-reviewers-tab">
      {/* Review trigger header */}
      <div className="review-trigger">
        <div className="review-trigger-row">
          <div className="review-trigger-mode-group">
            {(["local", "pr", "scan"] as const).map((mode) => (
              <button
                key={mode}
                type="button"
                className={`review-trigger-mode-btn ${reviewMode === mode ? "is-active" : ""}`}
                onClick={() => onReviewModeChange(mode)}
                aria-pressed={reviewMode === mode}
              >
                {mode === "local" ? "Local" : mode === "pr" ? "PR" : "Scan"}
              </button>
            ))}
          </div>
          {reviewMode === "pr" && (
            <input
              className="review-trigger-pr-input"
              inputMode="numeric"
              value={prNumber}
              onChange={(e) => onPrNumberChange(e.target.value.replace(/[^\d]/g, ""))}
              placeholder="PR #"
              aria-label="Pull request number"
            />
          )}
          <select
            className="review-trigger-focus-select"
            value={selectedFocus}
            onChange={(e) => onSelectedFocusChange(e.target.value as ReviewFocus)}
            aria-label="Review focus"
          >
            {FOCUS_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </div>
        <div className="review-trigger-actions">
          <Button
            variant="default"
            size="sm"
            className="font-mono font-semibold"
            onClick={onRunReview}
            disabled={!canRun}
          >
            {reviewLoading ? "Running" : "Run"}
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="font-mono font-semibold"
            onClick={onRunMultiReview}
            disabled={!canRun}
          >
            Run All
          </Button>
        </div>
        {reviewError && (
          <div className="review-trigger-error">{reviewError}</div>
        )}
      </div>

      {/* Summary pills */}
      {reviewActive && (
        <div className="reviewers-summary">
          <span className="reviewers-summary-meta">
            {verdictCount}/{reviewers.length} verdicts
          </span>
        </div>
      )}

      {/* Reviewer cards */}
      <div className="reviewers-list">
        {reviewers.length === 0 && !reviewActive && (
          <div className="reviewers-empty">
            Run a review to see parallel reviewer activity.
          </div>
        )}
        {reviewers.map((r) => (
          <ReviewerPanelCard
            key={r.id}
            r={r}
            active={activeReviewer === r.id}
            onHover={() => onHover(r.id)}
            onLeave={onLeave}
          />
        ))}
      </div>
    </div>
  );
}
