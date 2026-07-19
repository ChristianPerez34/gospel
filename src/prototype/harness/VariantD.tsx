// PROTOTYPE — throwaway. Variant D: "Workbench Constellation" (the synthesis).
//
// Verdict from review: Variant C captivated most, but (1) the terminal input
// wasn't obvious and (2) canvas load is a worry when an agent edits many files.
// Variant A's terminal writing was "perfect"; Variant B's activity log was
// interesting. This variant merges all three:
//
//   C's constellation (agent center, reviewers orbit right, tools fan left)
// + A's always-visible terminal prompt bar at the bottom (prominent, mono, ❯)
// + B's activity rail on the left (full tool timeline + prompt history) as the
//   scalable escape hatch
// + CLUSTERING: when tool nodes exceed a threshold, older ones collapse into a
//   "+N earlier" node on the arc; the activity rail always holds the full list.
//   This directly addresses the "agent edits 50 files" load worry — the canvas
//   stays readable, the rail stays complete.
//
// References: n8n (node canvas), Vapi (equalizer), AgentQL (nebula glow) for the
// canvas; Linear Changelog + Warp for the terminal prompt; Cursor PR workspace
// + Parallel deep research for the activity rail.
import { useEffect, useMemo, useRef, useState } from "react";
import {
  REVIEWER_COLOR_VAR,
  SEVERITY_META,
  type AgentTurn,
  type DiffLine,
  type Reviewer,
  type ToolCall,
} from "./data";
import { allTools, usePlayback } from "./usePlayback";

const CLUSTER_THRESHOLD = 12; // beyond this, collapse older tool nodes
const VISIBLE_TOOLS = 10; // how many recent tool nodes stay expanded on the arc

interface NodePos {
  id: string;
  x: number;
  y: number;
  kind: "agent" | "tool" | "reviewer" | "cluster";
  ref?: ToolCall | Reviewer | ToolCall[];
}

export function VariantD() {
  const { turns, reviewers, running, reviewStarted, restart, approve } = usePlayback();
  const [activeReviewer, setActiveReviewer] = useState<string | null>(null);
  const [leftTab, setLeftTab] = useState<"conversation" | "reviewers">("conversation");
  const [clusterOpen, setClusterOpen] = useState(false);
  const [diffTool, setDiffTool] = useState<ToolCall | null>(null);
  const [colW, setColW] = useState(380);
  const canvasRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 1000, h: 600 });

  // Draggable splitter — resize the left column between min and max.
  const draggingRef = useRef(false);
  const onSplitterDown = () => {
    draggingRef.current = true;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  };
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

  // Auto-switch to the Reviewers tab when the review pipeline starts, so the
  // user sees the parallel review happen. They can switch back to Conversation
  // to keep steering. Only fires once per run.
  const switchedRef = useRef(false);
  useEffect(() => {
    if (reviewStarted && !switchedRef.current) {
      switchedRef.current = true;
      setLeftTab("reviewers");
    }
    if (!reviewStarted) switchedRef.current = false;
  }, [reviewStarted]);

  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const tools = allTools(turns);
  const cx = size.w / 2;
  const cy = size.h / 2 - 10;

  // Decide which tools render as individual nodes vs. a cluster.
  const { visibleTools, clusteredTools } = useMemo(() => {
    if (tools.length <= CLUSTER_THRESHOLD) return { visibleTools: tools, clusteredTools: [] };
    const visible = tools.slice(-VISIBLE_TOOLS);
    const clustered = tools.slice(0, tools.length - VISIBLE_TOOLS);
    return { visibleTools: visible, clusteredTools: clustered };
  }, [tools]);

  const nodes = useMemo<NodePos[]>(() => {
    const list: NodePos[] = [{ id: "agent", x: cx, y: cy, kind: "agent" }];
    const toolCount = visibleTools.length + (clusteredTools.length > 0 ? 1 : 0);
    visibleTools.forEach((tc, i) => {
      const t = toolCount <= 1 ? 0.5 : i / (toolCount - 1);
      const angle = Math.PI - 0.65 + t * 1.3; // left arc
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
      // place the cluster node at the top of the left arc
      const angle = Math.PI - 0.65;
      const radius = 200;
      list.push({
        id: "cluster",
        x: cx + Math.cos(angle) * radius,
        y: cy + Math.sin(angle) * radius * 0.7,
        kind: "cluster",
        ref: clusteredTools,
      });
    }
    // Reviewers only appear on the canvas when the review pipeline is active.
    // Before that, the right side is empty — the canvas is just the agent + its
    // tools. When review starts, reviewer nodes fade in and orbit the right arc.
    if (reviewStarted) {
      reviewers.forEach((r, i) => {
        const t = reviewers.length <= 1 ? 0.5 : i / (reviewers.length - 1);
        const angle = -0.65 + t * 1.3; // right arc
        const radius = 230;
        list.push({
          id: r.id,
          x: cx + Math.cos(angle) * radius,
          y: cy + Math.sin(angle) * radius * 0.75,
          kind: "reviewer",
          ref: r,
        });
      });
    }
    return list;
  }, [visibleTools, clusteredTools, reviewers, cx, cy, reviewStarted]);

  const edges = useMemo(() => {
    const e: { from: string; to: string; color: string; dashed?: boolean }[] = [];
    for (const tc of visibleTools) e.push({ from: "agent", to: tc.id, color: "var(--surface-line)" });
    if (clusteredTools.length > 0)
      e.push({ from: "agent", to: "cluster", color: "var(--text-muted)" });
    if (reviewStarted)
      for (const r of reviewers)
        e.push({ from: "agent", to: r.id, color: REVIEWER_COLOR_VAR[r.color], dashed: true });
    return e;
  }, [visibleTools, clusteredTools, reviewers, reviewStarted]);

  const posOf = (id: string) => nodes.find((n) => n.id === id);

  return (
    <div style={shell}>
      <header style={topbar}>
        <div style={topLeft}>
          <span style={orbitGlyph(running ? "var(--accent-action)" : "var(--text-muted)")} />
          <span style={topTitle}>Workbench Constellation</span>
          <span style={topSub}>
            {reviewStarted
              ? `${reviewers.filter((r) => r.verdict).length}/${reviewers.length} verdicts`
              : running
                ? "agent working"
                : "idle"}
          </span>
        </div>
        <div style={topRight}>
          <button type="button" style={ghostBtn} onClick={restart}>
            replay
          </button>
        </div>
      </header>

      <div style={body}>
        {/* Left: tabbed column — Conversation (history + composer) | Reviewers.
            Conversation is the default and the chat mental model home; the tab
            auto-switches to Reviewers when the review pipeline starts.
            Draggable splitter resizes the column (min 280, max 640). */}
        <aside style={{ ...leftColumn, width: colW }}>
          <div style={tabBar}>
            <button
              type="button"
              style={tabBtn(leftTab === "conversation")}
              onClick={() => setLeftTab("conversation")}
            >
              Conversation
              {turns.length > 0 && <span style={tabBadge}>{turns.length}</span>}
            </button>
            <button
              type="button"
              style={tabBtn(leftTab === "reviewers")}
              onClick={() => setLeftTab("reviewers")}
            >
              Reviewers
              {reviewStarted && (
                <span style={tabBadgeLive}>
                  {reviewers.filter((r) => r.status !== "done" && r.status !== "queued").length} live
                </span>
              )}
            </button>
          </div>

          {leftTab === "conversation" ? (
            <ConversationTab
              turns={turns}
              running={running}
              onApprove={approve}
            />
          ) : (
            <ReviewersTab
              reviewers={reviewers}
              reviewStarted={reviewStarted}
              activeReviewer={activeReviewer}
              onHover={setActiveReviewer}
              onLeave={() => setActiveReviewer(null)}
            />
          )}
        </aside>

        {/* Draggable splitter */}
        <div style={splitter} onMouseDown={onSplitterDown} />

        {/* Right: constellation canvas — full height, the spatial view */}
        <div style={canvasWrap}>
          <div style={canvas} ref={canvasRef}>
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
              if (n.kind === "agent") return <AgentNode key={n.id} x={n.x} y={n.y} running={running} />;
              if (n.kind === "tool" && n.ref && "kind" in n.ref)
                return (
                  <ToolNode
                    key={n.id}
                    x={n.x}
                    y={n.y}
                    tc={n.ref as ToolCall}
                    onApprove={approve}
                    onShowDiff={setDiffTool}
                  />
                );
              if (n.kind === "cluster" && Array.isArray(n.ref))
                return (
                  <ClusterNode
                    key={n.id}
                    x={n.x}
                    y={n.y}
                    tools={n.ref as ToolCall[]}
                    open={clusterOpen}
                    onToggle={() => setClusterOpen((v) => !v)}
                  />
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

            <div style={equalizer(running)}>
              {Array.from({ length: 24 }).map((_, i) => (
                <span key={i} style={eqBar(i, running)} />
              ))}
            </div>

            {clusteredTools.length > 0 && (
              <div style={loadHint}>
                canvas showing latest {visibleTools.length} · {clusteredTools.length} earlier collapsed
                <button
                  type="button"
                  style={loadHintBtn}
                  onClick={() => setClusterOpen(true)}
                >
                  view all
                </button>
              </div>
            )}
          </div>

          {activeReviewer && (
            <ReviewerPopover r={reviewers.find((x) => x.id === activeReviewer)!} />
          )}

          {clusterOpen && <ClusterPopover tools={clusteredTools} onClose={() => setClusterOpen(false)} />}

          {diffTool && <DiffPopover tc={diffTool} onClose={() => setDiffTool(null)} />}
        </div>
      </div>
    </div>
  );
}

// Conversation tab — the chat mental model home. Full history (reasoning +
// response + tool chips) auto-scrolling, approval bar above the input, and the
// prompt composer pinned at the bottom of the column.
function ConversationTab({
  turns,
  running,
  onApprove,
}: {
  turns: AgentTurn[];
  running: boolean;
  onApprove: (id: string) => void;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [turns, running]);

  const awaiting = turns
    .flatMap((t) => t.tools ?? [])
    .find((tc) => tc.needsApproval && tc.status === "awaiting");

  return (
    <div style={convTab}>
      {awaiting && (
        <div style={approvalBar}>
          <span style={approvalDot} />
          <span style={approvalText}>
            agent wants to edit <code style={approvalCode}>{awaiting.target}</code>
          </span>
          <button type="button" style={approveBtn} onClick={() => onApprove(awaiting.id)}>
            approve
          </button>
        </div>
      )}

      <div style={convStream} ref={scrollRef}>
        {turns.length === 0 && (
          <div style={convEmpty}>waiting for the agent to start…</div>
        )}
        {turns.map((t) => (
          <TranscriptTurn key={t.id} turn={t} />
        ))}
        {running && (
          <div style={liveReasoning}>
            <span style={liveReasoningDot} />
            <span style={liveReasoningText}>agent is reasoning…</span>
          </div>
        )}
      </div>

      <Composer disabled={running} />
    </div>
  );
}

// Reviewers tab — the simultaneous-reviewer view. Summary pills + one card per
// reviewer with live status, progress, streaming comments, verdict.
function ReviewersTab({
  reviewers,
  reviewStarted,
  activeReviewer,
  onHover,
  onLeave,
}: {
  reviewers: Reviewer[];
  reviewStarted: boolean;
  activeReviewer: string | null;
  onHover: (id: string) => void;
  onLeave: () => void;
}) {
  return (
    <div style={reviewersTab}>
      <div style={panelSummary}>
        <span style={summaryPill("var(--status-success)")}>
          {reviewers.filter((r) => r.verdict === "approve").length} approve
        </span>
        <span style={summaryPill("var(--status-warning)")}>
          {reviewers.filter((r) => r.verdict === "request_changes").length} changes
        </span>
        <span style={panelMeta}>
          {reviewStarted
            ? `${reviewers.filter((r) => r.verdict).length}/${reviewers.length} verdicts`
            : "queued"}
        </span>
      </div>
      <div style={panelList}>
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

// Composer — the prompt input, pinned at the bottom of the conversation column.
// Includes the agent controls row: model selector, variant selector, and
// build/plan mode toggle. All "how should the agent work" controls live here,
// next to where the user steers.
const MODELS = ["Claude", "GPT-4o", "Gemini", "Llama"] as const;
const VARIANTS: Record<string, readonly string[]> = {
  Claude: ["Haiku", "Sonnet", "Opus"],
  "GPT-4o": ["mini", "standard", "o3"],
  Gemini: ["Flash", "Pro", "Ultra"],
  Llama: ["8B", "70B", "405B"],
};
type Mode = "build" | "plan";

function Composer({ disabled }: { disabled: boolean }) {
  const [v, setV] = useState("");
  const [model, setModel] = useState<string>("Claude");
  const [variant, setVariant] = useState<string>("Sonnet");
  const [mode, setMode] = useState<Mode>("build");
  const [modelOpen, setModelOpen] = useState(false);
  const [variantOpen, setVariantOpen] = useState(false);
  const canSend = !disabled && v.trim().length > 0;

  const selectModel = (m: string) => {
    setModel(m);
    setVariant(VARIANTS[m][0]);
    setModelOpen(false);
  };

  return (
    <div style={inputWrap}>
      {/* controls row — model, variant, mode toggle */}
      <div style={controlsRow}>
        <div style={selectorGroup}>
          {/* model selector */}
          <div style={selectorWrap}>
            <button
              type="button"
              style={selectorBtn(modelOpen)}
              onClick={() => {
                setModelOpen((v) => !v);
                setVariantOpen(false);
              }}
            >
              {model}
              <span style={selectorCaret}>▾</span>
            </button>
            {modelOpen && (
              <div style={selectorMenu}>
                {MODELS.map((m) => (
                  <button
                    key={m}
                    type="button"
                    style={selectorItem(m === model)}
                    onClick={() => selectModel(m)}
                  >
                    {m}
                    {m === model && <span style={selectorCheck}>✓</span>}
                  </button>
                ))}
              </div>
            )}
          </div>
          {/* variant selector */}
          <div style={selectorWrap}>
            <button
              type="button"
              style={selectorBtn(variantOpen)}
              onClick={() => {
                setVariantOpen((v) => !v);
                setModelOpen(false);
              }}
            >
              {variant}
              <span style={selectorCaret}>▾</span>
            </button>
            {variantOpen && (
              <div style={selectorMenu}>
                {(VARIANTS[model] ?? []).map((va) => (
                  <button
                    key={va}
                    type="button"
                    style={selectorItem(va === variant)}
                    onClick={() => {
                      setVariant(va);
                      setVariantOpen(false);
                    }}
                  >
                    {va}
                    {va === variant && <span style={selectorCheck}>✓</span>}
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>
        {/* mode toggle — build / plan */}
        <div style={modeToggle}>
          <button
            type="button"
            style={modeBtn(mode === "build")}
            onClick={() => setMode("build")}
          >
            build
          </button>
          <button
            type="button"
            style={modeBtn(mode === "plan")}
            onClick={() => setMode("plan")}
          >
            plan
          </button>
        </div>
      </div>

      {/* input card */}
      <div style={inputInner}>
        <textarea
          style={inputField}
          placeholder={
            disabled
              ? "agent is working — you can steer anytime"
              : mode === "plan"
                ? "Describe what you want — Gospel will plan before building…"
                : "Message Gospel…  (⏎ to send · ⇧⏎ for newline · / for skills)"
          }
          value={v}
          disabled={disabled}
          onChange={(e) => setV(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && canSend) {
              e.preventDefault();
              setV("");
            }
          }}
          rows={2}
        />
        <button type="button" style={sendBtn(canSend)} disabled={!canSend}>
          {mode === "plan" ? "Plan" : "Send"}
          <span style={sendHint}>⏎</span>
        </button>
      </div>
    </div>
  );
}

function TranscriptTurn({ turn }: { turn: AgentTurn }) {
  const isUser = turn.role === "user";
  return (
    <div style={tTurn(isUser)}>
      <div style={tHead}>
        <span style={tAvatar(isUser)}>{isUser ? "you" : "G"}</span>
        <span style={tRole(isUser)}>{isUser ? "you" : "agent"}</span>
      </div>
      {turn.reasoning && !isUser && (
        <div style={tReasoning}>
          <span style={tReasoningLabel}>reasoning</span>
          <p style={tReasoningText}>{turn.reasoning}</p>
        </div>
      )}
      <p style={tText(isUser)}>{turn.text}</p>
      {turn.tools && turn.tools.length > 0 && (
        <div style={tTools}>
          {turn.tools.map((tc) => (
            <span key={tc.id} style={tToolChip}>
              <span style={tToolDot(tc.status)} />
              {tc.kind} · {tc.target.split("/").pop()}
            </span>
          ))}
        </div>
      )}
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
  onShowDiff,
}: {
  x: number;
  y: number;
  tc: ToolCall;
  onApprove: (id: string) => void;
  onShowDiff: (tc: ToolCall) => void;
}) {
  const [hover, setHover] = useState(false);
  const hasDiff = tc.diff && tc.diff.length > 0;
  return (
    <div style={{ position: "absolute", left: x, top: y, transform: "translate(-50%,-50%)" }}>
      <div
        style={toolNode(tc.status, hasDiff)}
        onMouseEnter={() => setHover(true)}
        onMouseLeave={() => setHover(false)}
        onClick={() => hasDiff && onShowDiff(tc)}
      >
        <span style={toolNodeKind(tc.kind)}>{tc.kind}</span>
        <span style={toolNodeTarget}>{tc.target.split("/").pop()}</span>
        <span style={toolNodeStatus(tc.status)} />
        {hasDiff && <span style={diffBadge}>diff</span>}
      </div>
      {hover && (
        <div style={toolPop}>
          <code style={toolPopCode}>{tc.target}</code>
          {tc.detail && <p style={toolPopDetail}>{tc.detail}</p>}
          {hasDiff && (
            <button type="button" style={diffBtn} onClick={() => onShowDiff(tc)}>
              view diff
            </button>
          )}
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

// Diff popover — shows the actual code changes when a file/tool node is clicked.
function DiffPopover({ tc, onClose }: { tc: ToolCall; onClose: () => void }) {
  const hunks = tc.diff ?? [];
  const additions = hunks.reduce(
    (n, h) => n + h.lines.filter((l) => l.type === "add").length,
    0,
  );
  const deletions = hunks.reduce(
    (n, h) => n + h.lines.filter((l) => l.type === "del").length,
    0,
  );
  return (
    <div style={diffPop}>
      <div style={diffPopHead}>
        <div style={diffPopHeadLeft}>
          <span style={diffPopFile}>{tc.target}</span>
          <span style={diffPopStats}>
            <span style={{ color: "var(--status-success)" }}>+{additions}</span>
            <span style={{ color: "var(--status-error)" }}>−{deletions}</span>
          </span>
        </div>
        <button type="button" style={diffPopClose} onClick={onClose}>
          ×
        </button>
      </div>
      <div style={diffPopBody}>
        {hunks.map((h, hi) => (
          <div key={hi} style={diffHunk}>
            <div style={diffHunkHeader}>
              @@ -{h.oldStart} +{h.newStart} @@
            </div>
            {h.lines.map((l, li) => (
              <div key={li} style={diffLine(l.type)}>
                <span style={diffLineNo}>{l.oldNo ?? ""}</span>
                <span style={diffLineNo}>{l.newNo ?? ""}</span>
                <span style={diffLineSign(l.type)}>
                  {l.type === "add" ? "+" : l.type === "del" ? "−" : " "}
                </span>
                <code style={diffLineText(l.type)}>{l.text}</code>
              </div>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

function ClusterNode({
  x,
  y,
  tools,
  open,
  onToggle,
}: {
  x: number;
  y: number;
  tools: ToolCall[];
  open: boolean;
  onToggle: () => void;
}) {
  return (
    <div style={{ position: "absolute", left: x, top: y, transform: "translate(-50%,-50%)" }}>
      <button type="button" style={clusterNode(open)} onClick={onToggle}>
        <span style={clusterGlyph}>⇄</span>
        <span style={clusterCount}>+{tools.length}</span>
        <span style={clusterLabel}>earlier</span>
      </button>
    </div>
  );
}

function ClusterPopover({ tools, onClose }: { tools: ToolCall[]; onClose: () => void }) {
  // group by kind for a compact summary
  const byKind = useMemo(() => {
    const m = new Map<ToolCall["kind"], ToolCall[]>();
    for (const t of tools) {
      const arr = m.get(t.kind) ?? [];
      arr.push(t);
      m.set(t.kind, arr);
    }
    return Array.from(m.entries());
  }, [tools]);
  return (
    <div style={clusterPop}>
      <div style={clusterPopHead}>
        <span style={clusterPopTitle}>{tools.length} earlier tool calls</span>
        <button type="button" style={clusterPopClose} onClick={onClose}>
          ×
        </button>
      </div>
      <div style={clusterPopBody}>
        {byKind.map(([kind, items]) => (
          <div key={kind} style={clusterGroup}>
            <div style={clusterGroupHead}>
              <span style={clusterGroupKind(kind)}>{kind}</span>
              <span style={clusterGroupCount}>{items.length}</span>
            </div>
            <div style={clusterGroupList}>
              {items.map((tc) => (
                <div key={tc.id} style={clusterGroupItem}>
                  <span style={clusterGroupDot(tc.status)} />
                  <span style={clusterGroupTarget}>{tc.target}</span>
                  <span style={clusterGroupStatus(tc.status)}>{tc.status}</span>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

// Reviewer panel card — the full-detail home for each reviewer in the left
// panel. Live status, progress, streaming comments, verdict. Hovering also
// highlights the reviewer's node on the canvas (via activeReviewer state).
function ReviewerPanelCard({
  r,
  active,
  onHover,
  onLeave,
}: {
  r: Reviewer;
  active: boolean;
  onHover: () => void;
  onLeave: () => void;
}) {
  const color = REVIEWER_COLOR_VAR[r.color];
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [r.comments.length]);
  return (
    <div
      style={panelCard(color, active)}
      onMouseEnter={onHover}
      onMouseLeave={onLeave}
    >
      <div style={panelCardHead}>
        <span style={panelAvatar(color)}>{r.name[0]}</span>
        <div style={panelCardMeta}>
          <span style={panelCardName}>{r.name}</span>
          <span style={panelCardRole}>{r.role}</span>
        </div>
        <span style={panelCardStatus(color, r.status)}>{r.status}</span>
      </div>
      <div style={panelProgressTrack}>
        <div style={panelProgressFill(color, r.progress)} />
      </div>
      {r.nowCommenting && (
        <div style={panelNowReading}>
          <span style={panelPulseDot(color)} />
          <span>reading {r.nowCommenting.split("/").pop()}</span>
        </div>
      )}
      <div style={panelCommentStream} ref={scrollRef}>
        {r.comments.length === 0 && !r.nowCommenting && (
          <div style={panelCommentEmpty}>waiting…</div>
        )}
        {r.comments.map((c, i) => {
          const sev = SEVERITY_META[c.severity];
          return (
            <div key={i} style={panelComment(c.severity)}>
              <div style={panelCommentTop}>
                <span style={panelCommentSev(sev.color)}>{sev.label}</span>
                <span style={panelCommentLine}>
                  {c.file.split("/").pop()}:{c.line}
                </span>
              </div>
              <p style={panelCommentText}>{c.text}</p>
            </div>
          );
        })}
        {r.nowCommenting && <div style={panelTyping(color)}>typing…</div>}
      </div>
      {r.verdict && (
        <div style={panelVerdict(r.verdict)}>
          {r.verdict === "approve" ? "✓ approved" : "→ changes requested"}
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
        <p style={popEmpty}>
          {r.nowCommenting ? `reading ${r.nowCommenting}` : "queued"}
        </p>
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
const topSub: React.CSSProperties = { color: "var(--text-muted)", fontSize: 12, fontFamily: "var(--font-mono)" };
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

const body: React.CSSProperties = { display: "flex", flex: 1, minHeight: 0 };
const splitter: React.CSSProperties = {
  width: 4,
  background: "var(--surface-line)",
  cursor: "col-resize",
  flexShrink: 0,
  position: "relative",
  transition: "background 150ms var(--ease-out-quart)",
};

/* left column — tabbed (Conversation | Reviewers) */
const leftColumn: React.CSSProperties = {
  width: 380,
  borderRight: "1px solid var(--surface-line)",
  background: "var(--surface-sunken)",
  display: "flex",
  flexDirection: "column",
  flexShrink: 0,
};
const tabBar: React.CSSProperties = {
  display: "flex",
  borderBottom: "1px solid var(--surface-line)",
  flexShrink: 0,
};
const tabBtn = (active: boolean): React.CSSProperties => ({
  flex: 1,
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 8,
  padding: "12px 16px",
  background: active ? "var(--surface-elevated)" : "transparent",
  border: "none",
  borderBottom: `2px solid ${active ? "var(--accent-action)" : "transparent"}`,
  color: active ? "var(--text-primary)" : "var(--text-muted)",
  fontSize: 12,
  fontFamily: "var(--font-body)",
  fontWeight: 600,
  cursor: "pointer",
  transition: "color 150ms var(--ease-out-quart), border-color 150ms var(--ease-out-quart)",
});
const tabBadge: React.CSSProperties = {
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
  background: "var(--surface-base)",
  borderRadius: "var(--radius-full)",
  padding: "1px 7px",
};
const tabBadgeLive: React.CSSProperties = {
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  color: "var(--accent-action)",
  background: "var(--accent-action)22",
  borderRadius: "var(--radius-full)",
  padding: "1px 7px",
};

/* conversation tab */
const convTab: React.CSSProperties = {
  flex: 1,
  display: "flex",
  flexDirection: "column",
  minHeight: 0,
};
const convStream: React.CSSProperties = {
  flex: 1,
  overflowY: "auto",
  padding: "16px 16px 8px",
  display: "flex",
  flexDirection: "column",
  gap: 12,
};
const convEmpty: React.CSSProperties = {
  color: "var(--text-muted)",
  fontSize: 12,
  fontFamily: "var(--font-mono)",
  padding: "24px 0",
  textAlign: "center",
};

/* reviewers tab */
const reviewersTab: React.CSSProperties = {
  flex: 1,
  display: "flex",
  flexDirection: "column",
  minHeight: 0,
};
const panelMeta: React.CSSProperties = { color: "var(--text-muted)", fontSize: 10, fontFamily: "var(--font-mono)", marginLeft: "auto" };
const panelSummary: React.CSSProperties = { display: "flex", alignItems: "center", gap: 8, padding: "12px 16px", borderBottom: "1px solid var(--surface-line)", flexShrink: 0 };
const summaryPill = (c: string): React.CSSProperties => ({
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  color: c,
  border: `1px solid ${c}`,
  borderRadius: "var(--radius-full)",
  padding: "2px 8px",
  textTransform: "uppercase",
  letterSpacing: "0.1em",
});
const panelList: React.CSSProperties = {
  flex: 1,
  overflowY: "auto",
  padding: "10px 14px 16px",
  display: "flex",
  flexDirection: "column",
  gap: 10,
};

/* reviewer panel card */
const panelCard = (color: string, active: boolean): React.CSSProperties => ({
  background: "var(--surface-elevated)",
  border: `1px solid ${active ? color : "var(--surface-line)"}`,
  borderLeft: `2px solid ${color}`,
  borderRadius: "var(--radius-md)",
  padding: 12,
  boxShadow: active ? `0 0 12px ${color}33` : "none",
  transition: "border-color 150ms var(--ease-out-quart), box-shadow 150ms var(--ease-out-quart)",
});
const panelCardHead: React.CSSProperties = { display: "flex", alignItems: "center", gap: 10, marginBottom: 8 };
const panelAvatar = (color: string): React.CSSProperties => ({
  width: 26,
  height: 26,
  borderRadius: "50%",
  display: "grid",
  placeItems: "center",
  background: `${color}22`,
  color,
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  fontWeight: 600,
  border: `1px solid ${color}55`,
  flexShrink: 0,
});
const panelCardMeta: React.CSSProperties = { display: "flex", flexDirection: "column", flex: 1, minWidth: 0 };
const panelCardName: React.CSSProperties = { fontSize: 13, fontWeight: 600 };
const panelCardRole: React.CSSProperties = { fontSize: 10, color: "var(--text-muted)", fontFamily: "var(--font-mono)", textTransform: "uppercase", letterSpacing: "0.08em" };
const panelCardStatus = (color: string, s: Reviewer["status"]): React.CSSProperties => ({
  fontSize: 9,
  textTransform: "uppercase",
  letterSpacing: "0.1em",
  fontFamily: "var(--font-mono)",
  color: s === "done" ? "var(--text-muted)" : color,
  flexShrink: 0,
});
const panelProgressTrack: React.CSSProperties = { height: 3, background: "var(--surface-line)", borderRadius: 2, overflow: "hidden", marginBottom: 8 };
const panelProgressFill = (color: string, p: number): React.CSSProperties => ({
  height: "100%",
  width: `${p * 100}%`,
  background: color,
  transition: "width 600ms var(--ease-out-quart)",
});
const panelNowReading: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 6,
  fontSize: 10,
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
  marginBottom: 6,
};
const panelPulseDot = (color: string): React.CSSProperties => ({
  width: 5,
  height: 5,
  borderRadius: "50%",
  background: color,
  animation: "proto-pulse 1s infinite",
  flexShrink: 0,
});
const panelCommentStream: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 6,
  maxHeight: 140,
  overflowY: "auto",
};
const panelCommentEmpty: React.CSSProperties = { color: "var(--text-muted)", fontSize: 10, fontFamily: "var(--font-mono)" };
const panelComment = (sev: Reviewer["comments"][number]["severity"]): React.CSSProperties => ({
  background: "var(--surface-sunken)",
  borderRadius: "var(--radius-sm)",
  padding: "6px 8px",
  borderLeft: `2px solid ${sev === "blocker" ? "var(--status-error)" : sev === "issue" ? "var(--status-warning)" : sev === "praise" ? "var(--status-success)" : "var(--surface-line)"}`,
});
const panelCommentTop: React.CSSProperties = { display: "flex", alignItems: "center", gap: 6, marginBottom: 3 };
const panelCommentSev = (c: string): React.CSSProperties => ({
  fontSize: 8,
  textTransform: "uppercase",
  letterSpacing: "0.1em",
  fontFamily: "var(--font-mono)",
  color: c,
  fontWeight: 600,
});
const panelCommentLine: React.CSSProperties = { fontSize: 9, fontFamily: "var(--font-mono)", color: "var(--text-muted)" };
const panelCommentText: React.CSSProperties = { margin: 0, fontSize: 11, lineHeight: 1.45, color: "var(--text-secondary)" };
const panelTyping = (color: string): React.CSSProperties => ({ fontSize: 9, fontFamily: "var(--font-mono)", color, animation: "proto-blink 1.2s infinite" });
const panelVerdict = (v: Reviewer["verdict"]): React.CSSProperties => ({
  marginTop: 8,
  padding: "5px 10px",
  borderRadius: "var(--radius-sm)",
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  textAlign: "center",
  background: v === "approve" ? "var(--status-success)22" : "var(--status-warning)22",
  color: v === "approve" ? "var(--status-success)" : "var(--status-warning)",
});

/* canvas (C) */
const canvasWrap: React.CSSProperties = { flex: 1, position: "relative", minWidth: 0 };
const canvas: React.CSSProperties = { position: "absolute", inset: 0, overflow: "hidden", background: "var(--surface-base)" };
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
const agentLabel: React.CSSProperties = { fontSize: 9, textTransform: "uppercase", letterSpacing: "0.14em", fontFamily: "var(--font-mono)", color: "var(--text-muted)" };
const agentName: React.CSSProperties = { fontSize: 14, fontWeight: 600, marginTop: 2 };

const toolNode = (s: ToolCall["status"], hasDiff?: boolean): React.CSSProperties => ({
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: 2,
  padding: "8px 12px",
  borderRadius: "var(--radius-md)",
  background: "var(--surface-elevated)",
  border: `1px solid ${s === "awaiting" ? "var(--status-warning)" : hasDiff ? "var(--accent-signal)55" : "var(--surface-line)"}`,
  minWidth: 110,
  cursor: hasDiff ? "pointer" : "default",
  boxShadow: s === "running" ? "0 0 16px rgba(120,200,180,0.3)" : "none",
});
const diffBadge: React.CSSProperties = {
  fontSize: 8,
  textTransform: "uppercase",
  letterSpacing: "0.1em",
  fontFamily: "var(--font-mono)",
  color: "var(--accent-signal)",
  background: "var(--accent-signal)22",
  borderRadius: "var(--radius-full)",
  padding: "1px 5px",
  marginTop: 2,
};
const diffBtn: React.CSSProperties = {
  display: "block",
  width: "100%",
  marginTop: 8,
  background: "var(--accent-signal)22",
  color: "var(--accent-signal)",
  border: `1px solid var(--accent-signal)55`,
  borderRadius: "var(--radius-sm)",
  padding: "4px 10px",
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  cursor: "pointer",
  textTransform: "uppercase",
  letterSpacing: "0.08em",
};

/* diff popover */
const diffPop: React.CSSProperties = {
  position: "absolute",
  top: 60,
  left: "50%",
  transform: "translateX(-50%)",
  width: 560,
  maxHeight: 480,
  background: "var(--surface-overlay)",
  border: "1px solid var(--surface-line)",
  borderRadius: "var(--radius-md)",
  boxShadow: "var(--shadow-floating)",
  zIndex: 20,
  display: "flex",
  flexDirection: "column",
};
const diffPopHead: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
  padding: "12px 16px",
  borderBottom: "1px solid var(--surface-line)",
  flexShrink: 0,
};
const diffPopHeadLeft: React.CSSProperties = { display: "flex", alignItems: "center", gap: 12 };
const diffPopFile: React.CSSProperties = { fontSize: 13, fontFamily: "var(--font-mono)", color: "var(--text-primary)" };
const diffPopStats: React.CSSProperties = { display: "flex", gap: 8, fontSize: 11, fontFamily: "var(--font-mono)" };
const diffPopClose: React.CSSProperties = { background: "transparent", border: "none", color: "var(--text-muted)", fontSize: 18, cursor: "pointer" };
const diffPopBody: React.CSSProperties = { flex: 1, overflowY: "auto", padding: 0, fontFamily: "var(--font-mono)", fontSize: 12 };
const diffHunk: React.CSSProperties = { marginBottom: 4 };
const diffHunkHeader: React.CSSProperties = { padding: "6px 16px", color: "var(--text-muted)", fontSize: 11, background: "var(--surface-sunken)", borderBottom: "1px solid var(--surface-line)" };
const diffLine = (type: DiffLine["type"]): React.CSSProperties => ({
  display: "flex",
  padding: "0 16px",
  background: type === "add" ? "var(--status-success)0c" : type === "del" ? "var(--status-error)0c" : "transparent",
  lineHeight: 1.6,
});
const diffLineNo: React.CSSProperties = { width: 36, textAlign: "right", color: "var(--text-muted)", fontSize: 10, flexShrink: 0, userSelect: "none" };
const diffLineSign = (type: DiffLine["type"]): React.CSSProperties => ({ width: 16, textAlign: "center", flexShrink: 0, color: type === "add" ? "var(--status-success)" : type === "del" ? "var(--status-error)" : "var(--text-muted)" });
const diffLineText = (type: DiffLine["type"]): React.CSSProperties => ({ whiteSpace: "pre", color: type === "add" ? "var(--status-success)" : type === "del" ? "var(--status-error)" : "var(--text-secondary)" });
const toolNodeKind = (k: ToolCall["kind"]): React.CSSProperties => ({
  fontSize: 9,
  textTransform: "uppercase",
  letterSpacing: "0.12em",
  fontFamily: "var(--font-mono)",
  color: k === "edit" || k === "write" ? "var(--accent-signal)" : k === "run_shell" ? "var(--accent-action)" : "var(--text-muted)",
});
const toolNodeTarget: React.CSSProperties = { fontSize: 11, fontFamily: "var(--font-mono)", color: "var(--text-primary)", maxWidth: 120, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" };
const toolNodeStatus = (s: ToolCall["status"]): React.CSSProperties => ({
  width: 6,
  height: 6,
  borderRadius: "50%",
  marginTop: 2,
  background: s === "done" ? "var(--status-success)" : s === "awaiting" ? "var(--status-warning)" : s === "running" ? "var(--accent-action)" : "var(--text-muted)",
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
const toolPopCode: React.CSSProperties = { display: "block", fontFamily: "var(--font-mono)", fontSize: 10, color: "var(--text-secondary)", wordBreak: "break-all" };
const toolPopDetail: React.CSSProperties = { margin: "6px 0 0", fontSize: 11, color: "var(--text-muted)", lineHeight: 1.4 };
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

/* cluster node */
const clusterNode = (open: boolean): React.CSSProperties => ({
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: 2,
  padding: "10px 14px",
  borderRadius: "var(--radius-md)",
  background: "var(--surface-sunken)",
  border: `1px dashed ${open ? "var(--accent-action)" : "var(--text-muted)"}`,
  color: "var(--text-secondary)",
  cursor: "pointer",
  fontFamily: "var(--font-mono)",
  minWidth: 96,
});
const clusterGlyph: React.CSSProperties = { fontSize: 14, color: "var(--text-muted)" };
const clusterCount: React.CSSProperties = { fontSize: 16, fontWeight: 600, color: "var(--accent-action)" };
const clusterLabel: React.CSSProperties = { fontSize: 9, textTransform: "uppercase", letterSpacing: "0.12em", color: "var(--text-muted)" };

const clusterPop: React.CSSProperties = {
  position: "absolute",
  top: 60,
  left: 20,
  width: 320,
  maxHeight: 460,
  background: "var(--surface-overlay)",
  border: "1px solid var(--surface-line)",
  borderRadius: "var(--radius-md)",
  padding: 14,
  boxShadow: "var(--shadow-floating)",
  zIndex: 10,
  display: "flex",
  flexDirection: "column",
};
const clusterPopHead: React.CSSProperties = { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 10 };
const clusterPopTitle: React.CSSProperties = { fontSize: 12, fontFamily: "var(--font-mono)", textTransform: "uppercase", letterSpacing: "0.1em", color: "var(--text-secondary)" };
const clusterPopClose: React.CSSProperties = { background: "transparent", border: "none", color: "var(--text-muted)", fontSize: 16, cursor: "pointer" };
const clusterPopBody: React.CSSProperties = { flex: 1, overflowY: "auto", display: "flex", flexDirection: "column", gap: 12 };
const clusterGroup: React.CSSProperties = { display: "flex", flexDirection: "column", gap: 4 };
const clusterGroupHead: React.CSSProperties = { display: "flex", alignItems: "center", gap: 8 };
const clusterGroupKind = (k: ToolCall["kind"]): React.CSSProperties => ({
  fontSize: 10,
  textTransform: "uppercase",
  letterSpacing: "0.1em",
  fontFamily: "var(--font-mono)",
  color: k === "edit" || k === "write" ? "var(--accent-signal)" : k === "run_shell" ? "var(--accent-action)" : "var(--text-muted)",
  fontWeight: 600,
});
const clusterGroupCount: React.CSSProperties = { fontSize: 10, fontFamily: "var(--font-mono)", color: "var(--text-muted)" };
const clusterGroupList: React.CSSProperties = { display: "flex", flexDirection: "column", gap: 2, marginLeft: 14 };
const clusterGroupItem: React.CSSProperties = { display: "flex", alignItems: "center", gap: 8, fontFamily: "var(--font-mono)", fontSize: 10 };
const clusterGroupDot = (s: ToolCall["status"]): React.CSSProperties => ({
  width: 5,
  height: 5,
  borderRadius: "50%",
  background: s === "done" ? "var(--status-success)" : s === "awaiting" ? "var(--status-warning)" : "var(--text-muted)",
});
const clusterGroupTarget: React.CSSProperties = { flex: 1, color: "var(--text-secondary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" };
const clusterGroupStatus = (_s: ToolCall["status"]): React.CSSProperties => ({ fontSize: 9, color: "var(--text-muted)", textTransform: "uppercase" });

/* reviewer node + popover (from C) */
const reviewerNode = (color: string, active: boolean, status: Reviewer["status"]): React.CSSProperties => ({
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
  animation: status !== "done" && status !== "queued" ? "proto-pulse-ring 1.6s ease-out infinite" : "none",
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
const reviewerNodeRole: React.CSSProperties = { fontSize: 9, color: "var(--text-muted)", fontFamily: "var(--font-mono)", textTransform: "uppercase", letterSpacing: "0.08em" };
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
const popHead: React.CSSProperties = { display: "flex", alignItems: "center", gap: 10, marginBottom: 10 };
const popName: React.CSSProperties = { fontSize: 13, fontWeight: 600 };
const popRole: React.CSSProperties = { fontSize: 10, color: "var(--text-muted)", fontFamily: "var(--font-mono)" };
const popProgress: React.CSSProperties = { height: 3, background: "var(--surface-line)", borderRadius: 2, overflow: "hidden", marginBottom: 10 };
const popProgressFill = (color: string, p: number): React.CSSProperties => ({ height: "100%", width: `${p * 100}%`, background: color, transition: "width 600ms var(--ease-out-quart)" });
const popEmpty: React.CSSProperties = { fontSize: 11, color: "var(--text-muted)", fontFamily: "var(--font-mono)", margin: 0 };
const popComments: React.CSSProperties = { display: "flex", flexDirection: "column", gap: 8, maxHeight: 240, overflowY: "auto" };
const popComment: React.CSSProperties = { background: "var(--surface-sunken)", borderRadius: "var(--radius-sm)", padding: 8 };
const popSev = (c: string): React.CSSProperties => ({ fontSize: 9, textTransform: "uppercase", letterSpacing: "0.1em", fontFamily: "var(--font-mono)", color: c, fontWeight: 600 });
const popLine: React.CSSProperties = { fontSize: 10, fontFamily: "var(--font-mono)", color: "var(--text-muted)", marginLeft: 6 };
const popText: React.CSSProperties = { margin: "4px 0 0", fontSize: 11, lineHeight: 1.45, color: "var(--text-secondary)" };

/* equalizer + load hint */
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
  background: ["var(--agent-cyan)", "var(--agent-violet)", "var(--agent-amber)", "var(--agent-rose)"][i % 4],
  borderRadius: 2,
  height: running ? `${30 + Math.abs(Math.sin(i * 0.7)) * 70}%` : "20%",
  animation: running ? `proto-eq ${0.6 + (i % 5) * 0.1}s ease-in-out infinite alternate` : "none",
  animationDelay: `${i * 0.05}s`,
});
const loadHint: React.CSSProperties = {
  position: "absolute",
  bottom: 14,
  left: 20,
  display: "flex",
  alignItems: "center",
  gap: 10,
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
  textTransform: "uppercase",
  letterSpacing: "0.08em",
  background: "var(--surface-overlay)",
  border: "1px solid var(--surface-line)",
  borderRadius: "var(--radius-full)",
  padding: "4px 10px",
};
const loadHintBtn: React.CSSProperties = {
  background: "transparent",
  border: "none",
  color: "var(--accent-action)",
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  cursor: "pointer",
  textTransform: "uppercase",
  letterSpacing: "0.08em",
  textDecoration: "underline",
};

/* approval gate bar (inside conversation tab) */
const approvalBar: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 10,
  padding: "10px 16px",
  background: "var(--status-warning)14",
  borderBottom: "1px solid var(--surface-line)",
  flexShrink: 0,
};
const approvalDot: React.CSSProperties = {
  width: 8,
  height: 8,
  borderRadius: "50%",
  background: "var(--status-warning)",
  boxShadow: "0 0 8px var(--status-warning)",
  flexShrink: 0,
};
const approvalText: React.CSSProperties = { fontSize: 12, color: "var(--text-primary)", flexShrink: 0 };
const approvalCode: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  color: "var(--status-warning)",
  background: "var(--surface-base)",
  padding: "1px 6px",
  borderRadius: "var(--radius-sm)",
};

/* transcript turn */
const tTurn = (isUser: boolean): React.CSSProperties => ({
  display: "flex",
  flexDirection: "column",
  gap: 6,
  padding: "8px 12px",
  borderRadius: "var(--radius-md)",
  background: isUser ? "var(--accent-action)0f" : "var(--surface-elevated)",
  border: `1px solid ${isUser ? "var(--accent-action)33" : "var(--surface-line)"}`,
  alignSelf: isUser ? "flex-end" : "flex-start",
  maxWidth: "88%",
});
const tHead: React.CSSProperties = { display: "flex", alignItems: "center", gap: 8 };
const tAvatar = (isUser: boolean): React.CSSProperties => ({
  width: 20,
  height: 20,
  borderRadius: "50%",
  display: "grid",
  placeItems: "center",
  fontSize: 10,
  fontWeight: 600,
  fontFamily: "var(--font-mono)",
  background: isUser ? "var(--accent-action)22" : "var(--accent-structure)22",
  color: isUser ? "var(--accent-action)" : "var(--accent-structure)",
  border: `1px solid ${isUser ? "var(--accent-action)55" : "var(--accent-structure)55"}`,
});
const tRole = (isUser: boolean): React.CSSProperties => ({
  fontSize: 10,
  textTransform: "uppercase",
  letterSpacing: "0.12em",
  fontFamily: "var(--font-mono)",
  color: isUser ? "var(--accent-action)" : "var(--accent-structure)",
});
const tReasoning: React.CSSProperties = {
  borderLeft: "2px solid var(--surface-line)",
  padding: "6px 10px",
  background: "var(--surface-sunken)",
  borderRadius: "0 var(--radius-sm) var(--radius-sm) 0",
};
const tReasoningLabel: React.CSSProperties = {
  fontSize: 9,
  textTransform: "uppercase",
  letterSpacing: "0.14em",
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
};
const tReasoningText: React.CSSProperties = {
  margin: "3px 0 0",
  fontSize: 12,
  color: "var(--text-secondary)",
  lineHeight: 1.5,
  fontStyle: "italic",
};
const tText = (isUser: boolean): React.CSSProperties => ({
  margin: 0,
  fontSize: 13,
  lineHeight: 1.55,
  color: isUser ? "var(--text-primary)" : "var(--text-secondary)",
});
const tTools: React.CSSProperties = { display: "flex", flexWrap: "wrap", gap: 6, marginTop: 2 };
const tToolChip: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 5,
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
  background: "var(--surface-sunken)",
  border: "1px solid var(--surface-line)",
  borderRadius: "var(--radius-full)",
  padding: "2px 8px",
};
const tToolDot = (s: ToolCall["status"]): React.CSSProperties => ({
  width: 5,
  height: 5,
  borderRadius: "50%",
  background:
    s === "done"
      ? "var(--status-success)"
      : s === "awaiting"
        ? "var(--status-warning)"
        : s === "running"
          ? "var(--accent-action)"
          : "var(--text-muted)",
});

/* live reasoning indicator */
const liveReasoning: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "6px 12px",
  alignSelf: "flex-start",
};
const liveReasoningDot: React.CSSProperties = {
  width: 6,
  height: 6,
  borderRadius: "50%",
  background: "var(--accent-action)",
  animation: "proto-pulse 1s infinite",
};
const liveReasoningText: React.CSSProperties = {
  fontSize: 11,
  fontFamily: "var(--font-mono)",
  color: "var(--accent-action)",
  fontStyle: "italic",
};

/* prompt input */
const inputWrap: React.CSSProperties = { padding: "12px 16px 16px", flexShrink: 0 };

/* composer controls row — model, variant, mode toggle */
const controlsRow: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 8,
  marginBottom: 8,
};
const selectorGroup: React.CSSProperties = { display: "flex", gap: 6 };
const selectorWrap: React.CSSProperties = { position: "relative" };
const selectorBtn = (open: boolean): React.CSSProperties => ({
  display: "inline-flex",
  alignItems: "center",
  gap: 4,
  background: open ? "var(--surface-elevated)" : "var(--surface-base)",
  border: "1px solid var(--surface-line)",
  color: "var(--text-secondary)",
  borderRadius: "var(--radius-sm)",
  padding: "4px 10px",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
  cursor: "pointer",
  whiteSpace: "nowrap",
});
const selectorCaret: React.CSSProperties = { fontSize: 9, color: "var(--text-muted)" };
const selectorMenu: React.CSSProperties = {
  position: "absolute",
  bottom: "100%",
  left: 0,
  marginBottom: 4,
  background: "var(--surface-overlay)",
  border: "1px solid var(--surface-line)",
  borderRadius: "var(--radius-sm)",
  boxShadow: "var(--shadow-floating)",
  zIndex: 30,
  minWidth: 120,
  padding: 4,
  display: "flex",
  flexDirection: "column",
  gap: 1,
};
const selectorItem = (active: boolean): React.CSSProperties => ({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  background: active ? "var(--accent-action)14" : "transparent",
  border: "none",
  color: active ? "var(--accent-action)" : "var(--text-secondary)",
  borderRadius: "var(--radius-sm)",
  padding: "5px 10px",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
  cursor: "pointer",
  textAlign: "left",
});
const selectorCheck: React.CSSProperties = { color: "var(--accent-action)", fontSize: 10 };

/* mode toggle — build / plan */
const modeToggle: React.CSSProperties = {
  display: "inline-flex",
  background: "var(--surface-base)",
  border: "1px solid var(--surface-line)",
  borderRadius: "var(--radius-sm)",
  padding: 2,
  gap: 2,
};
const modeBtn = (active: boolean): React.CSSProperties => ({
  background: active ? "var(--accent-action)" : "transparent",
  color: active ? "var(--text-inverse)" : "var(--text-muted)",
  border: "none",
  borderRadius: "var(--radius-xs)",
  padding: "3px 12px",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
  fontWeight: 600,
  cursor: "pointer",
  textTransform: "uppercase",
  letterSpacing: "0.08em",
  transition: "background 150ms var(--ease-out-quart), color 150ms var(--ease-out-quart)",
});
const inputInner: React.CSSProperties = {
  display: "flex",
  alignItems: "flex-end",
  gap: 10,
  background: "var(--surface-elevated)",
  border: "1px solid var(--surface-line)",
  borderRadius: "var(--radius-md)",
  padding: "10px 12px",
  transition: "border-color 150ms var(--ease-out-quart)",
};
const inputField: React.CSSProperties = {
  flex: 1,
  background: "transparent",
  border: "none",
  outline: "none",
  resize: "none",
  color: "var(--text-primary)",
  fontFamily: "var(--font-body)",
  fontSize: 14,
  lineHeight: 1.5,
};
const sendBtn = (can: boolean): React.CSSProperties => ({
  display: "inline-flex",
  alignItems: "center",
  gap: 6,
  background: can ? "var(--accent-action)" : "var(--surface-line)",
  color: can ? "var(--text-inverse)" : "var(--text-muted)",
  border: "none",
  borderRadius: "var(--radius-md)",
  padding: "8px 16px",
  fontSize: 13,
  fontFamily: "var(--font-body)",
  fontWeight: 600,
  cursor: can ? "pointer" : "not-allowed",
  flexShrink: 0,
});
const sendHint: React.CSSProperties = {
  fontSize: 11,
  fontFamily: "var(--font-mono)",
  opacity: 0.7,
};
