// PROTOTYPE — throwaway. Drives the scripted agent run + parallel review playback.
import { useCallback, useEffect, useRef, useState } from "react";
import {
  type AgentTurn,
  INITIAL_REVIEWERS,
  REVIEW_SCRIPT,
  type Reviewer,
  SCRIPT,
  type ToolCall,
} from "./data";

export interface PlaybackState {
  turns: AgentTurn[]; // revealed so far (last may be partial)
  reviewers: Reviewer[];
  running: boolean;
  reviewStarted: boolean;
}

// Reveal the scripted turns one chunk at a time. Within a turn, tools flip
// pending→running→done in sequence. Then the review pipeline kicks off and all
// four reviewers advance through their scripts in parallel.
export function usePlayback(autoStart = true): PlaybackState & {
  restart: () => void;
  approve: (toolId: string) => void;
} {
  const [turns, setTurns] = useState<AgentTurn[]>([]);
  const [reviewers, setReviewers] = useState<Reviewer[]>(INITIAL_REVIEWERS);
  const [running, setRunning] = useState(false);
  const [reviewStarted, setReviewStarted] = useState(false);
  const timers = useRef<number[]>([]);
  const raf = useRef<number[]>([]);

  const clearAll = useCallback(() => {
    for (const t of timers.current) window.clearTimeout(t);
    for (const r of raf.current) window.cancelAnimationFrame(r);
    timers.current = [];
    raf.current = [];
  }, []);

  const restart = useCallback(() => {
    clearAll();
    setTurns([]);
    setReviewers(INITIAL_REVIEWERS.map((r) => ({ ...r, comments: [] })));
    setReviewStarted(false);
    setRunning(true);
    runScript();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clearAll]);

  const approve = useCallback((toolId: string) => {
    setTurns((prev) => {
      let advanced = false;
      const next = prev.map((turn) => {
        if (!turn.tools) return turn;
        const tools = turn.tools.map((tc) => {
          if (tc.id === toolId && tc.status === "awaiting") {
            advanced = true;
            return { ...tc, status: "done" as const, needsApproval: false };
          }
          return tc;
        });
        return { ...turn, tools };
      });
      if (advanced) {
        // unblock the pending run_shell after it
        const t0 = window.setTimeout(() => {
          setTurns((cur) =>
            cur.map((turn) => {
              if (!turn.tools) return turn;
              return {
                ...turn,
                tools: turn.tools.map((tc) =>
                  tc.status === "pending" && tc.kind === "run_shell"
                    ? { ...tc, status: "running" as const }
                    : tc
                ),
              };
            })
          );
          const t1 = window.setTimeout(() => {
            setTurns((cur) =>
              cur.map((turn) => {
                if (!turn.tools) return turn;
                return {
                  ...turn,
                  tools: turn.tools.map((tc) =>
                    tc.status === "running" && tc.kind === "run_shell"
                      ? { ...tc, status: "done" as const, detail: "exit 0 — 18 passed" }
                      : tc
                  ),
                };
              })
            );
          }, 900);
          timers.current.push(t1);
        }, 500);
        timers.current.push(t0);
      }
      return next;
    });
  }, []);

  function runScript() {
    let delay = 400;
    const revealTurn = (idx: number) => {
      if (idx >= SCRIPT.length) {
        // start the review pipeline after the agent run settles
        const t = window.setTimeout(() => startReview(), 700);
        timers.current.push(t);
        return;
      }
      const turn = SCRIPT[idx];
      // push the turn with tools in their initial scripted status
      const t0 = window.setTimeout(() => {
        setTurns((prev) => [...prev, structuredClone(turn)]);
        // animate tool statuses within the turn
        if (turn.tools) {
          let d = 600;
          for (const tool of turn.tools) {
            const startStatus = tool.status;
            // if it starts pending/awaiting, leave it; otherwise run→done
            if (startStatus === "done") {
              // already done in script — no-op
            } else if (startStatus === "awaiting" || startStatus === "pending") {
              // leave as-is; awaiting needs approval, pending waits
            }
            d += 700;
          }
        }
        const next = window.setTimeout(() => revealTurn(idx + 1), 1100);
        timers.current.push(next);
      }, delay);
      timers.current.push(t0);
      delay += 1300;
    };
    revealTurn(0);
  }

  function startReview() {
    setReviewStarted(true);
    // each reviewer advances through its script on its own cadence
    for (const reviewer of INITIAL_REVIEWERS) {
      const frames = REVIEW_SCRIPT[reviewer.id];
      if (!frames) continue;
      let d = 300 + Math.random() * 400;
      for (const frame of frames) {
        const t = window.setTimeout(() => {
          setReviewers((prev) =>
            prev.map((r) => {
              if (r.id !== reviewer.id) return r;
              const comments = frame.add ? [...r.comments, frame.add] : r.comments;
              return {
                ...r,
                status: frame.status,
                progress: frame.progress,
                nowCommenting: frame.nowCommenting ?? r.nowCommenting,
                verdict: frame.verdict ?? r.verdict,
                comments,
              };
            })
          );
        }, d);
        timers.current.push(t);
        d += 1100 + Math.random() * 500;
      }
      // when done, clear nowCommenting
      const end = window.setTimeout(() => {
        setReviewers((prev) =>
          prev.map((r) => (r.id === reviewer.id ? { ...r, nowCommenting: undefined } : r))
        );
      }, d);
      timers.current.push(end);
    }
    // stop "running" once all done
    const total = Math.max(
      ...INITIAL_REVIEWERS.map((r) => (REVIEW_SCRIPT[r.id]?.length ?? 0) * 1400 + 1200)
    );
    const stop = window.setTimeout(() => setRunning(false), total);
    timers.current.push(stop);
  }

  useEffect(() => {
    if (autoStart) {
      setRunning(true);
      runScript();
    }
    return clearAll;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return { turns, reviewers, running, reviewStarted, restart, approve };
}

// Helper: flatten all tool calls across revealed turns, in order.
export function allTools(turns: AgentTurn[]): ToolCall[] {
  return turns.flatMap((t) => t.tools ?? []);
}
