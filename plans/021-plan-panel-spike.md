# Plan 021 (D1 Spike): Investigate surfacing `.gospel/PLAN.md` as a first-class UI panel

> **Executor instructions**: This is a **design/spike plan**, not a
> build-everything plan. Your job is to investigate, prototype a thin
> read API, define the shape of the FE surfacing, and record open
> questions for the maintainer to resolve before a build plan is
> commissioned. DO NOT build the full editable panel — the spike scope is
> deliberately narrow. Run every verification command and confirm the
> expected result. If anything in the "STOP conditions" section occurs,
> stop and report. Update the status row in `plans/README.md` when done.
>
> **Drift check (run first)**: `git diff --stat 72819cd..HEAD -- src-tauri/src/workspace_tools.rs src-tauri/src/harness_profile.rs src/hooks/useConstellation.ts src/components/AppShell.tsx`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `72819cd`, 2026-07-20
- **Execution status**: IN PROGRESS — the current checkout does not contain
  the parser, command, panel, Spike Findings, or green verification evidence

## Why this matters

The product's stated reason for adding the Explicit Planning Mechanism
(see `CONTEXT.md` "Explicit Planning Mechanism" + "PEV Loop") was that
*verification and long-horizon progress tracking were limited to what fits
in conversational memory*. The substrate (`.gospel/`), the file contract
(`PLAN.md` with Goal / Steps / Evidence / Open Questions / Next Action),
the tool surface (`write_harness_file`, registered only for the main
agent), and the harness-preamble documentation all exist. The frontend has
zero references that parse or render plan content (`rg "PLAN|write_harness_file"
src` returns the CanvasTool mapping only).

A solo dev on a multi-step task currently has to open `.gospel/PLAN.md` in
an external editor to see goal/progress/Next Action — the exact
context-switch that "the tool disappears into the task" (PRODUCT.md)
exists to eliminate. This spike establishes whether the read API is one
cheap Tauri command away, what the FE seam should look like, and which
product decisions (read-only vs. editable, panel vs. drawer) the
maintainer must make before a build plan is worth writing.

This is a design/spike plan. The output is a written recommendation +
small reversible code spike (a read-only Tauri command and a one-screen
in-app preview behind a debug flag), not a productionized panel.

## Current state

- `src-tauri/src/workspace_tools.rs:79-108` — documents the Harness
  Control Area, `.gospel/` substrate, and the PLAN.md required structure:
  Goal, Steps (checklist), Evidence / Verification, Open Questions / Risks,
  Next Action. The file is plain UTF-8 markdown.
- `src-tauri/src/workspace_tools.rs:1700` (region — search
  `write_harness_file`) — registers the existing harness tool. It
  enforces the `.gospel/` prefix, creates parent dirs, caps content at
  1 MiB. Registered for the main agent in
  `src-tauri/src/harness_profile.rs:344` (search `write_harness_file`).
- `src/hooks/useConstellation.ts:65,95` — maps `write_harness_file` tool
  calls to a `"plan"` CanvasToolNode (a single dot on the Constellation),
  not parsed plan content.
- `rg "PLAN\b|plan_file|plan_path|\.gospel" src` — no FE caller reads or
  renders the plan.
- Tauri command surface: `src-tauri/src/lib.rs` `generate_handler!`
  (search the file). Existing analogues that read a file under the
  workspace: there are many read-only workspace commands (search
  `read_file` / `list_directory` / `find_files` / `context_search` in
  `lib.rs`). The new command mirrors a read-only one.
- DESIGN.md "Layout" / "Panels": the product uses overlay panels that
  slide from the right (Diff/Code Review Panel §4, File Context Panel
  §5) and the session drawer slides from the left (§3). A plan panel
  fits the right-panel overlay pattern (DESIGN.md §4/§5) — but the
  maintainer chooses (see Open Question A).
- PRODUCT.md "Show, don't tell." / "Same patterns, same places." —
  surfacing PLAN.md is exactly the verification affordance the design
  principles assume the user has.

CONTEXT.md vocabulary to reuse in any FE or backend module name:
- "Harness Control Area", "PLAN.md", "Goal", "Steps", "Evidence /
  Verification", "Open Questions / Risks", "Next Action".
- "Active Workspace Context" — the read command must require an active workspace.

## Commands you will need

| Purpose   | Command                                                                  | Expected on success |
|-----------|--------------------------------------------------------------------------|---------------------|
| Backend build/test | First run `cargo build --manifest-path src-tauri/Cargo.toml`, then run `cargo test --manifest-path src-tauri/Cargo.toml -- workspace_tools::` | both exit 0 |
| Frontend typecheck/tests | `bun run typecheck`, `bun run test`                           | both exit 0 |

## Scope

**In scope** for the spike (build minimal):
- `src-tauri/src/workspace_tools.rs` OR `src-tauri/src/lib.rs` (define a
  new read-only Tauri command `read_harness_plan` that returns the
  contents of `.gospel/PLAN.md` for the active workspace, or an empty /
  "no plan" sentinel if the file does not exist)
- A small Rust parser function in `workspace_tools.rs` that splits the
  `PLAN.md` body into the 5 documented sections (Goal / Steps / Evidence
  / Open Questions / Next Action) and returns a typed struct
  `PlanFile { goal: Option<String>, steps: Vec<PlanStep>, evidence: Vec<String>, open_questions: Vec<String>, next_action: Option<String>, has_plan_file: bool }`.
- Register the command in `generate_handler!`.
- `src-tauri/src/harness_plan.rs` (new module) — keep the parser
  isolated so its surface is testable without the Tauri command plumbing.

**In scope** for the spike (FE thin preview):
- A minimal `src/components/PlanPanel.tsx` that calls the new command on
  workspace change and renders the 5 sections (read-only). Rendered only
  behind a temporary debug flag (e.g. `?panel=plan` query param or a
  `localStorage` flag — confirm the existing prototype-gating pattern in
  `src/App.tsx` and mirror it). NOT wired as a default-toggled overlay.

**Out of scope**:
- Editing PLAN.md from the UI (a separate follow-up — the spike's job is
  to inform that decision, not build it).
- Subscribing to file-watch events for live updates (use a re-read on
  workspace change + an explicit Refresh button; file-watch is a build
  decision, deferred).
- The Constellation "plan" node rewrite — orthogonal.
- BE ML-driven re-rolling (the existing `write_harness_file` stays as the
  agent's only write path).
- A persistence migration (PLAN.md is plain text today; no schema change).
- Production rollout of the panel behind a feature flag cleanup (this is
  debug-only for the spike).

## Git workflow

- Branch: `advisor/021-plan-panel-spike`
- Two commits suggested: one for the BE command + parser + tests, one for
  the FE thin-preview. Examples: `feat: add read_harness_plan command`;
  `feat: spike read-only PlanPanel behind debug flag`.
- Do NOT push unless instructed.

## Steps

### Step 1: Investigate substrate + define the section parser

Read `.gospel/PLAN.md` examples (if any exist on the dev workspace):
use `ls .gospel` from the working tree. If none, create a temporary
sample consistent with CONTEXT.md's required structure (Goal / Steps /
Evidence / Open Questions / Next Action) — keep the sample minimal and
delete it before committing (do not commit a sample plan to the repo).

Decide the parse contract (record the decision in the spike's
`investigation-notes` section in the commit body or a new doc):
- The Goal line is whatever's under a `## Goal` heading (or without
  heading, the first plain paragraph per CONTEXT.md's description).
- Steps are a markdown checklist (`- [ ]` / `- [x]`).
- Evidence / Open Questions are paragraphs under their respective headings.
- Next Action is a single `## Next Action` paragraph (may include a
  checklist with one item).

Add `src-tauri/src/harness_plan.rs` and expose a pure function:

```rust
pub struct PlanFile {
    pub goal: Option<String>,
    pub steps: Vec<PlanStep>,        // PlanStep { text: String, done: bool }
    pub evidence: Vec<String>,
    pub open_questions: Vec<String>,
    pub next_action: Option<String>,
    pub has_plan_file: bool,
}

pub fn parse_plan_markdown(content: &str) -> PlanFile { ... }
```

Use the repo's markdown-parser choice if one is already in the workspace
(search `rg "pulldown|markdown|comrak" src-tauri/Cargo.toml`). If none,
implement a small line-based parser (the PLAN.md contract is stable
section headings only — no full markdown AST needed).

Add tests in `harness_plan.rs` covering:
1. Happy path (all 5 sections present).
2. Missing file sentinel (returns `has_plan_file: false` and empty
   fields).
3. Partial file (e.g. only Goal + Steps).
4. Mixed-heading-style (`# Goal` vs `## Goal` — choose one canonical form
   and document in the struct doc).

### Step 2: Add the Tauri read-only command

In `src-tauri/src/lib.rs` (or wherever the workspace read-only commands
live — search `read_file`'s command definition for placement), add a command
that accepts no caller-supplied workspace path:

```rust
#[tauri::command]
fn read_harness_plan(
    app_config: tauri::State<'_, AppConfigState>,
) -> Result<PlanFile, String> {
    let workspace = app_config
        .store
        .as_ref()
        .ok_or_else(|| "App config store is unavailable".to_string())?
        .get_active_workspace()
        .map_err(|e| format!("Failed to get active workspace: {e}"))?
        .ok_or_else(|| "No active workspace selected".to_string())?;

    match workspace_tools::read_workspace_text(
        Path::new(&workspace.path),
        Path::new(".gospel/PLAN.md"),
    )? {
        Some(content) => Ok(parse_plan_markdown(&content)),
        None => Ok(PlanFile { has_plan_file: false, ..Default::default() }),
    }
}
```

Extract or expose a crate-private `read_workspace_text` helper from the safe
target-resolution and UTF-8 read path used by `ReadFileTool::call` in
`workspace_tools.rs`. It must construct `WorkspaceAccess` from the trusted
active workspace, resolve only the relative `.gospel/PLAN.md` target, and
preserve the existing canonicalization, traversal rejection, symlink-escape
rejection, regular-file check, size cap, binary/UTF-8 checks, and missing-file
distinction. Do not route this command through an arbitrary path received from
the frontend and do not replace the safe-read path with raw
`std::fs::read_to_string`.

Register `read_harness_plan` in `generate_handler!`.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml -- harness_plan::`
→ tests pass. `cargo build --manifest-path src-tauri/Cargo.toml` → exit 0.
`cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` → exit 0.

### Step 3: Thin read-only preview behind a debug flag

In `src/App.tsx`, find how `?prototype=harness` is currently gated (search
`isHarnessPrototypeRequest` / `URLSearchParams` / `import.meta.env`). Add a
parallel `?panel=plan` (or equivalent) debug gate.

Create `src/components/PlanPanel.tsx`:

```tsx
export function PlanPanel({ workspaceId }: { workspaceId: string | null }) {
  const [plan, setPlan] = useState<PlanFile | null>(null);
  const refresh = useCallback(async () => {
    if (!workspaceId) return;
    const p = await invoke<PlanFile>("read_harness_plan");
    setPlan(p);
  }, [workspaceId]);
  useEffect(() => { void refresh(); }, [refresh]);
  // Render the 5 sections (read-only) with a "Refresh" button.
  // If plan.has_plan_file === false, render "No plan yet".
}
```

Type the PlanFile shape in `src/types/index.ts` (search an existing type
like `Session` for the export convention).

Render `<PlanPanel />` only when the debug flag is on (cosmetic placement;
the design decision for production placement is recorded as an open
question below).

**Verify**: `bun run typecheck` → exit 0. `bun run test` → full suite
green. (Manually verify with `bun run tauri dev` + `?panel=plan` + a
workspace with `.gospel/PLAN.md`; this manual check is the spike's
deliverable inspection.)

### Step 4: Write up the spike's findings

Append a `## Spike Findings` section at the end of THIS plan file
(plans/021-…) with:

1. The exact parse contract decision (Step 1, item 1).
2. The file-watch viability (cheap to add later via Tauri's fs watcher
   plugin? defer — confirm via a quick research pass).
3. **Open Question A**: read-only mirror vs. editor in panel. The
   maintainer's call. Lay out the trade-offs in 3-5 bullets:
   - Read-only: trivially safe, no edit/agent-write race; the agent stays
     the only writer (matches CONTEXT.md "Skill-agnostic contract" — the
     plan is the agent's outer-loop artifact). Cost: user has to ask the
     agent to update; can't scratch-edit before agent resumes.
   - Editor: higher leverage, but requires merging user edits with agent
     writes (concurrent write / atomic update policy). Tauri's fs events
     or a write_lock.
   - Recommendation (spike's).
4. **Open Question B**: panel placement — right-side overlay (DESIGN.md
   §4/§5 panel pattern) vs. a tab in the Constellation (which already
   renders a "plan" tool node — could upgrade the node into the actual
   panel). Recommendation.
5. The list of files a future build plan would touch — so the next
   advisor stays on rails.

### Step 5: Smoke verify + reset spike-only state

Run the full verification (Steps above).

Before finishing, remove any temporary sample `.gospel/PLAN.md` created
during investigation. Do NOT remove `src/components/PlanPanel.tsx` or
the backend command — those are the spike's intentionally-left
artifacts (gated by a debug flag, so they don't affect production runs).

**Verify**: `bun run test`, `bun run typecheck`, `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`. All exit 0.

## Test plan

- New `harness_plan.rs` parser tests (Step 1) — these are the durable
  artifact; cover the 4 cases.
- FE smoke: it's read-only thin preview, gated by a debug flag. No FE
  test required for the spike (the build plan will write FE tests once
  placement is decided).
- Backend command-level test is deferred to the build plan (the spike's
  payload shape and the active-workspace-path plumbing are the spike's
  outputs).

## Done criteria

Completion reconciliation (2026-07-21): all criteria remain unchecked in the
current checkout. The prior worktree/branch record is not sufficient evidence:
the checked-out tree lacks the required artifacts and no successful full
command/test verification is recorded here. Keep this plan and the index `IN
PROGRESS` until every item below is implemented, verified, and accompanied by
the Spike Findings appendix.

- [ ] `src-tauri/src/harness_plan.rs` exists with `parse_plan_markdown` + ≥4 unit tests passing
- [ ] `read_harness_plan` Tauri command registered in `generate_handler!`
- [ ] `rg "read_harness_plan" src-tauri/src/lib.rs` returns the command definition
- [ ] `src/components/PlanPanel.tsx` exists, gated behind the debug flag, calling `invoke<PlanFile>("read_harness_plan")` without a caller-supplied workspace path
- [ ] `rg "PlanPanel" src/App.tsx` returns the gated render
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] `bun run typecheck` and `bun run test` exit 0
- [ ] This plan file contains a `## Spike Findings` section at the end with Open Questions A and B + a file-list-a-build-plan-would-touch note
- [ ] No stray `.gospel/PLAN.md` test artifacts committed (`git status` is clean of incidental additions)
- [ ] `plans/README.md` status row for plan 021 updated

## STOP conditions

Stop and report back if:

- The existing workspace-aware file-read commands require large
  plumbing (e.g. a session-bound context) that this small read command
  can't satisfy — STOP and propose the seam (the spike's read command
  should NOT require session state).
- A `parse_plan_markdown` would actually need a full markdown parser
  (the PLAN.md contract is observably looser than the File Contract from
  CONTEXT.md suggests) — STOP and ask the maintainer to canonicalize the
  PLAN.md structure before any parser is written; this is exactly the
  kind of issue the spike surfaces.
- The FE debug-gate pattern in `src/App.tsx` does not exist or is
  inapplicable to a panel render (only the prototype variant pattern
  exists) — STOP and propose an alternative spike gating (e.g. a
  dev-build check via `import.meta.env.DEV`).
- The agent's `write_harness_file` and any future user-side edits to
  PLAN.md create a write race the spike's read-only mirror can't observe
  cleanly — record as Open Question A finding; do NOT attempt to solve
  the concurrency here (the spike is read-only).

## Maintenance notes

- The spike leaves two artifacts (a backend command + a debug-gated FE
  panel). A subsequent "Plan Panel Build" plan MUST resolve Open
  Questions A and B before productionizing.
- Reviewer: confirm the debug gate is OFF by default (no production
  users should see the panel without opt-in); confirm the parser tests
  pin the documented contract.
- Follow-up deferred (out of spike scope): (1) file-watch live updates;
  (2) edit-from-UI; (3) productionizing the panel; (4) Constellation
  node upgrade. Each becomes a separate plan once Open Questions A and B
  are resolved.
