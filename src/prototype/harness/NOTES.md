# Harness UI Prototype — NOTES

> **Status: PROTOTYPE — throwaway.** Delete this directory (`src/prototype/harness/`)
> and revert `src/App.tsx` + the `proto-*` keyframes in `src/styles/global.css`
> once a variant wins and is folded into the real app.

## Question being answered

What should gospel's **agentic coding harness UI** look like, taking into account
the app's current capabilities? Two sub-questions:

1. **The prompt → agent → tool-calls-visible loop.** The user writes a prompt,
   sees the agent's response, and sees every tool call (read / edit / run_shell /
   grep) as it happens — including approval gates.
2. **A creatively unique way to view the review pipeline** — multiple subagents
   reviewing the same work simultaneously, visualized in a way that isn't just a
   list.

## How to view it

```bash
bun run dev
```

Then open (port may differ if 1420 is taken — Vite prints the port):

- **Variant A — Terminal Workbench:** `http://localhost:PORT/?prototype=harness&variant=A`
- **Variant B — Review Theater:** `http://localhost:PORT/?prototype=harness&variant=B`
- **Variant C — Constellation:** `http://localhost:PORT/?prototype=harness&variant=C`

Use `←` / `→` arrow keys or the floating bottom bar to cycle variants. The
switcher and the `?prototype=harness` gate are hidden in production builds.

The prototype is fully self-contained — no Tauri backend. A scripted "playback"
simulates a streaming agent run (retry-with-backoff task) and then four review
subagents (Sage/Architecture, Vex/Security, Ember/Tests, Quill/Perf) reviewing
the diff in parallel. Hit **replay** to restart the playback.

## The three variants (structurally different, not recolours)

### A — Terminal Workbench
Single-column editorial chat feed dominates. Each agent turn expands into an
inline **tool ladder** — vertical monospace rows with status chips. Prompt
input pinned at the bottom like a terminal. Review pipeline opens as a right
**side-sheet**: stacked reviewer cards, color-coded, live status + progress +
streaming comments + verdict bar.
- Best for: reading the agent's reasoning top-to-bottom; reviewers as a quiet
  sidebar.
- Risk: reviewers compete for attention with the chat in a narrow sheet.

### B — Review Theater
Three columns: left = activity/tool timeline + prompt history; center = active
turn + composer; right = the unique twist — reviewers rendered as **parallel
horizontal lanes** you scan across. Each subagent is a vertical strip with its
own scrolling comment stream, live "typing" indicator, and verdict chip. You
compare what every reviewer is saying about the same diff *at the same time*,
like a multi-track timeline.
- Best for: the review-pipeline-as-event; comparing reviewers side-by-side.
- Risk: the chat column gets squeezed; the lane metaphor needs the diff to be
  shared context (not shown here yet).

### C — Constellation
Main area is a spatial **node canvas**. The agent is a central node; each tool
call spawns a child node connected by a curved edge. When review starts, four
reviewer nodes **orbit** the work on the right, each glowing in its agent color
with a pulsing status ring and a dashed edge back to the agent. Hover a reviewer
for a detail popover with their comments. Bottom dock = collapsible transcript +
prompt. An equalizer-style activity bar pulses while the agent runs.
- Best for: "who is looking at what, all at once" as a single glance; the most
  visually distinctive take on multi-agent review.
- Risk: spatial layouts don't scale gracefully to many tool calls / many
  reviewers; can feel like a toy at low data volumes.

## Refero references (research-first, per refero-design skill)

**Styles (visual direction):**
- **Linear Changelog** (linear.app/changelog) — dark, restrained, monochrome,
  Inter typography, capsule pills, frosted-glass command-center mood. → Variant A
  editorial tone + capsule status chips.
- **Warp** (warp.dev/code) — dark-mode precision workbench, near-black achromatic,
  electric-blue glow on terminal windows. → Variant A terminal feel + tool ladder.
- **Monologue** (monologue.to) — moody retro-futuristic midnight-terminal, oversized
  serif headlines, scan-line texture, electric-aqua accents. → Variant A hero +
  scanline overlay.
- **Vapi** (vapi.ai) — dark high-contrast, vivid accent pills, colorful
  equalizer-style capsule bars. → Variant C equalizer activity bar.
- **AgentQL** (agentql.com) — nebula-console, indigo/violet glows, glassy cards. →
  Variant C nebula glow + node cards.

**Screens (concrete UI patterns):**
- **OpenAI Assistants playground** — two-column: left config/tools, right chat
  thread + Run button. → prompt→response shape across all variants.
- **Manus** — split-screen chat + floating right editor. → docked panels.
- **Parallel deep research** — progress widget + activity log accordion + URL
  references. → reviewer progress + activity timeline (Variant B left rail).
- **Cursor PR review workspace** (multiple) — three-column charcoal layout:
  left activity/search, center PR cards, right diff. calm green/cream accents. →
  Variant B three-column structure + green positive accents.
- **n8n workflow canvas** — node-based flowchart + execution log, dark. →
  Variant C node canvas + edges.

No single reference was copied. Variant A synthesizes Linear+Warp+Monologue;
Variant B synthesizes Cursor+Parallel; Variant C synthesizes n8n+Vapi+AgentQL.
The dominant direction per variant is preserved (A: editorial terminal, B:
three-column theater, C: spatial constellation); the others contribute narrow
details only.

## Verdict (fill in before deleting the prototype)

> _Which variant won, and why? Which bits to steal from the others?_

**User feedback (first pass):** "I liked them all. Variant A's terminal type of
writing is perfect. Variant C I fell in love with but missed the terminal
portion — where do I input, or is this for the reviewer? Variant B was
interesting with the activity log. A and C captivated me most. C worries me
when an agent edits too many files — unsure how much load that would put on the
canvas, but regardless liked it the most."

**Direction locked → Variant D "Workbench Constellation" (the synthesis):**
C wins as the primary surface. A's terminal prompt is brought in as an
always-visible bottom bar (the obvious input point C was missing). B's activity
log becomes a left rail — and doubles as the scalability escape hatch for C's
load worry: when tool nodes exceed a threshold, older ones collapse into a
"+N earlier" cluster node on the canvas, while the rail always holds the full
list. Approval gates surface inline in the terminal bar so it's the single
input/approval point.

- [x] Chat loop winner: **C (constellation)** — most captivating
- [x] Review pipeline winner: **C (orbiting reviewer nodes)** — kept as-is
- [x] Bits to steal: **A's terminal prompt bar** (always-visible input) +
  **B's activity rail** (scalable full list + overflow for canvas clustering)
- [x] Canvas load solution: **clustering** — collapse older tool nodes into a
  "+N earlier" node past a threshold; rail holds the complete list; cluster
  node opens a grouped popover, "view all in activity" jumps to the rail
- [ ] Folded into real code on: ___ (then delete this directory)

### Open questions before folding into the real app
- Real clustering thresholds: 12 nodes / 10 visible is a prototype guess.
  Needs tuning against real agent runs (a 50-file refactor vs. a 5-file tweak).
- Should the activity rail be collapsible by default on small viewports? It
  competes with canvas width below ~900px.
- The reviewer popover (top-right) and cluster popover (top-left) can both be
  open — fine on desktop, needs a mobile strategy.
- Approval currently lives in three places (composer approval bar, tool node
  hover, rail item). Pick one canonical surface when folding in — the composer
  approval bar is the strongest candidate (it's where the user already is).

### Second-pass feedback → composer region added
**User feedback (second pass):** "Variant D is the best so far for me. I do not
see where user would type their prompt? Should also consider that user may want
to steer conversation so it is important that users also able to view any
streamed text agent is reasoning about as well."

**Fix applied:** Replaced the thin terminal command-line bar with a proper
**composer region** at the bottom with two parts:
1. **Streaming transcript** (collapsible, auto-scroll) — shows user prompts,
   agent reasoning (italic, muted, labeled) and agent response text as they
   arrive, plus a live "agent is reasoning…" indicator. This is the
   "see what the agent is thinking so you can steer" surface.
2. **Obvious prompt input** — a real textarea inside a rounded input card with
   a **Send button**, placeholder "Message Gospel… (⏎ to send · ⇧⏎ for newline
   · / for skills)". Reads as a message input, not a command line.

Approval gates now surface as a dedicated bar above the transcript (warning
dot + "agent wants to edit {file}" + approve button) so the composer region is
the single input/approval/steering point. The canvas stays purely for the
spatial tool/reviewer view.

### Third-pass feedback → activity rail replaced by reviewer panel
**User feedback (third pass):** "Maybe instead of the activity bar we just use
the conversation as conversation includes activity. That way we can play around
with remaining real estate."

**Change applied:** Removed the left activity rail entirely — the conversation
transcript already includes tool chips per turn, so the rail was redundant.
Replaced it with a **Reviewer panel** (320px left column): live reviewer list
with avatar, name, role, status badge, progress bar, "reading {file}" live
indicator, streaming comments (severity-colored, auto-scrolling), and verdict
bar. Summary pills at the top show approve/changes counts at a glance.

Hovering a reviewer card highlights their node on the canvas (and vice versa)
via shared `activeReviewer` state — the panel is the detail home, the canvas is
the spatial overview. The canvas reviewer nodes stay compact (avatar + status
ring + verdict dot); all detail lives in the panel.

The cluster "view all" button now opens the cluster popover directly (no rail
to jump to). Canvas gets more breathing room with the panel at 320px vs the old
248px rail, but the panel earns its width by carrying the full review pipeline.

### Fourth-pass feedback → conversation on left, canvas on right (tabbed)
**User feedback (fourth pass):** "Would it not make more sense to have
conversation history on the left? then right side can be the canvas. Does that
make sense? What do you recommend?"

**Recommendation given:** Yes — conversation-on-left is the stronger mental
model (every chat app puts it there, eye starts there in LTR, prompt input
lives at the bottom of the conversation column). But reviewers can't just be
deleted — the simultaneous-reviewer view is the whole point. Recommended a
**tabbed left column**: Conversation (default) | Reviewers, with the canvas at
full height on the right. Auto-switch to Reviewers when the review pipeline
starts so the user sees it happen; one click back to Conversation to steer.

**Change applied:** Restructured the entire layout:
- **Left column (380px):** two tabs — **Conversation** (full history with
  reasoning + response + tool chips, auto-scrolling, approval bar, and the
  prompt composer pinned at the bottom of the column) and **Reviewers** (the
  panel cards with live status, progress, streaming comments, verdicts).
- **Right side:** constellation canvas at **full height** — the spatial view
  maximized, no bottom strip eating vertical space.
- **Auto-switch:** when `reviewStarted` flips true, the tab auto-switches to
  Reviewers so the user sees the parallel review happen. A "N live" badge on
  the Reviewers tab shows how many reviewers are actively working. The user can
  switch back to Conversation at any time to keep steering.
- **No bottom strip:** the composer is now inside the Conversation tab, pinned
  at the bottom of the left column — where every chat app puts it.

Layout is now:
```
┌──────────────────────────────────────────────┐
│  header: Workbench Constellation · verdicts  │
├──────────────┬───────────────────────────────┤
│ Conversation │                               │
│  | Reviewers │     Constellation canvas      │
│  (380px)     │       (full height)           │
│              │    agent center, tools left,  │
│ [history]    │     reviewers orbit right     │
│              │                               │
│ ┌──────────┐ │                               │
│ │ Message… │ │                               │
│ └──────────┘ │                               │
└──────────────┴───────────────────────────────┘
```

### Fifth-pass feedback → resizable column, conditional reviewers, diff popover
**User feedback (fifth pass):** "User should be able to slide conversation much
wider or less wider depending on preference but should have a min width when
displayed. Also would love it if the subagents only appear on canvas if a
subagent was activated. Would be great if clicking a file on canvas displays
diff of what has changed."

**Changes applied:**
1. **Resizable left column** — a draggable splitter between the left column and
   the canvas. Drag left/right to resize the conversation/reviewers column
   between 280px (min) and 640px (max). Cursor changes to `col-resize` while
   dragging. Default is 380px.
2. **Reviewers only appear on canvas when activated** — reviewer nodes and
   their dashed edges are now gated on `reviewStarted`. Before the review
   pipeline starts, the right side of the canvas is empty — just the agent and
   its tool nodes. When review starts, reviewer nodes fade in and orbit the
   right arc. This keeps the canvas focused on what's actually happening.
3. **Click a file node to see the diff** — tool nodes for `edit`/`write` calls
   now have a `diff` badge and a signal-tinted border. Click the node (or the
   "view diff" button in the hover popover) to open a **diff popover** showing
   the actual code changes: hunk headers (`@@ -oldStart +newStart @@`), line
   numbers, `+`/`−` signs, and color-coded added/deleted/context lines. Mock
   diff data added to `data.ts` for the two edit tool calls in the script.

### Sixth-pass feedback → model/variant/mode controls in the composer
**User feedback (sixth pass):** "Where do you select which model to use, what
model variant to use, what mode (build / plan)?"

**Change applied:** Added a **controls row** at the top of the composer region
(above the textarea, inside the conversation column), containing:
- **Model selector** — dropdown with Claude, GPT-4o, Gemini, Llama. Opens
  upward (menu pops above the button) since the composer is at the bottom.
- **Variant selector** — dropdown with model-specific variants (e.g. Claude →
  Haiku/Sonnet/Opus, GPT-4o → mini/standard/o3). Resets to the first variant
  when the model changes.
- **Build / Plan mode toggle** — segmented control. In `plan` mode, the
  placeholder changes to "Describe what you want — Gospel will plan before
  building…" and the send button label changes to "Plan".

All three controls are in the composer — where the user is already looking when
they steer. The header stays clean (just status + replay). The controls are
compact (11px mono, small padding) so they don't eat into the textarea space.
