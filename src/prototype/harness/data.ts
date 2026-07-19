// PROTOTYPE — throwaway. Mock data simulating an agentic coding harness run.
// No backend. State lives in memory; a scripted "playback" advances the run.

export type ToolKind = "read" | "edit" | "run_shell" | "grep" | "write" | "plan";
export type ToolStatus = "pending" | "running" | "done" | "error" | "awaiting";

export interface ToolCall {
  id: string;
  kind: ToolKind;
  label: string;
  target: string; // file path or command
  status: ToolStatus;
  detail?: string; // short output preview
  lines?: number; // lines touched / read
  needsApproval?: boolean;
  diff?: DiffHunk[]; // for edit/write calls — what changed
}

export interface DiffHunk {
  oldStart: number;
  newStart: number;
  lines: DiffLine[];
}

export interface DiffLine {
  type: "add" | "del" | "ctx";
  text: string;
  oldNo?: number;
  newNo?: number;
}

export interface AgentTurn {
  id: string;
  role: "user" | "agent" | "assistant";
  text: string;
  tools?: ToolCall[];
  reasoning?: string;
}

export type ReviewerColor = "cyan" | "violet" | "amber" | "rose";
export type ReviewerStatus =
  | "queued"
  | "reading"
  | "analyzing"
  | "commenting"
  | "verdict"
  | "done";

export interface ReviewComment {
  file: string;
  line: number;
  severity: "praise" | "nit" | "issue" | "blocker";
  text: string;
}

export interface Reviewer {
  id: string;
  name: string;
  role: string; // e.g. "Security", "Architecture", "Tests", "Perf"
  color: ReviewerColor;
  status: ReviewerStatus;
  progress: number; // 0..1
  verdict?: "approve" | "request_changes" | "comment";
  comments: ReviewComment[];
  nowCommenting?: string; // file currently being inspected
}

// ── Scripted agent run ──────────────────────────────────────────────────────
// A realistic sequence: user prompt → plan → read → read → edit → run tests →
// edit → grep → edit → run tests → summary. Each step "arrives" on playback.

export const SCRIPT: AgentTurn[] = [
  {
    id: "u1",
    role: "user",
    text: "Add a retry-with-backoff wrapper around the provider streaming calls in src/hooks/useChatStream.ts. Keep it under 40 lines and don't change the public API.",
  },
  {
    id: "a1",
    role: "agent",
    reasoning:
      "Need to understand the current streaming path before wrapping it. Reading the hook and its caller in AppShell first.",
    text: "I'll start by reading the streaming hook and tracing how it's invoked.",
    tools: [
      {
        id: "t1",
        kind: "read",
        label: "read",
        target: "src/hooks/useChatStream.ts",
        status: "done",
        lines: 212,
        detail: "exposes startStream({ session, prompt, model }) → AbortController",
      },
      {
        id: "t2",
        kind: "read",
        label: "read",
        target: "src/components/AppShell.tsx",
        status: "done",
        lines: 584,
        detail: "calls startStream inside session.sendMessage; no retry on fetch error",
      },
    ],
  },
  {
    id: "a2",
    role: "agent",
    reasoning:
      "The retry boundary belongs inside startStream, wrapping the fetch + reader loop. I'll add an exponential backoff with jitter, capped at 4 attempts, only retrying on network/5xx — not on 4xx.",
    text: "Wrapping the fetch path in a retry-with-backoff helper. Adding it in the same file to keep the public API stable.",
    tools: [
      {
        id: "t3",
        kind: "edit",
        label: "edit",
        target: "src/hooks/useChatStream.ts",
        status: "done",
        lines: 38,
        detail: "added retryWithBackoff(attempts=4, base=400ms, cap=4s, jitter)",
        diff: [
          {
            oldStart: 80,
            newStart: 80,
            lines: [
              { type: "ctx", oldNo: 80, newNo: 80, text: "  const controller = new AbortController();" },
              { type: "ctx", oldNo: 81, newNo: 81, text: "" },
              { type: "add", newNo: 82, text: "  function retryWithBackoff(" },
              { type: "add", newNo: 83, text: "    fn: () => Promise<Response>," },
              { type: "add", newNo: 84, text: "    attempts = 4," },
              { type: "add", newNo: 85, text: "    base = 400," },
              { type: "add", newNo: 86, text: "    cap = 4000," },
              { type: "add", newNo: 87, text: "  ): Promise<Response> {" },
              { type: "add", newNo: 88, text: "    let lastErr: unknown;" },
              { type: "add", newNo: 89, text: "    for (let i = 0; i < attempts; i++) {" },
              { type: "add", newNo: 90, text: "      try { return await fn(); }" },
              { type: "add", newNo: 91, text: "      catch (err) {" },
              { type: "add", newNo: 92, text: "        if (!isRetryable(err)) throw err;" },
              { type: "add", newNo: 93, text: "        lastErr = err;" },
              { type: "add", newNo: 94, text: "        const delay = Math.min(cap, base * 2 ** i)" },
              { type: "add", newNo: 95, text: "          + Math.random() * base;" },
              { type: "add", newNo: 96, text: "        await sleep(delay);" },
              { type: "add", newNo: 97, text: "      }" },
              { type: "add", newNo: 98, text: "    }" },
              { type: "add", newNo: 99, text: "    throw lastErr;" },
              { type: "add", newNo: 100, text: "  }" },
              { type: "ctx", oldNo: 82, newNo: 101, text: "" },
              { type: "ctx", oldNo: 83, newNo: 102, text: "  const res = await retryWithBackoff(() =>" },
              { type: "ctx", oldNo: 84, newNo: 103, text: "    fetch(url, { ...opts, signal: controller.signal })," },
              { type: "ctx", oldNo: 85, newNo: 104, text: "  );" },
            ],
          },
        ],
      },
      {
        id: "t4",
        kind: "run_shell",
        label: "run_shell",
        target: "bun run typecheck",
        status: "done",
        detail: "exit 0 — no type errors",
      },
    ],
  },
  {
    id: "a3",
    role: "agent",
    reasoning:
      "Typecheck passes. Now I want to confirm the retry only fires on the right error classes — let me grep for where fetch errors are currently surfaced.",
    text: "Verifying error classification before running the suite.",
    tools: [
      {
        id: "t5",
        kind: "grep",
        label: "grep",
        target: '"AbortError|TypeError|5xx" src/hooks/useChatStream.ts',
        status: "done",
        detail: "3 matches — all in the reader loop, none retried today",
      },
      {
        id: "t6",
        kind: "edit",
        label: "edit",
        target: "src/hooks/useChatStream.ts",
        status: "awaiting",
        needsApproval: true,
        detail: "gate retry on isRetryable(err) — skip 4xx & AbortError",
        diff: [
          {
            oldStart: 108,
            newStart: 127,
            lines: [
              { type: "ctx", oldNo: 108, newNo: 127, text: "  }" },
              { type: "ctx", oldNo: 109, newNo: 128, text: "" },
              { type: "add", newNo: 129, text: "  function isRetryable(err: unknown): boolean {" },
              { type: "add", newNo: 130, text: "    if (err instanceof DOMException && err.name === 'AbortError') return false;" },
              { type: "add", newNo: 131, text: "    if (err instanceof TypeError) return true; // network failure" },
              { type: "add", newNo: 132, text: "    if (err instanceof Response && err.status >= 500) return true;" },
              { type: "add", newNo: 133, text: "    return false; // 4xx and other errors are not retried" },
              { type: "add", newNo: 134, text: "  }" },
            ],
          },
        ],
      },
    ],
  },
  {
    id: "a4",
    role: "agent",
    text: "Ready to run the test suite once you approve the error-classification edit.",
    tools: [
      {
        id: "t7",
        kind: "run_shell",
        label: "run_shell",
        target: "bun run test -- useChatStream",
        status: "pending",
        detail: "queued — awaiting approval of t6",
      },
    ],
  },
];

// ── Parallel review pipeline ────────────────────────────────────────────────
// Four subagents review the same diff simultaneously. Each "plays back" through
// statuses: queued → reading → analyzing → commenting → verdict → done.

export const INITIAL_REVIEWERS: Reviewer[] = [
  {
    id: "r1",
    name: "Sage",
    role: "Architecture",
    color: "cyan",
    status: "queued",
    progress: 0,
    comments: [],
  },
  {
    id: "r2",
    name: "Vex",
    role: "Security",
    color: "violet",
    status: "queued",
    progress: 0,
    comments: [],
  },
  {
    id: "r3",
    name: "Ember",
    role: "Tests & Coverage",
    color: "amber",
    status: "queued",
    progress: 0,
    comments: [],
  },
  {
    id: "r4",
    name: "Quill",
    role: "Performance",
    color: "rose",
    status: "queued",
    progress: 0,
    comments: [],
  },
];

// Per-reviewer scripted progression. Each frame advances status + progress and
// may push a comment. The prototype cycles through these on a timer.
export const REVIEW_SCRIPT: Record<
  string,
  {
    status: ReviewerStatus;
    progress: number;
    nowCommenting?: string;
    add?: ReviewComment;
    verdict?: "approve" | "request_changes" | "comment";
  }[]
> = {
  r1: [
    { status: "reading", progress: 0.15, nowCommenting: "src/hooks/useChatStream.ts" },
    { status: "analyzing", progress: 0.45 },
    {
      status: "commenting",
      progress: 0.6,
      nowCommenting: "src/hooks/useChatStream.ts",
      add: {
        file: "src/hooks/useChatStream.ts",
        line: 88,
        severity: "praise",
        text: "Backoff cap + jitter is the right call. Keeps thundering-herd risk low under partial outage.",
      },
    },
    {
      status: "commenting",
      progress: 0.78,
      add: {
        file: "src/hooks/useChatStream.ts",
        line: 104,
        severity: "nit",
        text: "Consider naming the inner fn `attemptFetch` so the stack trace reads cleaner on a failed retry.",
      },
    },
    { status: "verdict", progress: 0.95 },
    { status: "done", progress: 1, verdict: "approve" },
  ],
  r2: [
    { status: "reading", progress: 0.1, nowCommenting: "src/hooks/useChatStream.ts" },
    { status: "analyzing", progress: 0.4 },
    {
      status: "commenting",
      progress: 0.55,
      add: {
        file: "src/hooks/useChatStream.ts",
        line: 112,
        severity: "issue",
        text: "isRetryable swallows 401/403 silently into a retry loop if the auth header ever expires mid-stream. Confirm 4xx is hard-stopped.",
      },
    },
    {
      status: "commenting",
      progress: 0.72,
      add: {
        file: "src/hooks/useChatStream.ts",
        line: 96,
        severity: "nit",
        text: "Log the retry attempt count + target URL at debug, not info — avoids leaking prompt fragments in prod logs.",
      },
    },
    { status: "verdict", progress: 0.9 },
    { status: "done", progress: 1, verdict: "request_changes" },
  ],
  r3: [
    { status: "reading", progress: 0.12, nowCommenting: "src/hooks/useChatStream.ts" },
    { status: "analyzing", progress: 0.35 },
    {
      status: "commenting",
      progress: 0.5,
      add: {
        file: "src/hooks/useChatStream.ts",
        line: 120,
        severity: "issue",
        text: "No test for the backoff path. Add a unit test that stubs fetch to fail twice then succeed, asserts 3 attempts and final success.",
      },
    },
    {
      status: "commenting",
      progress: 0.7,
      add: {
        file: "src/hooks/useChatStream.ts",
        line: 8,
        severity: "nit",
        text: "Export retryWithBackoff so it's reachable from the test file without going through startStream.",
      },
    },
    { status: "verdict", progress: 0.88 },
    { status: "done", progress: 1, verdict: "request_changes" },
  ],
  r4: [
    { status: "reading", progress: 0.18, nowCommenting: "src/hooks/useChatStream.ts" },
    { status: "analyzing", progress: 0.5 },
    {
      status: "commenting",
      progress: 0.66,
      add: {
        file: "src/hooks/useChatStream.ts",
        line: 92,
        severity: "praise",
        text: "Cap at 4s is sensible for streaming. Anything higher and the user perceives a hung UI.",
      },
    },
    {
      status: "commenting",
      progress: 0.8,
      add: {
        file: "src/hooks/useChatStream.ts",
        line: 100,
        severity: "nit",
        text: "The jitter range (0–base) is fine, but consider full-jitter (0–cap*2^attempt) for tighter spread under load.",
      },
    },
    { status: "verdict", progress: 0.93 },
    { status: "done", progress: 1, verdict: "approve" },
  ],
};

export const REVIEWER_COLOR_VAR: Record<ReviewerColor, string> = {
  cyan: "var(--agent-cyan)",
  violet: "var(--agent-violet)",
  amber: "var(--agent-amber)",
  rose: "var(--agent-rose)",
};

export const TOOL_KIND_GLYPH: Record<ToolKind, string> = {
  read: "read",
  edit: "edit",
  run_shell: "$",
  grep: "grep",
  write: "write",
  plan: "plan",
};

export const SEVERITY_META: Record<
  ReviewComment["severity"],
  { label: string; color: string }
> = {
  praise: { label: "praise", color: "var(--status-success)" },
  nit: { label: "nit", color: "var(--text-muted)" },
  issue: { label: "issue", color: "var(--status-warning)" },
  blocker: { label: "blocker", color: "var(--status-error)" },
};
