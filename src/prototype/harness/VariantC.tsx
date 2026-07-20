// PROTOTYPE — throwaway. Variant C: "Node Canvas Constellation"
// References: n8n workflow canvas (node-based flowchart, execution log, dark),
// Vapi (vivid accent pills, colorful equalizer-style capsule bars),
// AgentQL (nebula-console, indigo/violet glows, glassy cards).
//
// Shape: main area is a spatial canvas. The primary agent is a central node;
// each tool call spawns a child node connected by a curved edge. When the
// review pipeline starts, four reviewer subagents appear as SATELLITE NODES
// orbiting the work node, each glowing in its agent color with a live status
// ring (pulsing when active) and an edge drawn to the file they're commenting
// on. Bottom dock = prompt input + collapsible chat transcript. The unique
// twist: the review pipeline is a literal constellation you can read spatially
// — who's looking at what, all at once.
import { useEffect, useMemo, useRef, useState } from "react";
import { REVIEWER_COLOR_VAR, type Reviewer, SEVERITY_META, type ToolCall } from "./data";
import { allTools, usePlayback } from "./usePlayback";

interface NodePos {
  id: string;
  x: number;
  y: number;
  kind: "agent" | "tool" | "reviewer";
  ref?: ToolCall | Reviewer;
}

export function VariantC() {
  const { turns, reviewers, running, reviewStarted, restart, approve } = usePlayback();
  const [dockOpen, setDockOpen] = useState(true);
  const [activeReviewer, setActiveReviewer] = useState<string | null>(null);
  const canvasRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 1000, h: 600 });

  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      setSize({ w: el.clientWidth, h: el.clientHeight });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const tools = allTools(turns);
  const cx = size.w / 2;
  const cy = size.h / 2 - 20;

  // layout: agent center; tools fan out to the left in a vertical arc;
  // reviewers orbit on the right in a vertical arc.
  const nodes = useMemo<NodePos[]>(() => {
    const list: NodePos[] = [{ id: "agent", x: cx, y: cy, kind: "agent" }];
    const toolCount = tools.length;
    tools.forEach((tc, i) => {
      const t = toolCount <= 1 ? 0.5 : i / (toolCount - 1);
      const angle = Math.PI - 0.6 + t * 1.2; // left arc
      const radius = 200 + t * 40;
      list.push({
        id: tc.id,
        x: cx + Math.cos(angle) * radius,
        y: cy + Math.sin(angle) * radius * 0.7,
        kind: "tool",
        ref: tc,
      });
    });
    reviewers.forEach((r, i) => {
      const t = reviewers.length <= 1 ? 0.5 : i / (reviewers.length - 1);
      const angle = -0.6 + t * 1.2; // right arc
      const radius = 220;
      list.push({
        id: r.id,
        x: cx + Math.cos(angle) * radius,
        y: cy + Math.sin(angle) * radius * 0.75,
        kind: "reviewer",
        ref: r,
      });
    });
    return list;
  }, [tools, reviewers, cx, cy]);

  const edges = useMemo(() => {
    const e: { from: string; to: string; color: string; dashed?: boolean }[] = [];
    for (const tc of tools) e.push({ from: "agent", to: tc.id, color: "var(--surface-line)" });
    if (reviewStarted) {
      for (const r of reviewers) {
        e.push({ from: "agent", to: r.id, color: REVIEWER_COLOR_VAR[r.color], dashed: true });
      }
    }
    return e;
  }, [tools, reviewers, reviewStarted]);

  const posOf = (id: string) => nodes.find((n) => n.id === id);

  return (
    <div style={shell}>
      <header style={topbar}>
        <div style={topLeft}>
          <span style={orbitGlyph(running ? "var(--accent-action)" : "var(--text-muted)")} />
          <span style={topTitle}>Constellation</span>
          <span style={topSub}>
            {reviewStarted
              ? `${reviewers.filter((r) => r.verdict).length}/${reviewers.length} verdicts`
              : "agent working"}
          </span>
        </div>
        <div style={topRight}>
          <button type="button" style={ghostBtn} onClick={() => setDockOpen((v) => !v)}>
            {dockOpen ? "hide transcript" : "show transcript"}
          </button>
          <button type="button" style={ghostBtn} onClick={restart}>
            replay
          </button>
        </div>
      </header>

      <div style={canvasWrap}>
        <div style={canvas} ref={canvasRef}>
          {/* nebula glow */}
          <div style={nebula(cx, cy)} aria-hidden />
          <svg style={svgLayer} width={size.w} height={size.h}>
            {edges.map((e, i) => {
              const a = posOf(e.from);
              const b = posOf(e.to);
              if (!a || !b) return null;
              const mx = (a.x + b.x) / 2;
              const my = (a.y + b.y) / 2 - 30;
              return (
                <path
                  key={i}
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

          {nodes.map((n) => {
            if (n.kind === "agent")
              return <AgentNode key={n.id} x={n.x} y={n.y} running={running} />;
            if (n.kind === "tool" && n.ref && "kind" in n.ref)
              return (
                <ToolNode key={n.id} x={n.x} y={n.y} tc={n.ref as ToolCall} onApprove={approve} />
              );
            if (n.kind === "reviewer" && n.ref && "color" in n.ref)
              return (
                <ReviewerNode
                  key={n.id}
                  x={n.x}
                  y={n.y}
                  r={n.ref as Reviewer}
                  active={activeReviewer === n.id}
                  onHover={() => setActiveReviewer(n.id)}
                  onLeave={() => setActiveReviewer(null)}
                />
              );
            return null;
          })}

          {/* equalizer-style activity bar at top of canvas */}
          <div style={equalizer(running)}>
            {Array.from({ length: 24 }).map((_, i) => (
              <span key={i} style={eqBar(i, running)} />
            ))}
          </div>
        </div>

        {/* reviewer detail popover */}
        {activeReviewer && <ReviewerPopover r={reviewers.find((x) => x.id === activeReviewer)!} />}
      </div>

      <Dock
        open={dockOpen}
        turns={turns}
        running={running}
        onApprove={approve}
        onToggle={() => setDockOpen((v) => !v)}
      />
    </div>
  );
}

function AgentNode({ x, y, running }: { x: number; y: number; running: boolean }) {
  return (
    <div style={{ position: "absolute", left: x, top: y, transform: "translate(-50%,-50%)" }}>
      <div style={agentRing(running)} />
      <div style={agentNode}>
        <span style={agentLabel}>agent</span>
        <span style={agentName}>Gospel</span>
      </div>
    </div>
  );
}

function ToolNode({
  x,
  y,
  tc,
  onApprove,
}: {
  x: number;
  y: number;
  tc: ToolCall;
  onApprove: (id: string) => void;
}) {
  const [hover, setHover] = useState(false);
  return (
    <div style={{ position: "absolute", left: x, top: y, transform: "translate(-50%,-50%)" }}>
      <div
        style={toolNode(tc.status)}
        onMouseEnter={() => setHover(true)}
        onMouseLeave={() => setHover(false)}
      >
        <span style={toolNodeKind(tc.kind)}>{tc.kind}</span>
        <span style={toolNodeTarget}>{tc.target.split("/").pop()}</span>
        <span style={toolNodeStatus(tc.status)} />
      </div>
      {hover && (
        <div style={toolPop}>
          <code style={toolPopCode}>{tc.target}</code>
          {tc.detail && <p style={toolPopDetail}>{tc.detail}</p>}
          {tc.needsApproval && tc.status === "awaiting" && (
            <button type="button" style={approveBtn} onClick={() => onApprove(tc.id)}>
              approve
            </button>
          )}
        </div>
      )}
    </div>
  );
}

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
  r: Reviewer;
  active: boolean;
  onHover: () => void;
  onLeave: () => void;
}) {
  const color = REVIEWER_COLOR_VAR[r.color];
  return (
    <div style={{ position: "absolute", left: x, top: y, transform: "translate(-50%,-50%)" }}>
      <div
        style={reviewerNode(color, active, r.status)}
        onMouseEnter={onHover}
        onMouseLeave={onLeave}
      >
        <span style={reviewerRing(color, r.status)} />
        <span style={reviewerAvatar(color)}>{r.name[0]}</span>
        <span style={reviewerNodeName}>{r.name}</span>
        <span style={reviewerNodeRole}>{r.role}</span>
        {r.verdict && <span style={reviewerVerdictDot(r.verdict)} />}
      </div>
    </div>
  );
}

function ReviewerPopover({ r }: { r: Reviewer }) {
  const color = REVIEWER_COLOR_VAR[r.color];
  return (
    <div style={popover(color)}>
      <div style={popHead}>
        <span style={reviewerAvatar(color)}>{r.name[0]}</span>
        <div>
          <div style={popName}>{r.name}</div>
          <div style={popRole}>
            {r.role} · {r.status}
          </div>
        </div>
      </div>
      <div style={popProgress}>
        <div style={popProgressFill(color, r.progress)} />
      </div>
      {r.comments.length === 0 ? (
        <p style={popEmpty}>{r.nowCommenting ? `reading ${r.nowCommenting}` : "queued"}</p>
      ) : (
        <div style={popComments}>
          {r.comments.map((c, i) => {
            const sev = SEVERITY_META[c.severity];
            return (
              <div key={i} style={popComment}>
                <span style={popSev(sev.color)}>{sev.label}</span>
                <span style={popLine}>L{c.line}</span>
                <p style={popText}>{c.text}</p>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function Dock({
  open,
  turns,
  running,
  onApprove,
  onToggle,
}: {
  open: boolean;
  turns: import("./data").AgentTurn[];
  running: boolean;
  onApprove: (id: string) => void;
  onToggle: () => void;
}) {
  const [v, setV] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [turns, open]);
  return (
    <div style={dock(open)}>
      <button type="button" style={dockToggle} onClick={onToggle}>
        {open ? "▾" : "▴"} transcript
      </button>
      {open && (
        <>
          <div style={dockStream} ref={scrollRef}>
            {turns.map((t) => (
              <div key={t.id} style={dockTurn(t.role === "user")}>
                <span style={dockRole(t.role === "user")}>
                  {t.role === "user" ? "you" : "agent"}
                </span>
                <span style={dockText}>{t.text}</span>
                {t.tools?.some((tc) => tc.needsApproval && tc.status === "awaiting") && (
                  <button
                    type="button"
                    style={approveBtn}
                    onClick={() => {
                      const tc = t.tools!.find((x) => x.needsApproval && x.status === "awaiting");
                      if (tc) onApprove(tc.id);
                    }}
                  >
                    approve edit
                  </button>
                )}
              </div>
            ))}
          </div>
          <div style={dockInput}>
            <span style={dockChevron}>❯</span>
            <input
              style={dockField}
              placeholder={running ? "agent working…" : "prompt the agent…"}
              value={v}
              disabled={running}
              onChange={(e) => setV(e.target.value)}
            />
          </div>
        </>
      )}
    </div>
  );
}

/* ── styles ────────────────────────────────────────────────────────────── */
const shell: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  height: "100vh",
  background: "var(--surface-base)",
  color: "var(--text-primary)",
  fontFamily: "var(--font-body)",
  overflow: "hidden",
};
const topbar: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
  padding: "12px 20px",
  borderBottom: "1px solid var(--surface-line)",
  background: "var(--surface-sunken)",
  flexShrink: 0,
};
const topLeft: React.CSSProperties = { display: "flex", alignItems: "center", gap: 12 };
const topRight: React.CSSProperties = { display: "flex", gap: 8 };
const topTitle: React.CSSProperties = { fontSize: 15, fontWeight: 600, letterSpacing: "-0.01em" };
const topSub: React.CSSProperties = {
  color: "var(--text-muted)",
  fontSize: 12,
  fontFamily: "var(--font-mono)",
};
const ghostBtn: React.CSSProperties = {
  background: "transparent",
  border: "1px solid var(--surface-line)",
  color: "var(--text-secondary)",
  borderRadius: "var(--radius-full)",
  padding: "4px 12px",
  fontSize: 12,
  cursor: "pointer",
  fontFamily: "var(--font-mono)",
};
const orbitGlyph = (c: string): React.CSSProperties => ({
  width: 14,
  height: 14,
  borderRadius: "50%",
  border: `2px solid ${c}`,
  borderTopColor: "transparent",
  animation: "proto-spin 1.4s linear infinite",
});
const canvasWrap: React.CSSProperties = { flex: 1, position: "relative", minHeight: 0 };
const canvas: React.CSSProperties = {
  position: "absolute",
  inset: 0,
  overflow: "hidden",
  background: "var(--surface-base)",
};
const nebula = (x: number, y: number): React.CSSProperties => ({
  position: "absolute",
  left: x - 300,
  top: y - 300,
  width: 600,
  height: 600,
  pointerEvents: "none",
  background: "radial-gradient(circle, rgba(120,90,255,0.12), transparent 60%)",
  filter: "blur(20px)",
});
const svgLayer: React.CSSProperties = { position: "absolute", inset: 0, pointerEvents: "none" };

const agentRing = (running: boolean): React.CSSProperties => ({
  position: "absolute",
  inset: -10,
  borderRadius: "50%",
  border: `1.5px solid ${running ? "var(--accent-action)" : "var(--surface-line)"}`,
  animation: running ? "proto-pulse-ring 1.8s ease-out infinite" : "none",
});
const agentNode: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  justifyContent: "center",
  width: 84,
  height: 84,
  borderRadius: "50%",
  background: "var(--surface-elevated)",
  border: "1px solid var(--surface-line)",
  boxShadow: "0 0 24px rgba(120,90,255,0.25)",
};
const agentLabel: React.CSSProperties = {
  fontSize: 9,
  textTransform: "uppercase",
  letterSpacing: "0.14em",
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
};
const agentName: React.CSSProperties = { fontSize: 14, fontWeight: 600, marginTop: 2 };

const toolNode = (s: ToolCall["status"]): React.CSSProperties => ({
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: 2,
  padding: "8px 12px",
  borderRadius: "var(--radius-md)",
  background: "var(--surface-elevated)",
  border: `1px solid ${s === "awaiting" ? "var(--status-warning)" : "var(--surface-line)"}`,
  minWidth: 110,
  cursor: "pointer",
  boxShadow: s === "running" ? "0 0 16px rgba(120,200,180,0.3)" : "none",
});
const toolNodeKind = (k: ToolCall["kind"]): React.CSSProperties => ({
  fontSize: 9,
  textTransform: "uppercase",
  letterSpacing: "0.12em",
  fontFamily: "var(--font-mono)",
  color:
    k === "edit" || k === "write"
      ? "var(--accent-signal)"
      : k === "run_shell"
        ? "var(--accent-action)"
        : "var(--text-muted)",
});
const toolNodeTarget: React.CSSProperties = {
  fontSize: 11,
  fontFamily: "var(--font-mono)",
  color: "var(--text-primary)",
  maxWidth: 120,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};
const toolNodeStatus = (s: ToolCall["status"]): React.CSSProperties => ({
  width: 6,
  height: 6,
  borderRadius: "50%",
  marginTop: 2,
  background:
    s === "done"
      ? "var(--status-success)"
      : s === "awaiting"
        ? "var(--status-warning)"
        : s === "running"
          ? "var(--accent-action)"
          : "var(--text-muted)",
});
const toolPop: React.CSSProperties = {
  position: "absolute",
  bottom: "100%",
  left: "50%",
  transform: "translateX(-50%)",
  marginBottom: 8,
  background: "var(--surface-overlay)",
  border: "1px solid var(--surface-line)",
  borderRadius: "var(--radius-md)",
  padding: 10,
  width: 240,
  boxShadow: "var(--shadow-floating)",
  zIndex: 5,
};
const toolPopCode: React.CSSProperties = {
  display: "block",
  fontFamily: "var(--font-mono)",
  fontSize: 10,
  color: "var(--text-secondary)",
  wordBreak: "break-all",
};
const toolPopDetail: React.CSSProperties = {
  margin: "6px 0 0",
  fontSize: 11,
  color: "var(--text-muted)",
  lineHeight: 1.4,
};
const approveBtn: React.CSSProperties = {
  marginTop: 8,
  background: "var(--accent-action)",
  color: "var(--text-inverse)",
  border: "none",
  borderRadius: "var(--radius-full)",
  padding: "4px 12px",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
  cursor: "pointer",
  fontWeight: 600,
};

const reviewerNode = (
  color: string,
  active: boolean,
  status: Reviewer["status"]
): React.CSSProperties => ({
  position: "relative",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: 4,
  padding: "10px 12px",
  borderRadius: "var(--radius-md)",
  background: "var(--surface-elevated)",
  border: `1px solid ${active ? color : "var(--surface-line)"}`,
  minWidth: 96,
  cursor: "pointer",
  boxShadow: status !== "done" && status !== "queued" ? `0 0 18px ${color}44` : "none",
});
const reviewerRing = (color: string, status: Reviewer["status"]): React.CSSProperties => ({
  position: "absolute",
  inset: -4,
  borderRadius: "var(--radius-lg)",
  pointerEvents: "none",
  border: `1.5px solid ${color}`,
  opacity: status === "done" ? 0 : status === "queued" ? 0.2 : 1,
  animation:
    status !== "done" && status !== "queued" ? "proto-pulse-ring 1.6s ease-out infinite" : "none",
});
const reviewerAvatar = (color: string): React.CSSProperties => ({
  width: 24,
  height: 24,
  borderRadius: "50%",
  display: "grid",
  placeItems: "center",
  background: `${color}22`,
  color,
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  fontWeight: 600,
  border: `1px solid ${color}55`,
});
const reviewerNodeName: React.CSSProperties = { fontSize: 12, fontWeight: 600 };
const reviewerNodeRole: React.CSSProperties = {
  fontSize: 9,
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
  textTransform: "uppercase",
  letterSpacing: "0.08em",
};
const reviewerVerdictDot = (v: Reviewer["verdict"]): React.CSSProperties => ({
  position: "absolute",
  top: 6,
  right: 6,
  width: 8,
  height: 8,
  borderRadius: "50%",
  background: v === "approve" ? "var(--status-success)" : "var(--status-warning)",
});

const popover = (color: string): React.CSSProperties => ({
  position: "absolute",
  top: 60,
  right: 20,
  width: 300,
  background: "var(--surface-overlay)",
  border: `1px solid ${color}`,
  borderRadius: "var(--radius-md)",
  padding: 14,
  boxShadow: "var(--shadow-floating)",
  zIndex: 10,
});
const popHead: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 10,
  marginBottom: 10,
};
const popName: React.CSSProperties = { fontSize: 13, fontWeight: 600 };
const popRole: React.CSSProperties = {
  fontSize: 10,
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
};
const popProgress: React.CSSProperties = {
  height: 3,
  background: "var(--surface-line)",
  borderRadius: 2,
  overflow: "hidden",
  marginBottom: 10,
};
const popProgressFill = (color: string, p: number): React.CSSProperties => ({
  height: "100%",
  width: `${p * 100}%`,
  background: color,
  transition: "width 600ms var(--ease-out-quart)",
});
const popEmpty: React.CSSProperties = {
  fontSize: 11,
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
  margin: 0,
};
const popComments: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 8,
  maxHeight: 240,
  overflowY: "auto",
};
const popComment: React.CSSProperties = {
  background: "var(--surface-sunken)",
  borderRadius: "var(--radius-sm)",
  padding: 8,
};
const popSev = (c: string): React.CSSProperties => ({
  fontSize: 9,
  textTransform: "uppercase",
  letterSpacing: "0.1em",
  fontFamily: "var(--font-mono)",
  color: c,
  fontWeight: 600,
});
const popLine: React.CSSProperties = {
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
  marginLeft: 6,
};
const popText: React.CSSProperties = {
  margin: "4px 0 0",
  fontSize: 11,
  lineHeight: 1.45,
  color: "var(--text-secondary)",
};

const equalizer = (running: boolean): React.CSSProperties => ({
  position: "absolute",
  top: 16,
  left: "50%",
  transform: "translateX(-50%)",
  display: "flex",
  gap: 3,
  alignItems: "flex-end",
  height: 18,
  opacity: running ? 1 : 0.3,
});
const eqBar = (i: number, running: boolean): React.CSSProperties => ({
  width: 3,
  background: [
    "var(--agent-cyan)",
    "var(--agent-violet)",
    "var(--agent-amber)",
    "var(--agent-rose)",
  ][i % 4],
  borderRadius: 2,
  height: running ? `${30 + Math.abs(Math.sin(i * 0.7)) * 70}%` : "20%",
  animation: running ? `proto-eq ${0.6 + (i % 5) * 0.1}s ease-in-out infinite alternate` : "none",
  animationDelay: `${i * 0.05}s`,
});

const dock = (open: boolean): React.CSSProperties => ({
  height: open ? 220 : 44,
  flexShrink: 0,
  borderTop: "1px solid var(--surface-line)",
  background: "var(--surface-sunken)",
  display: "flex",
  flexDirection: "column",
  transition: "height 200ms var(--ease-out-quart)",
});
const dockToggle: React.CSSProperties = {
  background: "transparent",
  border: "none",
  color: "var(--text-muted)",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
  padding: "12px 20px",
  cursor: "pointer",
  textAlign: "left",
  textTransform: "uppercase",
  letterSpacing: "0.12em",
};
const dockStream: React.CSSProperties = {
  flex: 1,
  overflowY: "auto",
  padding: "0 20px 8px",
  display: "flex",
  flexDirection: "column",
  gap: 6,
};
const dockTurn = (_isUser: boolean): React.CSSProperties => ({
  display: "flex",
  gap: 10,
  alignItems: "flex-start",
  padding: "4px 0",
});
const dockRole = (isUser: boolean): React.CSSProperties => ({
  fontSize: 10,
  textTransform: "uppercase",
  letterSpacing: "0.12em",
  fontFamily: "var(--font-mono)",
  color: isUser ? "var(--accent-action)" : "var(--accent-structure)",
  minWidth: 40,
  flexShrink: 0,
  paddingTop: 2,
});
const dockText: React.CSSProperties = {
  fontSize: 12,
  color: "var(--text-secondary)",
  lineHeight: 1.5,
  flex: 1,
};
const dockInput: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 10,
  padding: "8px 20px",
  borderTop: "1px solid var(--surface-line)",
};
const dockChevron: React.CSSProperties = {
  color: "var(--accent-action)",
  fontFamily: "var(--font-mono)",
  fontSize: 14,
};
const dockField: React.CSSProperties = {
  flex: 1,
  background: "transparent",
  border: "none",
  outline: "none",
  color: "var(--text-primary)",
  fontFamily: "var(--font-mono)",
  fontSize: 13,
};
