// PROTOTYPE — throwaway. Variant B: "Review Theater"
// References: Cursor PR review workspace (3-column charcoal, green accents,
// activity sidebar + center cards + right diff panel), Parallel deep research
// (progress + activity log accordion).
//
// Shape: three columns. Left = session/activity sidebar (prompt history + tool
// timeline). Center = active agent response + composer. Right = the unique
// twist — "Reviewers" rendered as PARALLEL HORIZONTAL LANES: each subagent is
// a vertical strip, and the strips sit side-by-side so you scan horizontally
// to compare what every reviewer is saying about the same diff at once, like a
// multi-track timeline. Calm charcoal, green-tinted positive accents.
import { useEffect, useRef, useState } from "react";
import {
  type AgentTurn,
  REVIEWER_COLOR_VAR,
  type Reviewer,
  SEVERITY_META,
  type ToolCall,
} from "./data";
import { allTools, usePlayback } from "./usePlayback";

export function VariantB() {
  const { turns, reviewers, running, reviewStarted, restart, approve } = usePlayback();
  const [activeTurnId, setActiveTurnId] = useState<string | null>(null);
  const centerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (turns.length && !activeTurnId) setActiveTurnId(turns[turns.length - 1].id);
  }, [turns, activeTurnId]);

  useEffect(() => {
    centerRef.current?.scrollTo({ top: centerRef.current.scrollHeight, behavior: "smooth" });
  }, [turns]);

  const activeTurn = turns.find((t) => t.id === activeTurnId) ?? turns[turns.length - 1];
  const tools = allTools(turns);
  const verdicts = reviewers.filter((r) => r.verdict);

  return (
    <div style={shell}>
      <header style={topbar}>
        <span style={topTitle}>Review Theater</span>
        <span style={topCrumb}>gospel · session #4821 · retry-with-backoff</span>
        <div style={topRight}>
          <span style={statusChip(running ? "running" : reviewStarted ? "reviewing" : "idle")}>
            {running ? "agent running" : reviewStarted ? "review in progress" : "idle"}
          </span>
          <button type="button" style={ghostBtn} onClick={restart}>
            replay
          </button>
        </div>
      </header>

      <div style={grid}>
        {/* Left: activity / tool timeline */}
        <aside style={leftCol}>
          <div style={colHead}>
            <span>activity</span>
            <span style={colCount}>{tools.length} calls</span>
          </div>
          <div style={promptHistory}>
            {turns
              .filter((t) => t.role === "user")
              .map((t) => (
                <div key={t.id} style={promptHistoryItem}>
                  <span style={promptHistoryGlyph}>❯</span>
                  <span style={promptHistoryText}>{t.text}</span>
                </div>
              ))}
          </div>
          <div style={timeline}>
            {tools.map((tc) => (
              <button
                key={tc.id}
                type="button"
                style={timelineItem(tc.status)}
                onClick={() => {
                  const owner = turns.find((t) => t.tools?.some((x) => x.id === tc.id));
                  if (owner) setActiveTurnId(owner.id);
                }}
              >
                <span style={timelineDot(tc)} />
                <span style={timelineLabel}>{tc.kind}</span>
                <span style={timelineTarget}>{tc.target.replace(/^.*\//, "")}</span>
              </button>
            ))}
            {tools.length === 0 && <div style={emptyHint}>no tool calls yet</div>}
          </div>
        </aside>

        {/* Center: active turn + composer */}
        <main style={centerCol} ref={centerRef}>
          <div style={centerInner}>
            <div style={centerHead}>
              <span style={centerHeadLabel}>conversation</span>
              <span style={centerHeadMeta}>{turns.length} turns</span>
            </div>
            {turns.map((t) => (
              <CenterTurn
                key={t.id}
                turn={t}
                active={t.id === activeTurn?.id}
                onFocus={() => setActiveTurnId(t.id)}
                onApprove={approve}
              />
            ))}
            {running && <div style={liveBar}>agent working…</div>}
          </div>
          <Composer disabled={running} />
        </main>

        {/* Right: parallel reviewer lanes — the unique twist */}
        <aside style={rightCol}>
          <div style={colHead}>
            <span>reviewers</span>
            <span style={colCount}>
              {reviewStarted ? `${verdicts.length}/${reviewers.length}` : "queued"}
            </span>
          </div>
          <div style={lanesWrap}>
            <div style={lanes}>
              {reviewers.map((r) => (
                <ReviewerLane key={r.id} r={r} />
              ))}
            </div>
            <div style={laneAxis}>
              <span>scan →</span>
              <span>compare reviewers across the same diff</span>
            </div>
          </div>
        </aside>
      </div>
    </div>
  );
}

function CenterTurn({
  turn,
  active,
  onFocus,
  onApprove,
}: {
  turn: AgentTurn;
  active: boolean;
  onFocus: () => void;
  onApprove: (id: string) => void;
}) {
  const isUser = turn.role === "user";
  return (
    <div style={centerTurn(active)} onClick={onFocus}>
      <div style={centerTurnHead(isUser)}>
        <span style={centerTurnRole(isUser)}>{isUser ? "prompt" : "agent"}</span>
      </div>
      {turn.reasoning && !isUser && <p style={centerReasoning}>{turn.reasoning}</p>}
      <p style={centerText(isUser)}>{turn.text}</p>
      {turn.tools && (
        <div style={centerTools}>
          {turn.tools.map((tc) => (
            <CenterTool key={tc.id} tc={tc} onApprove={onApprove} />
          ))}
        </div>
      )}
    </div>
  );
}

function CenterTool({ tc, onApprove }: { tc: ToolCall; onApprove: (id: string) => void }) {
  return (
    <div style={centerTool(tc.status)}>
      <div style={centerToolTop}>
        <span style={centerToolKind(tc.kind)}>{tc.kind}</span>
        <span style={centerToolTarget}>{tc.target}</span>
        <span style={centerToolStatus(tc.status)}>{tc.status}</span>
      </div>
      {tc.detail && <code style={centerToolDetail}>{tc.detail}</code>}
      {tc.needsApproval && tc.status === "awaiting" && (
        <button type="button" style={approveBtn} onClick={() => onApprove(tc.id)}>
          approve
        </button>
      )}
    </div>
  );
}

function ReviewerLane({ r }: { r: Reviewer }) {
  const color = REVIEWER_COLOR_VAR[r.color];
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [r.comments.length]);
  return (
    <div style={lane(color)}>
      <div style={laneHead(color)}>
        <span style={laneAvatar(color)}>{r.name[0]}</span>
        <span style={laneName}>{r.name}</span>
        <span style={laneRole}>{r.role}</span>
      </div>
      <div style={laneProgress}>
        <div style={laneProgressFill(color, r.progress)} />
      </div>
      <div style={laneStatusLine}>
        <span style={laneStatusText(color, r.status)}>{r.status}</span>
        {r.nowCommenting && <span style={laneNow}>{r.nowCommenting.split("/").pop()}</span>}
      </div>
      <div style={laneStream} ref={scrollRef}>
        {r.comments.length === 0 && !r.nowCommenting && <div style={laneEmpty}>waiting…</div>}
        {r.comments.map((c, i) => {
          const sev = SEVERITY_META[c.severity];
          return (
            <div key={i} style={laneComment(c.severity)}>
              <span style={laneCommentSev(sev.color)}>{sev.label}</span>
              <span style={laneCommentLine}>L{c.line}</span>
              <p style={laneCommentText}>{c.text}</p>
            </div>
          );
        })}
        {r.nowCommenting && <div style={laneTyping(color)}>typing…</div>}
      </div>
      {r.verdict && (
        <div style={laneVerdict(r.verdict)}>{r.verdict === "approve" ? "approve" : "changes"}</div>
      )}
    </div>
  );
}

function Composer({ disabled }: { disabled: boolean }) {
  const [v, setV] = useState("");
  return (
    <div style={composer}>
      <textarea
        style={composerInput}
        placeholder={disabled ? "agent working…" : "message the agent…"}
        value={v}
        disabled={disabled}
        onChange={(e) => setV(e.target.value)}
        rows={2}
      />
      <div style={composerFoot}>
        <span style={composerHint}>⏎ send · ⇧⏎ newline · / skills</span>
        <button type="button" style={sendBtn} disabled={disabled || !v.trim()}>
          send
        </button>
      </div>
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
  alignItems: "center",
  gap: 16,
  padding: "12px 20px",
  borderBottom: "1px solid var(--surface-line)",
  background: "var(--surface-sunken)",
  flexShrink: 0,
};
const topTitle: React.CSSProperties = { fontSize: 15, fontWeight: 600, letterSpacing: "-0.01em" };
const topCrumb: React.CSSProperties = {
  color: "var(--text-muted)",
  fontSize: 12,
  fontFamily: "var(--font-mono)",
  flex: 1,
};
const topRight: React.CSSProperties = { display: "flex", alignItems: "center", gap: 10 };
const ghostBtn: React.CSSProperties = {
  background: "transparent",
  border: "1px solid var(--surface-line)",
  color: "var(--text-secondary)",
  borderRadius: "var(--radius-sm)",
  padding: "5px 12px",
  fontSize: 12,
  cursor: "pointer",
  fontFamily: "var(--font-mono)",
};
const statusChip = (s: "running" | "reviewing" | "idle"): React.CSSProperties => ({
  fontSize: 10,
  textTransform: "uppercase",
  letterSpacing: "0.1em",
  fontFamily: "var(--font-mono)",
  padding: "4px 10px",
  borderRadius: "var(--radius-full)",
  border: "1px solid var(--surface-line)",
  color:
    s === "running"
      ? "var(--status-success)"
      : s === "reviewing"
        ? "var(--status-warning)"
        : "var(--text-muted)",
});
const grid: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "240px 1fr 1fr",
  flex: 1,
  minHeight: 0,
};
const leftCol: React.CSSProperties = {
  borderRight: "1px solid var(--surface-line)",
  display: "flex",
  flexDirection: "column",
  background: "var(--surface-sunken)",
};
const centerCol: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  borderRight: "1px solid var(--surface-line)",
  minWidth: 0,
};
const rightCol: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  minWidth: 0,
  background: "var(--surface-sunken)",
};
const colHead: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "baseline",
  padding: "14px 16px 10px",
  fontSize: 11,
  textTransform: "uppercase",
  letterSpacing: "0.12em",
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
  borderBottom: "1px solid var(--surface-line)",
};
const colCount: React.CSSProperties = { color: "var(--text-muted)" };
const promptHistory: React.CSSProperties = {
  padding: "10px 14px",
  borderBottom: "1px solid var(--surface-line)",
};
const promptHistoryItem: React.CSSProperties = {
  display: "flex",
  gap: 8,
  alignItems: "flex-start",
};
const promptHistoryGlyph: React.CSSProperties = {
  color: "var(--status-success)",
  fontFamily: "var(--font-mono)",
  fontSize: 12,
};
const promptHistoryText: React.CSSProperties = {
  fontSize: 12,
  color: "var(--text-secondary)",
  lineHeight: 1.45,
};
const timeline: React.CSSProperties = {
  flex: 1,
  overflowY: "auto",
  padding: "10px 14px",
  display: "flex",
  flexDirection: "column",
  gap: 2,
};
const timelineItem = (_s: ToolCall["status"]): React.CSSProperties => ({
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "6px 8px",
  borderRadius: "var(--radius-sm)",
  background: "transparent",
  border: "none",
  cursor: "pointer",
  textAlign: "left",
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  color: "var(--text-secondary)",
});
const timelineDot = (tc: ToolCall): React.CSSProperties => ({
  width: 7,
  height: 7,
  borderRadius: "50%",
  flexShrink: 0,
  background:
    tc.status === "done"
      ? "var(--status-success)"
      : tc.status === "awaiting"
        ? "var(--status-warning)"
        : tc.status === "running"
          ? "var(--accent-action)"
          : "var(--text-muted)",
});
const timelineLabel: React.CSSProperties = { color: "var(--text-muted)", minWidth: 52 };
const timelineTarget: React.CSSProperties = {
  color: "var(--text-primary)",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};
const emptyHint: React.CSSProperties = {
  color: "var(--text-muted)",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
  padding: "8px",
};

const centerInner: React.CSSProperties = { flex: 1, overflowY: "auto", padding: "16px 24px" };
const centerHead: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "baseline",
  marginBottom: 16,
};
const centerHeadLabel: React.CSSProperties = {
  fontSize: 11,
  textTransform: "uppercase",
  letterSpacing: "0.12em",
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
};
const centerHeadMeta: React.CSSProperties = {
  color: "var(--text-muted)",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
};
const centerTurn = (active: boolean): React.CSSProperties => ({
  marginBottom: 18,
  padding: 14,
  borderRadius: "var(--radius-md)",
  border: `1px solid ${active ? "var(--status-success)55" : "var(--surface-line)"}`,
  background: active ? "var(--status-success)0a" : "var(--surface-elevated)",
  cursor: "pointer",
});
const centerTurnHead = (_isUser: boolean): React.CSSProperties => ({ marginBottom: 8 });
const centerTurnRole = (isUser: boolean): React.CSSProperties => ({
  fontSize: 10,
  textTransform: "uppercase",
  letterSpacing: "0.12em",
  fontFamily: "var(--font-mono)",
  color: isUser ? "var(--status-success)" : "var(--accent-structure)",
});
const centerReasoning: React.CSSProperties = {
  margin: "0 0 8px",
  fontSize: 12,
  color: "var(--text-muted)",
  fontStyle: "italic",
  lineHeight: 1.5,
};
const centerText = (isUser: boolean): React.CSSProperties => ({
  margin: 0,
  fontSize: 14,
  lineHeight: 1.6,
  color: isUser ? "var(--text-primary)" : "var(--text-secondary)",
});
const centerTools: React.CSSProperties = {
  marginTop: 12,
  display: "flex",
  flexDirection: "column",
  gap: 6,
};
const centerTool = (_s: ToolCall["status"]): React.CSSProperties => ({
  background: "var(--surface-sunken)",
  border: "1px solid var(--surface-line)",
  borderRadius: "var(--radius-sm)",
  padding: "8px 10px",
});
const centerToolTop: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 10,
  fontFamily: "var(--font-mono)",
  fontSize: 11,
};
const centerToolKind = (k: ToolCall["kind"]): React.CSSProperties => ({
  color:
    k === "edit" || k === "write"
      ? "var(--accent-signal)"
      : k === "run_shell"
        ? "var(--status-success)"
        : "var(--text-muted)",
  fontWeight: 600,
  minWidth: 64,
});
const centerToolTarget: React.CSSProperties = {
  flex: 1,
  color: "var(--text-primary)",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};
const centerToolStatus = (s: ToolCall["status"]): React.CSSProperties => ({
  fontSize: 9,
  textTransform: "uppercase",
  letterSpacing: "0.1em",
  color:
    s === "done"
      ? "var(--status-success)"
      : s === "awaiting"
        ? "var(--status-warning)"
        : s === "running"
          ? "var(--accent-action)"
          : "var(--text-muted)",
});
const centerToolDetail: React.CSSProperties = {
  display: "block",
  marginTop: 6,
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  color: "var(--text-muted)",
};
const approveBtn: React.CSSProperties = {
  marginTop: 8,
  background: "var(--status-success)",
  color: "var(--text-inverse)",
  border: "none",
  borderRadius: "var(--radius-sm)",
  padding: "4px 12px",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
  cursor: "pointer",
  fontWeight: 600,
};
const liveBar: React.CSSProperties = {
  padding: "10px 14px",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
  color: "var(--status-success)",
};
const composer: React.CSSProperties = {
  borderTop: "1px solid var(--surface-line)",
  padding: 14,
  background: "var(--surface-elevated)",
};
const composerInput: React.CSSProperties = {
  width: "100%",
  background: "var(--surface-sunken)",
  border: "1px solid var(--surface-line)",
  borderRadius: "var(--radius-md)",
  color: "var(--text-primary)",
  fontFamily: "var(--font-body)",
  fontSize: 14,
  padding: 10,
  resize: "none",
  outline: "none",
};
const composerFoot: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
  marginTop: 8,
};
const composerHint: React.CSSProperties = {
  color: "var(--text-muted)",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
};
const sendBtn: React.CSSProperties = {
  background: "var(--status-success)",
  color: "var(--text-inverse)",
  border: "none",
  borderRadius: "var(--radius-sm)",
  padding: "6px 16px",
  fontSize: 12,
  fontFamily: "var(--font-mono)",
  cursor: "pointer",
  fontWeight: 600,
};

const lanesWrap: React.CSSProperties = {
  flex: 1,
  display: "flex",
  flexDirection: "column",
  minHeight: 0,
};
const lanes: React.CSSProperties = {
  flex: 1,
  display: "flex",
  gap: 1,
  background: "var(--surface-line)",
  overflowX: "auto",
  minHeight: 0,
};
const lane = (color: string): React.CSSProperties => ({
  flex: "0 0 240px",
  display: "flex",
  flexDirection: "column",
  background: "var(--surface-base)",
  borderTop: `2px solid ${color}`,
  minWidth: 0,
});
const laneHead = (_color: string): React.CSSProperties => ({
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "10px 12px",
});
const laneAvatar = (color: string): React.CSSProperties => ({
  width: 22,
  height: 22,
  borderRadius: "50%",
  display: "grid",
  placeItems: "center",
  background: `${color}22`,
  color,
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  fontWeight: 600,
  border: `1px solid ${color}55`,
});
const laneName: React.CSSProperties = { fontSize: 12, fontWeight: 600 };
const laneRole: React.CSSProperties = {
  fontSize: 10,
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
  marginLeft: "auto",
};
const laneProgress: React.CSSProperties = {
  height: 2,
  background: "var(--surface-line)",
  margin: "0 12px",
};
const laneProgressFill = (color: string, p: number): React.CSSProperties => ({
  height: "100%",
  width: `${p * 100}%`,
  background: color,
  transition: "width 600ms var(--ease-out-quart)",
});
const laneStatusLine: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  padding: "6px 12px 8px",
  fontSize: 10,
  fontFamily: "var(--font-mono)",
};
const laneStatusText = (color: string, s: Reviewer["status"]): React.CSSProperties => ({
  color: s === "done" ? "var(--text-muted)" : color,
  textTransform: "uppercase",
  letterSpacing: "0.08em",
});
const laneNow: React.CSSProperties = {
  color: "var(--text-muted)",
  maxWidth: 110,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};
const laneStream: React.CSSProperties = {
  flex: 1,
  overflowY: "auto",
  padding: "8px 12px",
  display: "flex",
  flexDirection: "column",
  gap: 8,
};
const laneEmpty: React.CSSProperties = {
  color: "var(--text-muted)",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
};
const laneComment = (sev: Reviewer["comments"][number]["severity"]): React.CSSProperties => ({
  background: "var(--surface-sunken)",
  borderRadius: "var(--radius-sm)",
  padding: "8px 10px",
  borderLeft: `2px solid ${sev === "blocker" ? "var(--status-error)" : sev === "issue" ? "var(--status-warning)" : sev === "praise" ? "var(--status-success)" : "var(--surface-line)"}`,
});
const laneCommentSev = (c: string): React.CSSProperties => ({
  fontSize: 9,
  textTransform: "uppercase",
  letterSpacing: "0.1em",
  fontFamily: "var(--font-mono)",
  color: c,
  fontWeight: 600,
});
const laneCommentLine: React.CSSProperties = {
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
  marginLeft: 6,
};
const laneCommentText: React.CSSProperties = {
  margin: "4px 0 0",
  fontSize: 11,
  lineHeight: 1.45,
  color: "var(--text-secondary)",
};
const laneTyping = (color: string): React.CSSProperties => ({
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  color,
  animation: "proto-blink 1.2s infinite",
});
const laneVerdict = (v: Reviewer["verdict"]): React.CSSProperties => ({
  margin: "0 12px 10px",
  padding: "6px",
  borderRadius: "var(--radius-sm)",
  textAlign: "center",
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  textTransform: "uppercase",
  letterSpacing: "0.1em",
  background: v === "approve" ? "var(--status-success)22" : "var(--status-warning)22",
  color: v === "approve" ? "var(--status-success)" : "var(--status-warning)",
});
const laneAxis: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  padding: "8px 14px",
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
  borderTop: "1px solid var(--surface-line)",
  textTransform: "uppercase",
  letterSpacing: "0.1em",
};
