# Plan 021 (D1 Spike): Investigate surfacing `.gospel/PLAN.md` as a first-class UI panel

> Inlined from the advisor's plan 021. This file is the executor's working
> copy; the reviewer maintains the index in `plans/README.md`.

## Status
- Priority: P2, Effort: M, Risk: MED, Depends on: none, Category: direction, Planned at: commit `72819cd`, 2026-07-20
- Executed at: commit (see git log on `advisor/021-plan-panel-spike`), 2026-07-20

## Why this matters
The product's stated reason for adding the Explicit Planning Mechanism (CONTEXT.md "Explicit Planning Mechanism" + "PEV Loop") was that verification and long-horizon progress tracking were limited to what fits in conversational memory. The substrate (`.gospel/`), the file contract (PLAN.md with Goal / Steps / Evidence / Open Questions / Next Action), the tool surface (`write_harness_file`), and the harness-preamble documentation all exist. The frontend had zero references that parse or render plan content.

A solo dev on a multi-step task had to open `.gospel/PLAN.md` in an external editor — the exact context-switch that "the tool disappears into the task" (PRODUCT.md) exists to eliminate. This spike establishes whether the read API is one cheap Tauri command away (it is), what the FE seam should look like, and which product decisions the maintainer must make before a build plan is worth writing.

This is a design/spike plan. The output is a written recommendation + small reversible code spike (a read-only Tauri command and a one-screen in-app preview behind a debug flag), not a productionized panel.

## Scope
**In scope** for the spike (built minimal):
- `src-tauri/src/harness_plan.rs` (new module) — parser isolated so its surface is testable without the Tauri command plumbing.
- A read-only Tauri command `read_harness_plan` in `src-tauri/src/lib.rs` that returns the contents of `.gospel/PLAN.md` for the active workspace, or an empty/"no plan" sentinel if the file does not exist.
- The command is registered in `generate_handler!`.
- `src/components/PlanPanel.tsx` (new) — minimal read-only panel that calls the new command on workspace change and renders the 5 sections. Rendered only behind a temporary debug flag (`?panel=plan` query param, mirroring the existing `?prototype=harness` gate). NOT wired as a default-toggled overlay.
- `src/types/index.ts` — `PlanFile` / `PlanStep` TypeScript types.

**Out of scope** (deferred):
- Editing PLAN.md from the UI.
- File-watch live updates (re-read on workspace change + explicit Refresh button only).
- Constellation "plan" node rewrite.
- BE ML-driven re-rolling.
- Persistence migration.
- Production rollout / feature-flag cleanup.

## Steps (executed)
1. **Investigate substrate + define the section parser** — DONE. The dev workspace already has `.gospel/PLAN.md` tracked in the repo (no sample needed to be created or cleaned up). Parser contract decided and documented in `harness_plan.rs` module docs: canonical `## <NAME>` headings, `# Goal` single-hash tolerated as an alias, line-based parser (no markdown dependency — the file contract is stable section headings only). 6 unit tests added covering happy path, missing-file sentinel, partial plan, mixed-heading-style, Next Action checklist form, and unknown headings.
2. **Add the Tauri read-only command** — DONE. `read_harness_plan` added in `src-tauri/src/lib.rs` next to `get_active_workspace`. Mirrors `get_corpus_status`'s workspace resolution (`AppConfigState` → `get_active_workspace` → `PathBuf`) and reuses `corpus::symlink_guard` (`canonical`, `validate_existing_ancestors`, `is_within`) to bound `.gospel/PLAN.md` under the workspace root, matching the existing `write_harness_file` guard. Registered in `generate_handler!`.
3. **Thin read-only preview behind a debug flag** — DONE. `src/App.tsx` gained a parallel `isPlanPanelRequest()` gate that mirrors `isHarnessPrototypeRequest()` (off in `PROD`, on only when `?panel=plan`). `PlanPanel` renders as a right-side overlay, calls `invoke<PlanFile>("read_harness_plan", { activeWorkspacePath })` on workspace change, has a Refresh button, and renders the 5 sections or "No plan yet". `PlanFile` / `PlanStep` types added to `src/types/index.ts`.
4. **Write up the spike's findings** — DONE (see `## Spike Findings` below).
5. **Smoke verify + reset spike-only state** — DONE. No temporary `.gospel/PLAN.md` was created (the dev workspace's tracked plan served as the inspection target). `PlanPanel.tsx` and the backend command are intentionally left as debug-gated artifacts.

## Commands
| Purpose | Command | Result |
| Backend build | `cargo build --manifest-path src-tauri/Cargo.toml` | exit 0 |
| Backend tests (parser) | `cargo test --manifest-path src-tauri/Cargo.toml -- harness_plan` | 6 passed |
| Backend clippy | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | exit 0 |
| Frontend typecheck | `bun run typecheck` | exit 0 |
| Frontend tests | `bun run test` | 116 passed (15 files) |

## Done criteria
- [x] `src-tauri/src/harness_plan.rs` exists with `parse_plan_markdown` + ≥4 unit tests passing (6 added)
- [x] `read_harness_plan` Tauri command registered in `generate_handler!`
- [x] `rg "read_harness_plan" src-tauri/src/lib.rs` returns the command definition
- [x] `src/components/PlanPanel.tsx` exists, gated behind the debug flag, calling `invoke<PlanFile>("read_harness_plan", { ... })`
- [x] `rg "PlanPanel" src/App.tsx` returns the gated render
- [x] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [x] `bun run typecheck` and `bun run test` exit 0
- [x] This plan file contains a `## Spike Findings` section with Open Questions A and B + a file-list-a-build-plan-would-touch note
- [x] No stray `.gospel/PLAN.md` test artifacts committed (`git status` clean of incidental additions; the tracked `.gospel/PLAN.md` was already in the repo and is unchanged)
- [ ] `plans/README.md` status row for plan 021 updated — SKIPPED (reviewer maintains the index)

## Spike Findings

### 1. Parse contract decision (Step 1)
The parser is line-based and canonicalises on `## <NAME>` section headings. Recognised sections (case-insensitive, leading `#` count 1–3 tolerated):
- `## Goal` (alias `# Goal`) → first non-empty paragraph after the heading, joined into a single `Option<String>`.
- `## Steps` → markdown checklist lines `- [ ]` / `- [x]` (also `* ` / `+ `); each becomes a `PlanStep { text, done }`.
- `## Evidence / Verification` (aliases `## Evidence`, `## Verification`) → bullet lines (`- text` not starting with `- [`) and paragraph blocks are each pushed as a `String` into `evidence: Vec<String>`.
- `## Open Questions / Risks` (aliases `## Open Questions`, `## Risks`) → same shape as Evidence.
- `## Next Action` → first non-empty paragraph OR the first checklist item (preserved as `"[ ] text"` / `"[x] text"`) into `Option<String>`.
Unknown headings do not reset the active section (their content is ignored unless it lands under a known section); the documented "first paragraph under `## Goal`" rule is preserved by only setting `goal` when it is still `None`.

No markdown crate was needed (the file contract is stable section headings only). `rg "pulldown|markdown|comrak" src-tauri/Cargo.toml` returned nothing — no existing parser to reuse.

### 2. File-watch viability
Tauri exposes `tauri-plugin-fs` with a `watch` API and the broader event system (`app.listen`, `app.emit`). Adding live updates later is cheap: subscribe to `.gospel/PLAN.md` (or `.gospel/`) via the fs watcher on workspace activation, re-run `parse_plan_markdown`, and emit a `plan-updated` event the `PlanPanel` listens to. The read-only command shape stays the same. **Deferred** — the spike's Refresh button + re-read-on-workspace-change is enough to validate the seam.

### 3. Open Question A: read-only mirror vs. editor in panel (maintainer's call)
- **Read-only mirror (spike's current shape)**: zero write-path concurrency concerns; the agent stays the sole writer via `write_harness_file`; the panel is a pure inspection surface. Matches PRODUCT.md "Show, don't tell." with no new write races.
- **Editor in panel**: lets the dev correct/tick steps without leaving the app; risks racing the agent's `write_harness_file` (last-writer-wins clobbers the other's edit). Needs an explicit write API + conflict policy (the spike's STOP condition on write races is real — recorded, not solved here).
- **Hybrid (recommended for the build plan)**: read-only mirror + a "tick this step" affordance that funnels through the same `write_harness_file` contract (atomic rewrites, agent's write path reused), so there is one writer and the panel never edits prose directly.
- A separate follow-up plan MUST resolve this before productionizing; the spike intentionally does not.

### 4. Open Question B: panel placement
- **Right-side overlay** (DESIGN.md §4 Diff/Code Review Panel, §5 File Context Panel) — the panel slides in from the right, same pattern as the existing `ReviewPanel`. The spike already uses this placement. **Recommended.** Matches "Same patterns, same places." (PRODUCT.md) and keeps the chat column primary.
- **Constellation tab** — the plan becomes another node/cluster in the constellation. Better for spatial thinkers, but the plan is a long-form document, not a graph node; this would fight the constellation's strength and duplicate the existing `write_harness_file` → "plan" CanvasToolNode mapping (`src/hooks/useConstellation.ts:65,95`), which is orthogonal and out of scope.
- **Recommendation**: right-side overlay for the build plan. Constellation integration stays as the existing single-dot node; a richer plan node is a separate, orthogonal plan.

### 5. Files a future build plan would touch
- `src-tauri/src/harness_plan.rs` — extend parser if the maintainer canonicalises heading forms further; add serde versioning if PLAN.md gets a structured front-matter block.
- `src-tauri/src/lib.rs` — keep `read_harness_plan`; add a `write_harness_plan_step` (or similar) command if Open Question A lands on the hybrid; register it in `generate_handler!`.
- `src/components/PlanPanel.tsx` — graduate from debug-gated preview to a default overlay; wire a trigger (button in TopBar / CommandPalette entry) and a close interaction; add file-watch subscription for live updates; add the "tick step" affordance if hybrid.
- `src/App.tsx` — remove the `?panel=plan` debug gate and wire `PlanPanel` into `AppShell` (or `WorkbenchLayout`) as a real overlay.
- `src/types/index.ts` — extend `PlanFile` if new fields are added; add a write-request type if editing.
- `src/hooks/useWorkspaces.ts` (or a new `usePlan.ts`) — extract plan-fetching into a hook with file-watch subscription.
- `src-tauri/src/workspace_tools.rs` — possibly relax the `source_edit` block on `.gospel/` if the build plan allows the user to edit prose (currently `workspace_tools.rs:2138` blocks `.gospel/` from `source_edit`; the build plan must decide whether user-side edits go through `write_harness_file` or a new user-scoped write path).
- No DB / persistence migration: PLAN.md stays plain UTF-8 markdown.

## Maintenance notes
- The spike leaves two artifacts (a backend command + a debug-gated FE panel). A subsequent "Plan Panel Build" plan MUST resolve Open Questions A and B before productionizing.
- Reviewer: confirm the debug gate is OFF by default (`isPlanPanelRequest` returns `false` in `PROD` and when `?panel=plan` is absent); confirm the parser tests pin the documented contract.
- Follow-up deferred (out of spike scope): (1) file-watch live updates; (2) edit-from-UI; (3) productionizing the panel; (4) Constellation node upgrade.