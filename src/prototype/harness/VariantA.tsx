// PROTOTYPE — throwaway. Variant A: "Terminal Workbench"
// References: Linear Changelog (editorial monochrome, capsule pills),
// Warp (dark workbench, electric-blue glow on terminal windows),
// Monologue (oversized serif headlines, scan-line texture).
//
// Shape: single-column chat feed dominates. Each agent turn expands into an
// inline "tool ladder" — vertical monospace rows with status chips. Prompt
// input pinned at bottom like a terminal. Review pipeline opens as a right
// side-sheet: stacked reviewer cards, color-coded, live status.
import { useEffect, useRef, useState } from "react";
import {
  REVIEWER_COLOR_VAR,
  type Reviewer,
  SEVERITY_META,
  TOOL_KIND_GLYPH,
  type ToolCall,
} from "./data";
import { usePlayback } from "./usePlayback";

export function VariantA() {
  const { turns, reviewers, running, reviewStarted, restart, approve } = usePlayback();
  const [reviewOpen, setReviewOpen] = useState(true);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [turns]);

  const verdicts = reviewers.filter((r) => r.verdict);
  const approved = verdicts.filter((r) => r.verdict === "approve").length;
  const changes = verdicts.filter((r) => r.verdict === "request_changes").length;

  return (
    <div style={shell}>
      <header style={header}>
        <div style={headerLeft}>
          <span style={dot(running ? "var(--accent-action)" : "var(--text-muted)")} />
          <span style={headerTitle}>gospel / harness</span>
          <span style={headerSub}>agent run · retry-with-backoff</span>
        </div>
        <div style={headerRight}>
          <button type="button" style={chipBtn} onClick={() => setReviewOpen((v) => !v)}>
            {reviewOpen ? "hide review" : "show review"}
          </button>
          <button type="button" style={chipBtn} onClick={restart}>
            replay
          </button>
        </div>
      </header>

      <div style={body}>
        <main style={mainCol} ref={scrollRef}>
          <div style={scanline} aria-hidden />
          <div style={feed}>
            <div style={hero}>
              <h1 style={heroTitle}>Terminal Workbench</h1>
              <p style={heroSub}>prompt in, agent out, every tool call visible on the ladder.</p>
            </div>
            {turns.map((turn) => (
              <TurnBlock key={turn.id} turn={turn} onApprove={approve} />
            ))}
            {running && <Typing />}
          </div>
        </main>

        {reviewOpen && (
          <aside style={sideSheet}>
            <div style={sideHead}>
              <span style={sideTitle}>Review pipeline</span>
              <span style={sideMeta}>
                {reviewStarted ? `${verdicts.length}/${reviewers.length} verdicts` : "queued"}
              </span>
            </div>
            <div style={sideSummary}>
              <span style={summaryPill("var(--status-success)")}>{approved} approve</span>
              <span style={summaryPill("var(--status-warning)")}>{changes} changes</span>
            </div>
            <div style={reviewerStack}>
              {reviewers.map((r) => (
                <ReviewerCard key={r.id} r={r} />
              ))}
            </div>
          </aside>
        )}
      </div>

      <PromptBar disabled={running} onSend={() => {}} />
    </div>
  );
}

function TurnBlock({
  turn,
  onApprove,
}: {
  turn: import("./data").AgentTurn;
  onApprove: (id: string) => void;
}) {
  const isUser = turn.role === "user";
  return (
    <div style={turnWrap}>
      <div style={turnHead(isUser)}>
        <span style={turnRole(isUser)}>{isUser ? "you" : "agent"}</span>
        <span style={turnTime}>·</span>
      </div>
      {turn.reasoning && !isUser && (
        <div style={reasoning}>
          <span style={reasoningLabel}>reasoning</span>
          <p style={reasoningText}>{turn.reasoning}</p>
        </div>
      )}
      <p style={turnText(isUser)}>{turn.text}</p>
      {turn.tools && turn.tools.length > 0 && (
        <div style={ladder}>
          {turn.tools.map((tc) => (
            <ToolRow key={tc.id} tc={tc} onApprove={onApprove} />
          ))}
        </div>
      )}
    </div>
  );
}

function ToolRow({ tc, onApprove }: { tc: ToolCall; onApprove: (id: string) => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div style={toolRow}>
      <button type="button" style={toolRowBtn} onClick={() => setOpen((v) => !v)}>
        <span style={toolGlyph(tc.kind)}>{TOOL_KIND_GLYPH[tc.kind]}</span>
        <span style={toolTarget}>{tc.target}</span>
        <span style={toolStatus(tc.status)}>{statusLabel(tc.status)}</span>
        {tc.lines ? <span style={toolLines}>{tc.lines}L</span> : null}
      </button>
      {tc.detail && (open || tc.needsApproval) && (
        <div style={toolDetail}>
          <code style={toolDetailCode}>{tc.detail}</code>
          {tc.needsApproval && tc.status === "awaiting" && (
            <button type="button" style={approveBtn} onClick={() => onApprove(tc.id)}>
              approve edit
            </button>
          )}
        </div>
      )}
    </div>
  );
}

function ReviewerCard({ r }: { r: Reviewer }) {
  const color = REVIEWER_COLOR_VAR[r.color];
  return (
    <div style={reviewerCard(color)}>
      <div style={reviewerHead}>
        <span style={reviewerAvatar(color)}>{r.name[0]}</span>
        <div style={reviewerMeta}>
          <span style={reviewerName}>{r.name}</span>
          <span style={reviewerRole}>{r.role}</span>
        </div>
        <span style={reviewerStatus(color, r.status)}>{r.status}</span>
      </div>
      <div style={progressTrack}>
        <div style={progressFill(color, r.progress)} />
      </div>
      {r.nowCommenting && (
        <div style={nowCommenting}>
          <span style={pulseDot(color)} />
          <span>reading {r.nowCommenting}</span>
        </div>
      )}
      {r.comments.length > 0 && (
        <div style={commentList}>
          {r.comments.map((c, i) => {
            const sev = SEVERITY_META[c.severity];
            return (
              <div key={i} style={commentItem}>
                <span style={commentSev(sev.color)}>{sev.label}</span>
                <span style={commentLoc}>
                  {c.file}:{c.line}
                </span>
                <p style={commentText}>{c.text}</p>
              </div>
            );
          })}
        </div>
      )}
      {r.verdict && (
        <div style={verdictBar(r.verdict)}>
          <span>{r.verdict === "approve" ? "✓ approved" : "→ changes requested"}</span>
        </div>
      )}
    </div>
  );
}

function Typing() {
  return (
    <div style={typing}>
      <span style={typingDot} />
      <span style={typingDot} />
      <span style={typingDot} />
    </div>
  );
}

function PromptBar({ disabled, onSend }: { disabled: boolean; onSend: () => void }) {
  const [v, setV] = useState("");
  return (
    <div style={promptBar}>
      <span style={promptChevron}>❯</span>
      <input
        style={promptInput}
        placeholder={disabled ? "agent working…" : "prompt the agent — ⏎ to send"}
        value={v}
        disabled={disabled}
        onChange={(e) => setV(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && v.trim()) {
            onSend();
            setV("");
          }
        }}
      />
      <span style={promptHint}>⌘↵ send · / for skills</span>
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
const header: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
  padding: "10px 20px",
  borderBottom: "1px solid var(--surface-line)",
  background: "var(--surface-sunken)",
  flexShrink: 0,
};
const headerLeft: React.CSSProperties = { display: "flex", alignItems: "center", gap: 10 };
const headerRight: React.CSSProperties = { display: "flex", gap: 8 };
const dot = (c: string): React.CSSProperties => ({
  width: 8,
  height: 8,
  borderRadius: "50%",
  background: c,
  boxShadow: `0 0 8px ${c}`,
});
const headerTitle: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 13,
  letterSpacing: "0.02em",
};
const headerSub: React.CSSProperties = { color: "var(--text-muted)", fontSize: 12 };
const chipBtn: React.CSSProperties = {
  background: "transparent",
  border: "1px solid var(--surface-line)",
  color: "var(--text-secondary)",
  borderRadius: "var(--radius-full)",
  padding: "4px 12px",
  fontSize: 12,
  cursor: "pointer",
  fontFamily: "var(--font-mono)",
};
const body: React.CSSProperties = { display: "flex", flex: 1, minHeight: 0 };
const mainCol: React.CSSProperties = {
  flex: 1,
  overflowY: "auto",
  position: "relative",
};
const scanline: React.CSSProperties = {
  position: "absolute",
  inset: 0,
  pointerEvents: "none",
  background:
    "repeating-linear-gradient(0deg, transparent 0, transparent 3px, rgba(255,255,255,0.012) 3px, rgba(255,255,255,0.012) 4px)",
  zIndex: 1,
};
const feed: React.CSSProperties = {
  maxWidth: 760,
  margin: "0 auto",
  padding: "32px 28px 120px",
  position: "relative",
  zIndex: 2,
};
const hero: React.CSSProperties = { marginBottom: 36, textAlign: "left" };
const heroTitle: React.CSSProperties = {
  fontFamily: "var(--font-display)",
  fontSize: 30,
  fontWeight: 500,
  letterSpacing: "-0.02em",
  margin: 0,
  color: "var(--text-primary)",
};
const heroSub: React.CSSProperties = {
  color: "var(--text-muted)",
  fontSize: 13,
  margin: "6px 0 0",
  fontFamily: "var(--font-mono)",
};
const turnWrap: React.CSSProperties = { marginBottom: 28 };
const turnHead = (_isUser: boolean): React.CSSProperties => ({
  display: "flex",
  alignItems: "center",
  gap: 8,
  marginBottom: 8,
});
const turnRole = (isUser: boolean): React.CSSProperties => ({
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  textTransform: "uppercase",
  letterSpacing: "0.12em",
  color: isUser ? "var(--accent-action)" : "var(--accent-structure)",
});
const turnTime: React.CSSProperties = { color: "var(--text-muted)", fontSize: 11 };
const reasoning: React.CSSProperties = {
  borderLeft: "2px solid var(--surface-line)",
  padding: "8px 14px",
  marginBottom: 10,
  background: "var(--surface-sunken)",
  borderRadius: "0 var(--radius-sm) var(--radius-sm) 0",
};
const reasoningLabel: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 10,
  textTransform: "uppercase",
  letterSpacing: "0.14em",
  color: "var(--text-muted)",
};
const reasoningText: React.CSSProperties = {
  margin: "4px 0 0",
  fontSize: 13,
  color: "var(--text-secondary)",
  lineHeight: 1.55,
  fontStyle: "italic",
};
const turnText = (isUser: boolean): React.CSSProperties => ({
  fontSize: 15,
  lineHeight: 1.6,
  color: isUser ? "var(--text-primary)" : "var(--text-secondary)",
  margin: 0,
});
const ladder: React.CSSProperties = {
  marginTop: 12,
  display: "flex",
  flexDirection: "column",
  gap: 4,
};
const toolRow: React.CSSProperties = { display: "flex", flexDirection: "column" };
const toolRowBtn: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 10,
  padding: "7px 10px",
  background: "var(--surface-elevated)",
  border: "1px solid var(--surface-line)",
  borderRadius: "var(--radius-sm)",
  cursor: "pointer",
  textAlign: "left",
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  color: "var(--text-secondary)",
};
const toolGlyph = (k: ToolCall["kind"]): React.CSSProperties => ({
  color:
    k === "edit" || k === "write"
      ? "var(--accent-signal)"
      : k === "run_shell"
        ? "var(--accent-action)"
        : k === "grep"
          ? "var(--accent-data)"
          : "var(--text-muted)",
  minWidth: 36,
  fontWeight: 600,
});
const toolTarget: React.CSSProperties = {
  flex: 1,
  color: "var(--text-primary)",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};
const toolStatus = (s: ToolCall["status"]): React.CSSProperties => ({
  fontSize: 10,
  textTransform: "uppercase",
  letterSpacing: "0.1em",
  color:
    s === "done"
      ? "var(--status-success)"
      : s === "running"
        ? "var(--accent-action)"
        : s === "error"
          ? "var(--status-error)"
          : s === "awaiting"
            ? "var(--status-warning)"
            : "var(--text-muted)",
});
const toolLines: React.CSSProperties = { color: "var(--text-muted)", fontSize: 10 };
const toolDetail: React.CSSProperties = {
  marginTop: 4,
  padding: "8px 12px",
  background: "var(--surface-sunken)",
  border: "1px solid var(--surface-line)",
  borderRadius: "var(--radius-sm)",
  display: "flex",
  alignItems: "center",
  gap: 12,
  flexWrap: "wrap",
};
const toolDetailCode: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  color: "var(--text-muted)",
  flex: 1,
};
const approveBtn: React.CSSProperties = {
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
const typing: React.CSSProperties = { display: "flex", gap: 4, padding: "8px 0" };
const typingDot: React.CSSProperties = {
  width: 5,
  height: 5,
  borderRadius: "50%",
  background: "var(--text-muted)",
  animation: "proto-blink 1.2s infinite",
};
const promptBar: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 12,
  padding: "14px 24px",
  background: "var(--surface-sunken)",
  borderTop: "1px solid var(--surface-line)",
  flexShrink: 0,
};
const promptChevron: React.CSSProperties = {
  color: "var(--accent-action)",
  fontFamily: "var(--font-mono)",
  fontSize: 16,
};
const promptInput: React.CSSProperties = {
  flex: 1,
  background: "transparent",
  border: "none",
  outline: "none",
  color: "var(--text-primary)",
  fontFamily: "var(--font-mono)",
  fontSize: 14,
};
const promptHint: React.CSSProperties = {
  color: "var(--text-muted)",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
};

const sideSheet: React.CSSProperties = {
  width: 360,
  borderLeft: "1px solid var(--surface-line)",
  background: "var(--surface-elevated)",
  display: "flex",
  flexDirection: "column",
  flexShrink: 0,
};
const sideHead: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "baseline",
  padding: "16px 18px 10px",
  borderBottom: "1px solid var(--surface-line)",
};
const sideTitle: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  textTransform: "uppercase",
  letterSpacing: "0.12em",
};
const sideMeta: React.CSSProperties = {
  color: "var(--text-muted)",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
};
const sideSummary: React.CSSProperties = { display: "flex", gap: 8, padding: "10px 18px" };
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
const reviewerStack: React.CSSProperties = {
  flex: 1,
  overflowY: "auto",
  padding: "8px 14px 20px",
  display: "flex",
  flexDirection: "column",
  gap: 10,
};
const reviewerCard = (color: string): React.CSSProperties => ({
  background: "var(--surface-base)",
  border: "1px solid var(--surface-line)",
  borderLeft: `2px solid ${color}`,
  borderRadius: "var(--radius-md)",
  padding: 12,
});
const reviewerHead: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 10,
  marginBottom: 8,
};
const reviewerAvatar = (color: string): React.CSSProperties => ({
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
});
const reviewerMeta: React.CSSProperties = { display: "flex", flexDirection: "column", flex: 1 };
const reviewerName: React.CSSProperties = { fontSize: 13, fontWeight: 600 };
const reviewerRole: React.CSSProperties = {
  fontSize: 11,
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
};
const reviewerStatus = (color: string, s: Reviewer["status"]): React.CSSProperties => ({
  fontSize: 10,
  textTransform: "uppercase",
  letterSpacing: "0.1em",
  fontFamily: "var(--font-mono)",
  color: s === "done" ? "var(--text-muted)" : color,
});
const progressTrack: React.CSSProperties = {
  height: 3,
  background: "var(--surface-line)",
  borderRadius: 2,
  overflow: "hidden",
};
const progressFill = (color: string, p: number): React.CSSProperties => ({
  height: "100%",
  width: `${p * 100}%`,
  background: color,
  transition: "width 600ms var(--ease-out-quart)",
});
const nowCommenting: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  marginTop: 8,
  fontSize: 11,
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
};
const pulseDot = (color: string): React.CSSProperties => ({
  width: 6,
  height: 6,
  borderRadius: "50%",
  background: color,
  animation: "proto-pulse 1s infinite",
});
const commentList: React.CSSProperties = {
  marginTop: 10,
  display: "flex",
  flexDirection: "column",
  gap: 8,
};
const commentItem: React.CSSProperties = {
  background: "var(--surface-sunken)",
  borderRadius: "var(--radius-sm)",
  padding: "8px 10px",
  display: "grid",
  gridTemplateColumns: "auto auto",
  gap: "2px 8px",
  alignItems: "baseline",
};
const commentSev = (c: string): React.CSSProperties => ({
  fontSize: 9,
  textTransform: "uppercase",
  letterSpacing: "0.1em",
  fontFamily: "var(--font-mono)",
  color: c,
  fontWeight: 600,
});
const commentLoc: React.CSSProperties = {
  fontSize: 10,
  fontFamily: "var(--font-mono)",
  color: "var(--text-muted)",
};
const commentText: React.CSSProperties = {
  gridColumn: "1 / -1",
  margin: 0,
  fontSize: 12,
  lineHeight: 1.5,
  color: "var(--text-secondary)",
};
const verdictBar = (v: Reviewer["verdict"]): React.CSSProperties => ({
  marginTop: 10,
  padding: "6px 10px",
  borderRadius: "var(--radius-sm)",
  fontSize: 11,
  fontFamily: "var(--font-mono)",
  textAlign: "center",
  background: v === "approve" ? "var(--status-success)22" : "var(--status-warning)22",
  color: v === "approve" ? "var(--status-success)" : "var(--status-warning)",
});

function statusLabel(s: ToolCall["status"]): string {
  return s === "awaiting" ? "needs approval" : s;
}
