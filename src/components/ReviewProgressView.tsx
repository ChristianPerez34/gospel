import { useEffect, useRef } from "react";
import type { FocusProgress } from "../hooks/useReviewProgress";
import type {
  ReviewActivityEntry,
  ReviewFocus,
  ReviewNodeState,
  ReviewPipelineState,
} from "../types";
import { FOCUS_ORDER, focusLabel } from "../utils/focus";
import { FocusBadge } from "./FocusBadge";

interface ReviewProgressViewProps {
  perFocus: Partial<Record<ReviewFocus, FocusProgress>>;
  log: ReviewActivityEntry[];
  variant?: "active" | "collapsed";
}

interface NodeConfig {
  label: string;
  state: ReviewNodeState;
  /** CSS color value used for the dot/label via `--rp-node-color`. */
  color: string;
  meta: string;
}

function formatClock(timestamp: number): string {
  const date = new Date(timestamp);
  const hh = String(date.getHours()).padStart(2, "0");
  const mm = String(date.getMinutes()).padStart(2, "0");
  const ss = String(date.getSeconds()).padStart(2, "0");
  return `${hh}:${mm}:${ss}`;
}

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches
  );
}

function chunkPercent(pipeline: ReviewPipelineState): number {
  const { totalChunks, chunk, status } = pipeline.detector;
  if (totalChunks <= 0) return 0;
  const completed = status === "done" || status === "failed" ? chunk : chunk - 1;
  const clamped = Math.max(0, Math.min(completed, totalChunks));
  return (clamped / totalChunks) * 100;
}

function connectorPercent(pipeline: ReviewPipelineState): number {
  // 0% at start, 50% when detector done, 100% when finalize done.
  if (pipeline.finalize === "done" || pipeline.done) return 100;
  if (pipeline.validator === "active" || pipeline.validator === "done") return 50;
  if (pipeline.detector.status === "done") return 50;
  if (pipeline.detector.status === "active") {
    return Math.min(50, chunkPercent(pipeline) / 2);
  }
  return 0;
}

export function ReviewProgressView({ perFocus, log, variant = "active" }: ReviewProgressViewProps) {
  const feedRef = useRef<HTMLDivElement>(null);

  // Auto-scroll the feed to the latest entry. Respect reduced-motion.
  // A newly appended entry should move the feed to the latest activity.
  // biome-ignore lint/correctness/useExhaustiveDependencies: Log length is an intentional trigger.
  useEffect(() => {
    const node = feedRef.current;
    if (!node) return;
    const behavior = prefersReducedMotion() ? "auto" : "smooth";
    node.scrollTo({ top: node.scrollHeight, behavior });
  }, [log.length]);

  if (variant === "collapsed") {
    return (
      <details className="review-progress group rounded-sm border border-surface-overlay">
        <summary className="min-h-11 cursor-pointer px-2 py-3 font-mono text-caption text-text-muted transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary">
          Activity log
        </summary>
        <div className="border-t border-surface-overlay p-2">
          <ActivityFeed log={log} />
        </div>
      </details>
    );
  }

  const sortedFocuses = FOCUS_ORDER.filter((focus) => focus in perFocus);

  return (
    <div className="review-progress grid gap-3" role="status" aria-live="polite">
      {sortedFocuses.map((focus) => {
        const progress = perFocus[focus];
        return progress ? <FocusPipeline key={focus} progress={progress} /> : null;
      })}
      <ActivityFeed log={log} feedRef={feedRef} />
    </div>
  );
}

function FocusPipeline({ progress }: { progress: FocusProgress }) {
  const { pipeline, focus } = progress;

  const detector: NodeConfig = {
    label: "Detector",
    state: pipeline.detector.status,
    color: "var(--gospel-agent-cyan)",
    meta:
      pipeline.detector.totalChunks > 0
        ? `chunk ${pipeline.detector.chunk}/${pipeline.detector.totalChunks}`
        : pipeline.detector.status === "active"
          ? "starting"
          : "",
  };
  const validator: NodeConfig = {
    label: "Validator",
    state: pipeline.validator,
    color: "var(--gospel-agent-violet)",
    meta: "",
  };
  const finalize: NodeConfig = {
    label: "Finalize",
    state: pipeline.finalize,
    color: "var(--gospel-accent-action)",
    meta: "",
  };
  const nodes = [detector, validator, finalize];

  const barWidth = pipeline.done ? 100 : chunkPercent(pipeline);
  const connectorWidth = connectorPercent(pipeline);

  return (
    <div className="review-progress__focus grid gap-2" data-focus={focus}>
      <div className="font-mono text-caption text-text-secondary">{focusLabel(focus)}</div>
      <div className="review-progress__pipeline-row">
        <div className="review-progress__connector" aria-hidden="true" />
        <div
          className="review-progress__connector-fill"
          style={{ width: `calc((100% - 33.32%) * ${connectorWidth / 100})` }}
          aria-hidden="true"
        />
        {nodes.map((node) => (
          <div
            key={node.label}
            className="review-progress__node"
            data-state={node.state}
            style={{ ["--rp-node-color" as string]: node.color }}
          >
            <div className="review-progress__dot" aria-hidden="true" />
            <div className="review-progress__label">{node.label}</div>
            <div className="review-progress__meta">{node.meta}</div>
          </div>
        ))}
      </div>

      <div className="review-progress__bar" aria-hidden="true">
        <div className="review-progress__bar-fill" style={{ width: `${barWidth}%` }} />
      </div>
    </div>
  );
}

interface ActivityFeedProps {
  log: ReviewActivityEntry[];
  feedRef?: React.RefObject<HTMLDivElement>;
}

function ActivityFeed({ log, feedRef }: ActivityFeedProps) {
  if (log.length === 0) {
    return (
      <div className="review-progress__feed" ref={feedRef}>
        <div className="text-text-muted">Waiting for review to start…</div>
      </div>
    );
  }
  return (
    <div className="review-progress__feed" ref={feedRef}>
      {log.map((entry, index) => (
        <div
          // Entries have no backend ID and identical events can occur in sequence.
          // biome-ignore lint/suspicious/noArrayIndexKey: Position disambiguates duplicate log entries.
          key={`${entry.timestamp}-${index}`}
          className={`review-progress__entry${
            entry.phase === "failed" ? " review-progress__entry--failed" : ""
          }${entry.phase === "done" ? " review-progress__entry--done" : ""}`}
        >
          <span className="review-progress__entry-time">{formatClock(entry.timestamp)}</span>
          {entry.focus && entry.phase !== "multiFocus" && <FocusBadge focus={entry.focus} />}
          <span>{entry.text}</span>
        </div>
      ))}
    </div>
  );
}
