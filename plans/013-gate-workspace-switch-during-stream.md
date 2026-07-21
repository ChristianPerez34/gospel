# Plan 013: Gate workspace switcher + TopBar switch button while streaming

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 72819cd..HEAD -- src/components/AppShell.tsx src/components/TopBar.tsx src/components/WorkspaceSwitcher.tsx`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: plans/012-stream-characterization-tests.md
- **Category**: bug
- **Planned at**: commit `72819cd`, 2026-07-20

## Why this matters

Switching workspaces during a stream leaves the old session's live tokens
rendered in the new workspace's chat column until the turn completes, then
jarring snap-to-empty. The cross-workspace visual leak happens because the
workspace switcher and the TopBar workspace button have **no** `isStreaming`
guard, while the SessionDrawer and CommandPalette session actions do block
during streaming (the codebase already converged on the guard pattern — this
plan completes it for the two surfaces that were missed).

The clean fix is the stop-gate (consistent with sibling surfaces). A
"continue streaming in background" UX is a larger product decision deferred to
a follow-up; for now the same gate that protects SessionDrawer and
CommandPalette protects workspace switching.

This plan depends on plan 012 (streaming characterization tests) — the late-
event / state-leak paths exercised there give the regression net for this
refactor.

## Current state

- `src/components/AppShell.tsx:836-910` — the surfaces to fix:
  - **`SessionDrawer` onSelect (line 846)** is the canonical guard:
    ```tsx
    onSelect={(s) => {
      if (session.isStreaming) return;
      void session.handleSessionSelect(s);
      closeSessionDrawer();
    }}
    ```
  - **`CommandPalette` onSelectSession (line 941)** and **onNewSession
    (line 945)** use the same guard.
  - **`WorkspaceSwitcher` onSelect (lines 883-885)** does NOT:
    ```tsx
    onSelect={(ws) => {
      void switchWorkspace(ws.id);   // no isStreaming guard
    }}
    ```
    There's no equivalent `if (session.isStreaming) return;` and no early-out
    toast/disabled row; `archiveActionsDisabled={session.isStreaming}` is
    already set for SessionDrawer (line 868) but not for the switcher.
- `src/components/TopBar.tsx:61-...` — renders the workspace button. The
  `onWorkspaceSwitch` prop (declared at lines 6–18 as `() => void`) is wired
  into a clickable element somewhere in the topbar primary area; it is
  **always reachable**. There is no disabled state based on
  `session.isStreaming`. The `status` prop is passed in (search for `status`).
- `src/components/WorkspaceSwitcher.tsx` — the dropdown itself. Each workspace
  row already supports an active state (`activeWorkspaceId` is highlighted
  with `--accent-action` left border per DESIGN.md §3). It has no
  `disabled`/`onSwitchBlocked` prop; **do not** add a per-row disabled flag —
  the cleanest fix is to block the toggle that opens the switcher.
- `src/hooks/useSessionManager.ts:96-116` — has a `statusRef` gates the
  workspace reset on the next "connected" tick; out of scope for this plan
  (the leak only occurs because the switcher/TopBar are clickable mid-stream;
  blocking them at the source closes the user-visible bug).
- DESIGN.md spec excerpts to honor (inline for the executor, who hasn't read the doc):
  - "Same patterns, same places. Consistency is an affordance." (DESIGN.md:33)
  - "Error states are inline and recoverable, never modal roadblocks." — the
    guard's feedback should not be a modal; a brief inline hint is fine if any.

No ADR covers this. The product "trust" principle (PRODUCT.md) means a
predictable block is preferred over silent partial behavior.

## Commands you will need

| Purpose   | Command                                                                  | Expected on success |
|-----------|--------------------------------------------------------------------------|---------------------|
| Typecheck | `bun run typecheck`                                                       | exit 0             |
| Frontend tests | `bun run test -- src/components/TopBar.test.tsx src/components/AppShell.test.tsx` | all pass |
| Full suite | `bun run test`                                                          | all pass           |
| Lint | `bun run lint`                                                              | exit 0 (warnings ok) |

## Scope

**In scope**:
- `src/components/AppShell.tsx` (gate `WorkspaceSwitcher` open + select during streaming)
- `src/components/TopBar.tsx` (disable the workspace-switch button during streaming)
- `src/components/AppShell.test.tsx` (exercise AppShell's guarded WorkspaceSwitcher selection while streaming)
- `src/components/TopBar.test.tsx` (assert the workspace-switch button is disabled while streaming)

**Out of scope**:
- Building a "finish turn in background" UX
- Any backend cancel-stream work (that's plan 014)
- `useChatStream.ts` (covered by plan 012)
- Changes to SessionDrawer / CommandPalette (they're already guarded)
- Disabled styling of individual WorkspaceSwitcher rows

## Git workflow

- Branch: `advisor/013-gate-workspace-switch-during-stream`
- Commit per step. Match the repo's `fix:` convention — example:
  `fix: block workspace switcher and TopBar switch button during streaming`.
- Do NOT push unless instructed.

## Steps

### Step 1: Gate the WorkspaceSwitcher open and select handlers

In `src/components/AppShell.tsx`:

1. Add a streaming guard to the WorkspaceSwitcher open trigger. Find where
   `setWorkspaceSwitcherOpen(true)` is called from the TopBar
   (`onWorkspaceSwitch`); the more consistent approach is to also keep the
   switcher from opening when streaming. Wrap the open call (wherever
   `setWorkspaceSwitcherOpen(true)` is invoked in response to the TopBar
   button — read the AppShell around `WorkspaceSwitcher`/`onWorkspaceSwitch`)
   so it does nothing when `session.isStreaming` is true. If you can't find a
   single open site, fall back to gating the `onSelect` of the switcher (Step
   1.2) and the TopBar button (Step 2), which together close the leak; do
   BOTH regardless to keep the consistency contract.
2. Gate the switcher's `onSelect`:
   ```tsx
   onSelect={(ws) => {
     if (session.isStreaming) return;
     void switchWorkspace(ws.id);
   }}
   ```
   Mirror the SessionDrawer onSelect pattern exactly (the `if
   (session.isStreaming) return;` form, no toast, no extra branching).
3. If the switcher has an `trapPaused`/`loading`/etc. prop that controls
   pointer interactivity, do NOT add a fake disabled prop — the open+select
   gates are sufficient and consistent with the SessionDrawer approach
   (which uses the gate, not a disabled flag).

**Verify**: `bun run typecheck` → exit 0. `bun run test` → full suite green
(no tests added yet, just confirming nothing regressed).

### Step 2: Disable the TopBar workspace-switch button during streaming

In `src/components/TopBar.tsx`:

1. The TopBar already receives `status: AgentStatus`. The file already
   computes `computeActive` in the body (near line 59):
   ```ts
   const computeActive = status === "thinking" || status === "acting";
   ```
   Reuse `computeActive` for the disabled state. Do not re-derive
   `isStreaming` from a separate prop — `status` is the canonical signal.
2. Find the workspace-switch button (read the `onWorkspaceSwitch` prop's
   attachment). Add `disabled={computeActive}` to the button. If the button
   is a `<button type="button">`, native `disabled` is sufficient. If it's a
   `<div onClick={...}>` (check first), convert to a `<button type="button">`
   so `disabled` works (this is also better for keyboard accessibility per
   DESIGN.md §Accessibility — keyboard-reachable). Add a `disabled` styling
   branch using the existing `text-text-muted` utility class for disabled
   state (the same token SessionDrawer uses for its disabled affordance).
3. Do NOT add a tooltip explaining why it's disabled (PRODUCT.md "no tooltips
   on first visit" + the disabled state is self-explanatory in the same way
   the SessionDrawer's archive actions are).

**Verify**: `bun run typecheck` → exit 0. `bun run test -- src/components/TopBar.test.tsx`
→ existing TopBar tests still pass (none should depend on the button being
enabled during `"thinking"`/`"acting"` states; if one does, stop and report
per STOP condition).

### Step 3: Add a regression test for AppShell's guarded switch path

In `src/components/AppShell.test.tsx`, use the existing `invoke`/`listen`
harness and render the real AppShell seam with two workspaces:

- Open the `WorkspaceSwitcher` before streaming starts, then start a turn and
  trigger `llm-token` so `currentTurn` belongs to the original session and
  `isStreaming` is true.
- Click the second workspace through the rendered WorkspaceSwitcher action.
  Do not call `useWorkspaces.switchWorkspace`, `setActiveWorkspaceId`, or a
  mocked equivalent directly; the test must execute AppShell's exact guarded
  `onSelect` callback.
- Assert that no `set_active_workspace` invoke occurs, the active session does
  not change, and the streamed `currentTurn` content remains rendered for the
  original session.

Also add a focused `TopBar.test.tsx` assertion that the workspace-switch
button is disabled when `isStreaming` is true.

**Verify**:
`bun run test -- src/components/TopBar.test.tsx src/components/AppShell.test.tsx`
→ both guarded-path tests pass.

## Test plan

- The AppShell test exercises the actual WorkspaceSwitcher selection callback
  and proves the active session/current turn stay attached to the original
  workspace while streaming.
- The TopBar test separately pins the disabled affordance.

## Done criteria

- [ ] `bun run typecheck` exits 0
- [ ] `bun run test` exits 0; the AppShell guarded-switch and TopBar disabled-state tests pass
- [ ] `rg "if \(session\.isStreaming\) return;" src/components/AppShell.tsx` returns at least 3 matches (SessionDrawer, CommandPalette existing + WorkspaceSwitcher new)
- [ ] `rg "disabled=\{computeActive\}" src/components/TopBar.tsx` returns 1 match on the workspace-switch button
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row for plan 013 updated

## STOP conditions

Stop and report back if:

- The TopBar workspace-switch control is not a `<button>` and converting it
  to one for the `disabled` attribute changes the visual layout in a way the
  plan can't anticipate (report; the fix may need a `<div aria-disabled=…>`
  fallback with a focus guard).
- An existing `TopBar.test.tsx` test asserts the workspace button is enabled
  during `"thinking"`/`"acting"` (would mean the test is closer to a spec;
  report and propose updating it).
- `switchWorkspace` is invoked from somewhere other than AppShell (e.g. a
  `useWorkspaces` hook owns the workspace lifecycle AppShell just calls into)
  — confirm during Step 1; the gate position is still AppShell, but the exact
  site of `setActiveWorkspaceId` matters for Step 3's test seam.
- Existing `useSessionManager.test.ts` already has a streaming lifecycle
  block that overlaps Step 3's tests — merge into it instead of duplicating.

## Maintenance notes

- When plan 014 (cancel stream) lands, the "block during streaming" guard
  can be reconsidered as "cancel the stream and then switch" — but the
  silent leak is closed by this plan alone.
- Reviewer should confirm the disabled TopBar button visually matches the
  disabled affordance used elsewhere (e.g. SessionDrawer archive buttons):
  same `text-text-muted`, same `cursor-not-allowed`.
- Follow-up deferred: per-row disabled indicator in WorkspaceSwitcher when
  a workspace is the streaming source; out of scope here for consistency with
  SessionDrawer's per-list-level guard.
