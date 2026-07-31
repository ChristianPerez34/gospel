import { useEffect, useMemo, useRef, useState } from "react";
import {
  type CanvasReviewerNode,
  type CanvasToolKind,
  type CanvasToolNode,
  type CanvasToolStatus,
  CLUSTER_THRESHOLD,
  VISIBLE_TOOLS,
} from "../hooks/useConstellation";
import type { ReviewFocus } from "../types";
import {
  type CanvasPoint,
  layoutReviewerActivityPosition,
  layoutReviewerPositions,
} from "./constellationLayout";

// ── Review focus → color mapping ────────────────────────────────────────────

const FOCUS_COLOR_VAR: Record<ReviewFocus, string> = {
  Security: "var(--gospel-status-error)",
  BugHunt: "var(--gospel-accent-data)",
  Architecture: "var(--gospel-accent-structure)",
  Performance: "var(--gospel-status-warning)",
  Style: "var(--gospel-text-muted)",
};

const FOCUS_COLOR_HEX: Record<ReviewFocus, string> = {
  Security: "oklch(0.65 0.2 25)",
  BugHunt: "oklch(0.7 0.13 30)",
  Architecture: "oklch(0.72 0.14 280)",
  Performance: "oklch(0.75 0.15 85)",
  Style: "oklch(0.7 0.02 260)",
};

// ── Node position model ─────────────────────────────────────────────────────

interface NodePos {
  id: string;
  x: number;
  y: number;
  kind: "agent" | "tool" | "reviewer" | "cluster";
  ref?: CanvasToolNode | CanvasReviewerNode | CanvasToolNode[];
}

interface Edge {
  from: string;
  to: string;
  color: string;
  dashed?: boolean;
}

const MAIN_CLUSTER_PREFIX = "cluster";

function reviewerClusterId(reviewerId: string): string {
  return `reviewer-cluster:${reviewerId}`;
}

// ── Props ───────────────────────────────────────────────────────────────────

interface ConstellationCanvasProps {
  toolNodes: CanvasToolNode[];
  reviewerNodes: CanvasReviewerNode[];
  reviewVisible: boolean;
  agentRunning: boolean;
  onApprove?: (id: string) => void;
}

// ── Component ───────────────────────────────────────────────────────────────

export function ConstellationCanvas({
  toolNodes,
  reviewerNodes,
  reviewVisible,
  agentRunning,
  onApprove,
}: ConstellationCanvasProps) {
  const canvasRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 1000, h: 600 });
  const [activeReviewer, setActiveReviewer] = useState<string | null>(null);
  const [openClusterId, setOpenClusterId] = useState<string | null>(null);
  const [diffTool, setDiffTool] = useState<CanvasToolNode | null>(null);
  const clusterOpenerRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const cx = size.w / 2;
  const cy = size.h / 2 - 10;

  const mainToolNodes = useMemo(() => toolNodes.filter((tool) => !tool.reviewerId), [toolNodes]);
  const reviewerToolNodes = useMemo(() => toolNodes.filter((tool) => tool.reviewerId), [toolNodes]);

  const toolsByFocus = useMemo(() => {
    const map = new Map<string, CanvasToolNode[]>();
    for (const tool of reviewerToolNodes) {
      if (!tool.reviewerId) continue;
      const list = map.get(tool.reviewerId) ?? [];
      list.push(tool);
      map.set(tool.reviewerId, list);
    }
    return map;
  }, [reviewerToolNodes]);

  // Decide which main-agent tools render as individual nodes vs. a cluster.
  // Reviewer activity stays visible on bounded, reviewer-owned branches.
  const { visibleTools, clusteredTools } = useMemo(() => {
    if (mainToolNodes.length <= CLUSTER_THRESHOLD)
      return { visibleTools: mainToolNodes, clusteredTools: [] };
    const visible = mainToolNodes.slice(-VISIBLE_TOOLS);
    const clustered = mainToolNodes.slice(0, mainToolNodes.length - VISIBLE_TOOLS);
    return { visibleTools: visible, clusteredTools: clustered };
  }, [mainToolNodes]);
  const mainClusterId = clusteredTools.length > 0 ? MAIN_CLUSTER_PREFIX : null;

  const activeReviewers = useMemo(
    () => (reviewVisible ? reviewerNodes : []),
    [reviewerNodes, reviewVisible]
  );

  const nodes = useMemo<NodePos[]>(() => {
    const list: NodePos[] = [{ id: "agent", x: cx, y: cy, kind: "agent" }];
    const reviewerPositions = new Map<string, { x: number; y: number }>();
    const toolCount = visibleTools.length + (clusteredTools.length > 0 ? 1 : 0);
    visibleTools.forEach((tc, i) => {
      const t = toolCount <= 1 ? 0.5 : i / (toolCount - 1);
      const angle = Math.PI - 0.65 + t * 1.3;
      const radius = 200 + t * 40;
      list.push({
        id: tc.id,
        x: cx + Math.cos(angle) * radius,
        y: cy + Math.sin(angle) * radius * 0.7,
        kind: "tool",
        ref: tc,
      });
    });
    if (clusteredTools.length > 0) {
      const angle = Math.PI - 0.65;
      const radius = 200;
      list.push({
        id: mainClusterId!,
        x: cx + Math.cos(angle) * radius,
        y: cy + Math.sin(angle) * radius * 0.7,
        kind: "cluster",
        ref: clusteredTools,
      });
    }
    if (reviewVisible) {
      const positions = layoutReviewerPositions(activeReviewers.length, size, { x: cx, y: cy });
      const activityPositions: CanvasPoint[] = [];
      activeReviewers.forEach((r, i) => {
        const position = positions[i];
        reviewerPositions.set(r.id, position);
        list.push({
          id: r.id,
          ...position,
          kind: "reviewer",
          ref: r,
        });
      });

      for (const [reviewerIndex, reviewer] of activeReviewers.entries()) {
        const reviewerPosition = reviewerPositions.get(reviewer.id);
        if (!reviewerPosition) continue;
        const ownedTools = toolsByFocus.get(reviewer.focus) ?? [];
        if (ownedTools.length === 0) continue;

        const activityPosition = layoutReviewerActivityPosition(
          reviewerPosition,
          positions.filter((_, index) => index !== reviewerIndex),
          activityPositions,
          size,
          { x: cx, y: cy }
        );
        activityPositions.push(activityPosition);
        list.push({
          id: ownedTools.length === 1 ? ownedTools[0].id : reviewerClusterId(reviewer.id),
          ...activityPosition,
          kind: ownedTools.length === 1 ? "tool" : "cluster",
          ref: ownedTools.length === 1 ? ownedTools[0] : ownedTools,
        });
      }
    }
    return list;
  }, [
    visibleTools,
    clusteredTools,
    activeReviewers,
    toolsByFocus,
    cx,
    cy,
    reviewVisible,
    size,
    mainClusterId,
  ]);

  const edges = useMemo<Edge[]>(() => {
    const e: Edge[] = [];
    for (const tc of visibleTools)
      e.push({ from: "agent", to: tc.id, color: "var(--gospel-surface-line)" });
    if (mainClusterId)
      e.push({ from: "agent", to: mainClusterId, color: "var(--gospel-text-muted)" });
    if (reviewVisible)
      for (const r of activeReviewers) {
        const color = FOCUS_COLOR_VAR[r.focus] ?? "var(--gospel-text-muted)";
        e.push({ from: "agent", to: r.id, color, dashed: true });
        const ownedTools = toolsByFocus.get(r.focus) ?? [];
        if (ownedTools.length > 0)
          e.push({
            from: r.id,
            to: ownedTools.length === 1 ? ownedTools[0].id : reviewerClusterId(r.id),
            color,
          });
      }
    return e;
  }, [visibleTools, activeReviewers, toolsByFocus, reviewVisible, mainClusterId]);

  const openClusterTools = useMemo(() => {
    const cluster = nodes.find((node) => node.id === openClusterId && node.kind === "cluster");
    return cluster && Array.isArray(cluster.ref) ? cluster.ref : null;
  }, [nodes, openClusterId]);

  const closeCluster = () => {
    const opener = clusterOpenerRef.current;
    clusterOpenerRef.current = null;
    setOpenClusterId(null);
    requestAnimationFrame(() => opener?.focus());
  };

  const posOf = (id: string) => nodes.find((n) => n.id === id);

  return (
    <div className="constellation-canvas" ref={canvasRef}>
      {/* Nebula glow */}
      <div className="constellation-nebula" style={{ left: cx - 300, top: cy - 300 }} aria-hidden />

      {/* SVG edges */}
      <svg className="constellation-svg" width={size.w} height={size.h} aria-hidden>
        {edges.map((e) => {
          const a = posOf(e.from);
          const b = posOf(e.to);
          if (!a || !b) return null;
          const mx = (a.x + b.x) / 2;
          const my = (a.y + b.y) / 2 - 30;
          return (
            <path
              key={`${e.from}:${e.to}`}
              data-edge-from={e.from}
              data-edge-to={e.to}
              d={`M ${a.x} ${a.y} Q ${mx} ${my} ${b.x} ${b.y}`}
              stroke={e.color}
              strokeWidth={e.dashed ? 1.5 : 1}
              fill="none"
              strokeDasharray={e.dashed ? "4 4" : undefined}
              opacity={0.7}
            />
          );
        })}
      </svg>

      {/* Nodes */}
      {nodes.map((n) => {
        if (n.kind === "agent")
          return <AgentNode key={n.id} x={n.x} y={n.y} running={agentRunning} />;
        if (n.kind === "tool" && n.ref && "kind" in n.ref)
          return (
            <ToolNode
              key={n.id}
              x={n.x}
              y={n.y}
              tc={n.ref as CanvasToolNode}
              onApprove={onApprove}
              onShowDiff={setDiffTool}
            />
          );
        if (n.kind === "cluster" && Array.isArray(n.ref))
          return (
            <ClusterNode
              key={n.id}
              x={n.x}
              y={n.y}
              tools={n.ref as CanvasToolNode[]}
              label={n.id === mainClusterId ? "earlier" : "activity"}
              open={openClusterId === n.id}
              popoverId={clusterPopoverId(n.id)}
              onToggle={(opener) => {
                if (openClusterId === n.id) {
                  clusterOpenerRef.current = null;
                  setOpenClusterId(null);
                } else {
                  clusterOpenerRef.current = opener;
                  setOpenClusterId(n.id);
                }
              }}
            />
          );
        if (n.kind === "reviewer" && n.ref && "focus" in n.ref)
          return (
            <ReviewerNode
              key={n.id}
              x={n.x}
              y={n.y}
              r={n.ref as CanvasReviewerNode}
              active={activeReviewer === n.id}
              onHover={() => setActiveReviewer(n.id)}
              onLeave={() => setActiveReviewer(null)}
            />
          );
        return null;
      })}

      {/* Load hint */}
      {clusteredTools.length > 0 && (
        <div className="constellation-load-hint">
          canvas showing latest {visibleTools.length} · {clusteredTools.length} earlier collapsed
          <button
            type="button"
            className="constellation-load-hint-btn"
            onClick={(event) => {
              clusterOpenerRef.current = event.currentTarget;
              setOpenClusterId(mainClusterId);
            }}
          >
            view all
          </button>
        </div>
      )}

      {/* Popovers */}
      {activeReviewer && (
        <ReviewerPopover
          r={
            reviewerNodes.find((x) => x.id === activeReviewer) ??
            activeReviewers.find((x) => x.id === activeReviewer)!
          }
        />
      )}
      {openClusterTools && (
        <ClusterPopover
          tools={openClusterTools}
          title={
            openClusterId === mainClusterId
              ? `${openClusterTools.length} earlier tool calls`
              : `${openClusterTools.length} grouped tool calls`
          }
          id={clusterPopoverId(openClusterId ?? mainClusterId ?? MAIN_CLUSTER_PREFIX)}
          onClose={closeCluster}
        />
      )}
      {diffTool && <DiffPopover tc={diffTool} onClose={() => setDiffTool(null)} />}
    </div>
  );
}

// ── Agent node ──────────────────────────────────────────────────────────────

function AgentNode({ x, y, running }: { x: number; y: number; running: boolean }) {
  return (
    <div className="constellation-node-agent" style={{ left: x, top: y }}>
      <div
        className="constellation-agent-ring"
        style={{
          borderColor: running ? "var(--gospel-accent-action)" : "var(--gospel-surface-line)",
        }}
      />
      <div className="constellation-agent-core">
        <span className="constellation-agent-label">agent</span>
        <span className="constellation-agent-name">Gospel</span>
      </div>
    </div>
  );
}

// ── Tool node ───────────────────────────────────────────────────────────────

function ToolNode({
  x,
  y,
  tc,
  onApprove,
  onShowDiff,
}: {
  x: number;
  y: number;
  tc: CanvasToolNode;
  onApprove?: (id: string) => void;
  onShowDiff: (tc: CanvasToolNode) => void;
}) {
  const [hover, setHover] = useState(false);
  const compact = Boolean(tc.reviewerId);
  return (
    <div className="constellation-node-tool" style={{ left: x, top: y }}>
      <div
        className={`constellation-tool-card${compact ? " constellation-reviewer-tool-card" : ""}`}
        style={{
          borderColor:
            tc.status === "awaiting"
              ? "var(--gospel-status-warning)"
              : tc.hasDiff
                ? "color-mix(in srgb, var(--gospel-accent-signal) 40%, transparent)"
                : "var(--gospel-surface-line)",
          boxShadow:
            tc.status === "running"
              ? "0 0 16px color-mix(in srgb, var(--gospel-accent-action) 30%, transparent)"
              : "none",
          cursor: tc.hasDiff ? "pointer" : "default",
        }}
        onMouseEnter={() => setHover(true)}
        onMouseLeave={() => setHover(false)}
        onClick={() => tc.hasDiff && onShowDiff(tc)}
      >
        <span className="constellation-tool-kind" style={toolKindStyle(tc.kind)}>
          {tc.label}
        </span>
        <span className="constellation-tool-target">{tc.target.split("/").pop()}</span>
        <span className="constellation-tool-status-dot" style={toolStatusStyle(tc.status)} />
        {tc.hasDiff && <span className="constellation-diff-badge">diff</span>}
      </div>
      {hover && (
        <div className="constellation-tool-pop">
          <code className="constellation-tool-pop-code">{tc.target}</code>
          {tc.hasDiff && (
            <button type="button" className="constellation-diff-btn" onClick={() => onShowDiff(tc)}>
              view diff
            </button>
          )}
          {tc.status === "awaiting" && onApprove && (
            <button
              type="button"
              className="constellation-approve-btn"
              onClick={() => onApprove(tc.id)}
            >
              approve
            </button>
          )}
        </div>
      )}
    </div>
  );
}

function toolKindStyle(kind: CanvasToolKind): React.CSSProperties {
  const color =
    kind === "edit" || kind === "write"
      ? "var(--gospel-accent-signal)"
      : kind === "run_shell"
        ? "var(--gospel-accent-action)"
        : kind === "review"
          ? "var(--gospel-accent-structure)"
          : "var(--gospel-text-muted)";
  return {
    color,
  };
}

function toolStatusStyle(status: CanvasToolStatus): React.CSSProperties {
  const bg =
    status === "done"
      ? "var(--gospel-status-success)"
      : status === "awaiting"
        ? "var(--gospel-status-warning)"
        : status === "running"
          ? "var(--gospel-accent-action)"
          : "var(--gospel-text-muted)";
  return { background: bg };
}

// ── Cluster node ────────────────────────────────────────────────────────────

function ClusterNode({
  x,
  y,
  tools,
  label,
  open,
  popoverId,
  onToggle,
}: {
  x: number;
  y: number;
  tools: CanvasToolNode[];
  label: "activity" | "earlier";
  open: boolean;
  popoverId: string;
  onToggle: (opener: HTMLButtonElement) => void;
}) {
  return (
    <div className="constellation-node-cluster" style={{ left: x, top: y }}>
      <button
        type="button"
        className={`constellation-cluster-btn${label === "activity" ? " constellation-activity-cluster-btn" : ""}`}
        style={{
          borderColor: open ? "var(--gospel-accent-action)" : "var(--gospel-text-muted)",
          borderStyle: "dashed",
        }}
        aria-controls={popoverId}
        aria-expanded={open}
        aria-label={`${tools.length} ${label === "activity" ? "grouped reviewer tool calls" : "earlier tool calls"}`}
        onClick={(event) => onToggle(event.currentTarget)}
      >
        <span className="constellation-cluster-glyph">⇄</span>
        <span className="constellation-cluster-count">+{tools.length}</span>
        <span className="constellation-cluster-label">{label}</span>
      </button>
    </div>
  );
}

function ClusterPopover({
  tools,
  title,
  id,
  onClose,
}: {
  tools: CanvasToolNode[];
  title: string;
  id: string;
  onClose: () => void;
}) {
  const byKind = useMemo(() => {
    const m = new Map<CanvasToolKind, CanvasToolNode[]>();
    for (const t of tools) {
      const arr = m.get(t.kind) ?? [];
      arr.push(t);
      m.set(t.kind, arr);
    }
    return Array.from(m.entries());
  }, [tools]);

  return (
    <section id={id} className="constellation-cluster-pop" aria-label={title}>
      <div className="constellation-cluster-pop-head">
        <span className="constellation-cluster-pop-title">{title}</span>
        <button
          type="button"
          className="constellation-pop-close"
          aria-label="Close tool activity"
          onClick={onClose}
        >
          ×
        </button>
      </div>
      <div className="constellation-cluster-pop-body">
        {byKind.map(([kind, items]) => (
          <div key={kind} className="constellation-cluster-group">
            <div className="constellation-cluster-group-head">
              <span style={toolKindStyle(kind)}>{kind}</span>
              <span className="constellation-cluster-group-count">{items.length}</span>
            </div>
            <div className="constellation-cluster-group-list">
              {items.map((tc) => (
                <div key={tc.id} className="constellation-cluster-group-item">
                  <span
                    className="constellation-cluster-group-dot"
                    style={toolStatusStyle(tc.status)}
                  />
                  <span className="constellation-cluster-group-target">{tc.target}</span>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}

function clusterPopoverId(clusterId: string): string {
  return `constellation-cluster-popover-${clusterId.replace(/[^a-zA-Z0-9_-]/g, "-")}`;
}

// ── Diff popover ────────────────────────────────────────────────────────────

function DiffPopover({ tc, onClose }: { tc: CanvasToolNode; onClose: () => void }) {
  // Try to extract diff lines from the tool result.
  const diffLines = useMemo(() => parseDiffFromResult(tc.result), [tc.result]);
  const additions = diffLines.filter((l) => l.type === "add").length;
  const deletions = diffLines.filter((l) => l.type === "del").length;

  return (
    <div className="constellation-diff-pop">
      <div className="constellation-diff-pop-head">
        <div className="constellation-diff-pop-head-left">
          <span className="constellation-diff-pop-file">{tc.target}</span>
          <span className="constellation-diff-pop-stats">
            <span style={{ color: "var(--gospel-status-success)" }}>+{additions}</span>
            <span style={{ color: "var(--gospel-status-error)" }}>−{deletions}</span>
          </span>
        </div>
        <button type="button" className="constellation-pop-close" onClick={onClose}>
          ×
        </button>
      </div>
      <div className="constellation-diff-pop-body">
        {diffLines.length === 0 ? (
          <div className="constellation-diff-pop-empty">
            No diff content available for this tool call.
          </div>
        ) : (
          diffLines.map((l, i) => (
            <div
              key={i}
              className="constellation-diff-line"
              style={{
                background:
                  l.type === "add"
                    ? "color-mix(in srgb, var(--gospel-status-success) 5%, transparent)"
                    : l.type === "del"
                      ? "color-mix(in srgb, var(--gospel-status-error) 5%, transparent)"
                      : "transparent",
              }}
            >
              <span
                className="constellation-diff-line-sign"
                style={{
                  color:
                    l.type === "add"
                      ? "var(--gospel-status-success)"
                      : l.type === "del"
                        ? "var(--gospel-status-error)"
                        : "var(--gospel-text-muted)",
                }}
              >
                {l.type === "add" ? "+" : l.type === "del" ? "−" : " "}
              </span>
              <code
                className="constellation-diff-line-text"
                style={{
                  color:
                    l.type === "add"
                      ? "var(--gospel-status-success)"
                      : l.type === "del"
                        ? "var(--gospel-status-error)"
                        : "var(--gospel-text-secondary)",
                }}
              >
                {l.text}
              </code>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

interface ParsedDiffLine {
  type: "add" | "del" | "ctx";
  text: string;
}

function parseDiffFromResult(result?: string): ParsedDiffLine[] {
  if (!result || typeof result !== "string") return [];
  const lines = result.split("\n");
  const diffLines: ParsedDiffLine[] = [];
  let inDiff = false;
  for (const line of lines) {
    if (line.startsWith("@@")) {
      inDiff = true;
      continue;
    }
    if (inDiff) {
      if (line.startsWith("+++") || line.startsWith("---")) continue;
      if (line.startsWith("+")) diffLines.push({ type: "add", text: line.slice(1) });
      else if (line.startsWith("-")) diffLines.push({ type: "del", text: line.slice(1) });
      else if (line.startsWith(" ")) diffLines.push({ type: "ctx", text: line.slice(1) });
      else if (line === "") continue;
      else {
        // Non-diff line — stop if we were in a diff
        if (diffLines.length > 0) break;
      }
    }
  }
  return diffLines.slice(0, 200); // cap for performance
}

// ── Reviewer node ───────────────────────────────────────────────────────────

function ReviewerNode({
  x,
  y,
  r,
  active,
  onHover,
  onLeave,
}: {
  x: number;
  y: number;
  r: CanvasReviewerNode;
  active: boolean;
  onHover: () => void;
  onLeave: () => void;
}) {
  const color = FOCUS_COLOR_HEX[r.focus] ?? "var(--gospel-text-muted)";
  const isActive = r.status === "active";
  return (
    <div className="constellation-node-reviewer" style={{ left: x, top: y }}>
      <div
        className="constellation-reviewer-card"
        style={{
          borderColor: active ? color : "var(--gospel-surface-line)",
          boxShadow:
            isActive && !active
              ? `0 0 18px color-mix(in srgb, ${color} 27%, transparent)`
              : active
                ? `0 0 18px color-mix(in srgb, ${color} 40%, transparent)`
                : "none",
        }}
        onMouseEnter={onHover}
        onMouseLeave={onLeave}
      >
        <span
          className="constellation-reviewer-ring"
          style={{
            borderColor: color,
            opacity: r.status === "done" ? 0 : r.status === "idle" ? 0.2 : 1,
          }}
        />
        <span
          className="constellation-reviewer-avatar"
          style={{
            background: `color-mix(in srgb, ${color} 13%, transparent)`,
            color,
            borderColor: `color-mix(in srgb, ${color} 33%, transparent)`,
          }}
        >
          {r.name[0]}
        </span>
        <span className="constellation-reviewer-name">{r.name}</span>
        {r.model && (
          <span
            className="constellation-reviewer-model"
            title={`Review model: ${r.provider ? `${r.provider}/` : ""}${r.model}`}
          >
            {r.model}
          </span>
        )}
        {r.status === "done" && (
          <span
            className="constellation-reviewer-verdict-dot"
            style={{
              background:
                r.findings > 0 ? "var(--gospel-status-warning)" : "var(--gospel-status-success)",
            }}
          />
        )}
      </div>
    </div>
  );
}

function ReviewerPopover({ r }: { r: CanvasReviewerNode }) {
  const color = FOCUS_COLOR_HEX[r.focus] ?? "var(--gospel-text-muted)";
  const modelLabel = r.provider && r.model ? `${r.provider}/${r.model}` : "model pending";
  return (
    <div className="constellation-reviewer-pop" style={{ borderColor: color }}>
      <div className="constellation-pop-head">
        <span
          className="constellation-reviewer-avatar"
          style={{
            background: `color-mix(in srgb, ${color} 13%, transparent)`,
            color,
            borderColor: `color-mix(in srgb, ${color} 33%, transparent)`,
          }}
        >
          {r.name[0]}
        </span>
        <div>
          <div className="constellation-pop-name">{r.name}</div>
          <div className="constellation-pop-role">
            {modelLabel} · {r.status} · {r.findings} findings
          </div>
        </div>
      </div>
      <div className="constellation-pop-progress">
        <div
          className="constellation-pop-progress-fill"
          style={{ transform: `scaleX(${r.progress})`, background: color }}
        />
      </div>
      {r.comments.length === 0 ? (
        <p className="constellation-pop-empty">{r.status === "active" ? "analyzing…" : "queued"}</p>
      ) : (
        <div className="constellation-pop-comments">
          {r.comments.slice(-8).map((c, i) => (
            <div key={i} className="constellation-pop-comment">
              <span className="constellation-pop-comment-text">{c.text}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
