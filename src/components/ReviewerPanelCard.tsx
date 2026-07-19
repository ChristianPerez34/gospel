import { useEffect, useRef } from "react";
import type { CanvasReviewerNode } from "../hooks/useConstellation";

const FOCUS_COLOR_HEX: Record<string, string> = {
  Security: "oklch(0.65 0.2 25)",
  BugHunt: "oklch(0.7 0.13 30)",
  Architecture: "oklch(0.72 0.14 280)",
  Performance: "oklch(0.75 0.15 85)",
  Style: "oklch(0.7 0.02 260)",
};

interface ReviewerPanelCardProps {
  r: CanvasReviewerNode;
  active: boolean;
  onHover: () => void;
  onLeave: () => void;
}

export function ReviewerPanelCard({
  r,
  active,
  onHover,
  onLeave,
}: ReviewerPanelCardProps) {
  const color = FOCUS_COLOR_HEX[r.focus] ?? "var(--gospel-text-muted)";
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    scrollRef.current?.scrollTo({
      top: scrollRef.current.scrollHeight,
      behavior: "smooth",
    });
  }, [r.comments.length]);

  const statusLabel = reviewerStatusLabel(r.status);

  return (
    <div
      className="reviewer-panel-card"
      style={{
        borderLeftColor: color,
        borderColor: active ? color : "var(--gospel-surface-line)",
        boxShadow: active ? `0 0 12px color-mix(in srgb, ${color} 20%, transparent)` : "none",
      }}
      onMouseEnter={onHover}
      onMouseLeave={onLeave}
    >
      <div className="reviewer-panel-card-head">
        <span
          className="reviewer-panel-avatar"
          style={{
            background: `color-mix(in srgb, ${color} 13%, transparent)`,
            color,
            borderColor: `color-mix(in srgb, ${color} 33%, transparent)`,
          }}
        >
          {r.name[0]}
        </span>
        <div className="reviewer-panel-card-meta">
          <span className="reviewer-panel-card-name">{r.name}</span>
          <span className="reviewer-panel-card-role">{r.focus}</span>
        </div>
        <span
          className="reviewer-panel-card-status"
          style={{ color: r.status === "done" ? "var(--gospel-text-muted)" : color }}
        >
          {statusLabel}
        </span>
      </div>

      <div className="reviewer-panel-progress-track">
        <div
          className="reviewer-panel-progress-fill"
          style={{ width: `${r.progress * 100}%`, background: color }}
        />
      </div>

      {r.status === "active" && (
        <div className="reviewer-panel-now-reading">
          <span
            className="reviewer-panel-pulse-dot"
            style={{ background: color }}
          />
          <span>analyzing…</span>
        </div>
      )}

      <div className="reviewer-panel-comment-stream" ref={scrollRef}>
        {r.comments.length === 0 && r.status !== "active" && (
          <div className="reviewer-panel-comment-empty">waiting…</div>
        )}
        {r.comments.slice(-12).map((c, i) => (
          <div key={i} className="reviewer-panel-comment">
            <span className="reviewer-panel-comment-text">{c.text}</span>
          </div>
        ))}
        {r.status === "active" && (
          <div
            className="reviewer-panel-typing"
            style={{ color }}
          >
            typing…
          </div>
        )}
      </div>

      {r.status === "done" && (
        <div
          className="reviewer-panel-verdict"
          style={{
            background:
              r.findings > 0
                ? "color-mix(in srgb, var(--gospel-status-warning) 13%, transparent)"
                : "color-mix(in srgb, var(--gospel-status-success) 13%, transparent)",
            color:
              r.findings > 0
                ? "var(--gospel-status-warning)"
                : "var(--gospel-status-success)",
          }}
        >
          {r.findings > 0
            ? `${r.findings} finding${r.findings === 1 ? "" : "s"}`
            : "✓ clean"}
        </div>
      )}
    </div>
  );
}

function reviewerStatusLabel(status: CanvasReviewerNode["status"]): string {
  switch (status) {
    case "idle":
      return "queued";
    case "active":
      return "running";
    case "done":
      return "done";
    case "failed":
      return "failed";
  }
}
