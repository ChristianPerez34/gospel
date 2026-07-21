# Plan 015: Delete dead/duplicate components and their misleading tests

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 72819cd..HEAD -- src/components/ReviewPanel.tsx src/components/ReviewPanel.test.tsx src/components/ReviewProgressView.tsx src/components/TitleBar.tsx src/components/ContextPill.tsx src/components/WorkspaceStage.tsx src/components/WorkbenchLayout.tsx src/components/SessionDrawer.tsx`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none (lands cleanly alongside plans 011–014)
- **Category**: tech-debt
- **Planned at**: commit `72819cd`, 2026-07-20

## Why this matters

Five components have **zero non-test importers** — they were replaced by
`WorkbenchLayout` (production code says so explicitly in its comments), but
the old components and their tests were left behind. `bun run test` reports
green on a suite exercising components the live app never renders, which
masquerades as coverage on the review surface. About 1,000 lines of dead
code inflates the bundle and the review surface.

## Current state

Dead components with self-references only — confirmed by `rg`:

- `src/components/ReviewPanel.tsx` — `rg "ReviewPanel\b" src` returns only its
  own definition and `src/components/ReviewPanel.test.tsx`.
- `src/components/ReviewPanel.test.tsx` — tests a component the live app
  never renders.
- `src/components/ReviewProgressView.tsx` — imported only by `ReviewPanel.tsx`
  (per the subagent audit; verify with `rg "ReviewProgressView" src`).
- `src/components/TitleBar.tsx` — `rg "TitleBar" src` returns only its own
  definition (no importers). Window chrome with no `onClick`/`onClose`
  handlers (see `TitleBar.tsx:20-54`); appears to be a pre-app-shell
  scaffold.
- `src/components/ContextPill.tsx` — `rg "ContextPill" src` returns only its
  own definition. DESIGN.md §3 mandates context pills above InputBar, but
  none is wired. (Note: revival is a separate direction spike — D2 in the
  audit table. Deleting now is safe; a future plan can re-create it.)
- `src/components/WorkspaceStage.tsx` — `rg "WorkspaceStage" src` returns
  only its own definition.

Replacement comments in production code (the executor's confirmation that
this is intentional):
- `src/components/WorkbenchLayout.tsx:61` —
  `"Review trigger state (extracted from ReviewPanel)"` (verbatim).
- `src/components/WorkbenchLayout.tsx:110` —
  `"Review trigger handlers (extracted from ReviewPanel)"` (verbatim).

Singular-action props in `SessionDrawer.tsx` (dead branches):
- `src/components/SessionDrawer.tsx:25-27` declares
  `onArchiveSession`/`onRestoreSession`/`onDeleteArchivedSession` in
  singular form (the plural forms `onArchiveSessions` etc. are what AppShell
  actually passes — see `AppShell.tsx:850-867`).
- `src/components/SessionDrawer.tsx:522-588` has `if (onRestoreSession) ...`
  blocks that are dead because AppShell never passes the singular form.

No design-doc or ADR preserves these. The repo's last commit message style is
`chore: remove prototype artifacts` (commit 8c6e3ad in `git log`) — this
deletion is in the same spirit.

## Commands you will need

| Purpose   | Command                                                                  | Expected on success |
|-----------|--------------------------------------------------------------------------|---------------------|
| Frontend tests | `bun run test`                                                       | all pass (count drops by ReviewPanel.test.tsx's test count) |
| Typecheck | `bun run typecheck`                                                       | exit 0             |
| Lint      | `bun run lint`                                                            | exit 0 (warnings ok; also unused-import warnings may shrink) |

## Scope

**In scope** (delete):
- `src/components/ReviewPanel.tsx`
- `src/components/ReviewPanel.test.tsx`
- `src/components/ReviewProgressView.tsx`
- `src/components/TitleBar.tsx`
- `src/components/ContextPill.tsx`
- `src/components/WorkspaceStage.tsx`

**In scope** (edit):
- `src/components/SessionDrawer.tsx` — remove the singular-action props
  (`onArchiveSession`, `onRestoreSession`, `onDeleteArchivedSession`) and
  the dead `if (onRestoreSession)` / `if (onDeleteArchivedSession)` blocks

**Out of scope**:
- Any production code that imports one of the deleted components (there is
  none): if `rg` returns a non-test importer during Step 1, STOP.
- The `useConstellation` reviewer-node mapping (lives; not dead).
- Reimplementing ContextPill later (see direction item D2 — separate plan).
- Removing review fixtures the deleted tests may have exercised transitively
  — they're imported by the deleted test only.

## Git workflow

- Branch: `advisor/015-delete-dead-prototype-components`
- Single commit is fine; example: `chore: remove dead ReviewPanel/ReviewProgressView/TitleBar/ContextPill/WorkspaceStage components`.
- Do NOT push unless instructed.

## Steps

### Step 1: Confirm zero non-test importers for each component

Search only import statements and component definitions, and STOP if any
returns an unexpected match (a live production importer would mean the
component is NOT dead):

```
rg '^\s*import\b.*\b(ReviewPanel|ReviewProgressView|TitleBar|ContextPill|WorkspaceStage)\b|^\s*export\s+(default\s+)?(function|class|const)\s+(ReviewPanel|ReviewProgressView|TitleBar|ContextPill|WorkspaceStage)\b' src
```

Before deletion, matches should be limited to each component's definition and
the known component/test imports. The intentional WorkbenchLayout comments
containing `ReviewPanel` are not imports or definitions and must not fail this
check. If any production importer is returned, STOP and report.

### Step 2: Delete the five component files + the ReviewPanel test

```
git rm src/components/ReviewPanel.tsx
git rm src/components/ReviewPanel.test.tsx
git rm src/components/ReviewProgressView.tsx
git rm src/components/TitleBar.tsx
git rm src/components/ContextPill.tsx
git rm src/components/WorkspaceStage.tsx
```

**Verify**: `bun run typecheck` → exit 0. If typecheck fails, an importer
remains — STOP and report (re-run `rg` with the broken import symbol).

### Step 3: Strip dead singular-action props from SessionDrawer

In `src/components/SessionDrawer.tsx`:

1. Read lines 25-27 (interface / props declaration). Remove
   `onArchiveSession`, `onRestoreSession`, `onDeleteArchivedSession` (the
   singular forms). Keep `onArchiveSessions`, `onRestoreArchivedSessions`,
   `onDeleteArchivedSessions` (plural — AppShell passes these).
2. Read lines 522-588. Remove any `if (onRestoreSession) { ... }` and
   `if (onDeleteArchivedSession) { ... }` blocks that are gated on the
   removed singular props.
3. If the singular forms are wired into other props (e.g. passed down to
   a child component), remove those wirings too. Search
   `rg "onArchiveSession|onRestoreSession|onDeleteArchivedSession" src` —
   every match must be on the deleted test/component paths or this file.

**Verify**: `bun run typecheck` → exit 0. `bun run test -- src/components/SessionDrawer.test.tsx`
→ all pass. `rg "onArchiveSession\b|onRestoreSession\b|onDeleteArchivedSession\b" src`
→ no matches.

### Step 4: Verify the whole project still builds and tests pass

Run the full gate that `package.json`'s `check` script runs:

```
bun run typecheck && biome check . && bun run clippy
```

Per `AGENTS.md`: use `bun run test` (Vitest), not `bun test` (Bun runner).

**Verify**: `bun run test` → all pass. Confirm the count dropped by however
many `ReviewPanel.test.tsx` had (note the before/after counts in the commit
message). `bun run lint` → exit 0 with a possibly-reduced warning count.

## Test plan

No new tests. The deletion itself is the deliverable. Existing tests (116)
minus `ReviewPanel.test.tsx`'s 4 tests must still pass; if AppShell tests
or other component tests referenced the deleted singular-action props
indirectly (they shouldn't, since AppShell passed the plural), they will
surface a typecheck error caught at Step 3's verification.

## Done criteria

- [ ] `ls src/components/ReviewPanel.tsx src/components/ReviewPanel.test.tsx src/components/ReviewProgressView.tsx src/components/TitleBar.tsx src/components/ContextPill.tsx src/components/WorkspaceStage.tsx 2>&1` reports all as missing
- [ ] The import/definition-only `rg` command from Step 1 returns zero matches after deletion; intentional WorkbenchLayout comments containing `ReviewPanel` may remain
- [ ] `rg "onArchiveSession\b|onRestoreSession\b|onDeleteArchivedSession\b" src` returns zero matches
- [ ] `bun run typecheck` exits 0
- [ ] `bun run test` exits 0 with the test count reduced by ReviewPanel.test.tsx's test count (record before/after in commit body)
- [ ] `bun run lint` exits 0 (lint warnings should drop, not rise)
- [ ] No files outside the in-scope list are modified (`git status` — only the six deletions and the SessionDrawer edit)
- [ ] `plans/README.md` status row for plan 015 updated

## STOP conditions

Stop and report back if:

- Step 1 (`rg`) returns a non-test production importer for any deleted
  component — the finding is wrong; do not delete.
- Step 2 typecheck fails — there IS an importer somewhere; restore the
  deleted file and report.
- `SessionDrawer.tsx:25-27` does not contain the singular-action props — the
  drift check fired; re-read the file and reconsider.
- An existing `SessionDrawer.test.tsx` test asserts the singular-action prop
  path — update the test to drop the dead assertion, but if the test encodes
  intended behavior that should be preserved, STOP and report (the test
  may be evidence the singular form is the intended future API, not dead code).
- The deletion changes the visible bundle or layout in a running app (smoke
  check by running `bun run dev` and visually verifying Chat View renders,
  the session drawer opens, and workspace switching works); any visual
  regression is a STOP.

## Maintenance notes

- A future plan reviving `ContextPill` (DESIGN.md §3) should re-create it
  with current conventions, not un-delete. The deletion here is safe to
  "rebuild from scratch" because the original had no callers.
- Reviewer should confirm: only 7 files touched (6 deletions + 1 edit), the
  commit body lists the test count delta, and `bun run dev` visual smoke
  shows no regression on the session drawer / workspace switcher / chat view.
