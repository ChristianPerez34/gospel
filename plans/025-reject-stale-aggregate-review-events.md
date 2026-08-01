# Plan 025: Reject Stale Aggregate Review Events

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer dispatched you and told you they maintain
> the index.
>
> **Drift check (run first)**: `git diff 0ec1edd..HEAD -- src/hooks/useReviewProgress.ts src/hooks/useReviewProgress.test.tsx`
> If the reducer or test fixture shape differs from the excerpts, stop and
> report the drift before editing.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: tests
- **Planned at**: commit `0ec1edd`, 2026-07-31

## Why this matters

Review progress is keyed by `run_id` because multiple review runs can overlap
or emit late events during cleanup. Focused events already reject a different
run ID once a run is active, but aggregate events omit `focus` and bypass that
guard. A late aggregate start or terminal event from an older run can therefore
replace the current run state and show incorrect progress or completion. The
fix is a one-condition state-machine correction plus regression tests.

## Current state

Relevant files:

- `src/hooks/useReviewProgress.ts` — Tauri event listener and state reducer
  boundary.
- `src/hooks/useReviewProgress.test.tsx` — Vitest hook tests and event helper.

The listener currently guards only focused events:

```ts
// src/hooks/useReviewProgress.ts:368-390
const isAggregate = focus == null;
if (!isAggregate && prev.runId !== null && prev.runId !== payload.run_id) {
  return prev;
}
const runChanged =
  prev.runId === null || (isAggregate && prev.runId !== payload.run_id);
```

That means an aggregate event from another run sets `runChanged` and replaces
the active state. The existing regression test covers a stale focused terminal
event only:

```ts
// src/hooks/useReviewProgress.test.tsx:383-410
it("ignores focused events from a different active run", async () => {
  emitProgress(/* current */, "Security", "current-run");
  emitProgress({ type: "done", findings: 9, suppressed: 0 }, "Security", "stale-run");
  expect(result.current.runId).toBe("current-run");
  expect(result.current.done).toBe(false);
});
```

The first event after a reset must still establish a new run, regardless of
whether it is aggregate or focused. Do not reject all aggregate events.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Focused hook tests | `bun run test -- src/hooks/useReviewProgress.test.tsx` | all review-progress tests pass |
| Frontend full tests | `bun run test` | all frontend tests pass |
| Typecheck | `bun run typecheck` | exit 0, no errors |
| Lint | `bun run lint` | exit 0; existing warnings may remain, no new errors |

## Scope

**In scope** (the only files to modify):

- `src/hooks/useReviewProgress.ts` — reject mismatched run IDs whenever a run
  is already active and simplify `runChanged` to the reset/first-event case.
- `src/hooks/useReviewProgress.test.tsx` — add aggregate stale-event tests.

**Out of scope** (do not touch):

- Backend review event serialization or `run_id` generation.
- `useChatStream` event handling; it has a separate run guard and lifecycle.
- Review layout, progress copy, or reducer phase semantics unrelated to run
  ownership.

## Git workflow

- Branch: `advisor/025-reject-stale-aggregate-review-events`
- Commit style example: `fix(review): ignore stale aggregate progress events`.
- Do not push or open a PR unless explicitly instructed.

## Steps

### Step 1: Make run ownership independent of focus

Change the listener state update so that, when `prev.runId !== null`, any
payload with a different `payload.run_id` returns `prev`, regardless of whether
`payload.focus` is present. Preserve the existing invalid-payload early return
and preserve acceptance of the first valid event after `reset()`.

The intended state transition is:

- `prev.runId === null`: accept the first valid event and initialize its run.
- `prev.runId === payload.run_id`: reduce the event normally, aggregate or
  focused.
- `prev.runId !== null && prev.runId !== payload.run_id`: ignore the event.

Do not use omitted `focus` as a signal that the event is newer. `run_id` is the
authority.

**Verify**: `bun run typecheck` -> exit 0; `bun run test -- src/hooks/useReviewProgress.test.tsx` -> existing tests pass.

### Step 2: Add aggregate start and terminal regressions

Add tests in the existing `useReviewProgress` suite that:

1. Start a current run with a valid focused event.
2. Emit a stale aggregate `multiFocusStart` event with a different run ID and
   assert the current run ID, pipeline, and activity log remain unchanged.
3. Emit a stale aggregate terminal event (`done` or the aggregate completion
   phase used by the fixture) and assert the current run is not marked done or
   failed and the stale event is not appended to the log.
4. Separately verify that after calling `reset()`, the first aggregate event of
   a new run is accepted. This prevents over-tightening the guard.

Use the existing `emitProgress` helper and phase types at the top of
`src/hooks/useReviewProgress.test.tsx`; do not add a second event harness.

**Verify**: `bun run test -- src/hooks/useReviewProgress.test.tsx` -> all focused tests pass, including the new aggregate cases.

### Step 3: Run the full frontend gates

Review the final listener branch for all event shapes, including events with
and without `focus`. Confirm `reset()` still clears `runId` and all progress
state.

**Verify**: `bun run test` -> all tests pass; `bun run typecheck` -> exit 0; `bun run lint` -> exit 0 with no newly introduced diagnostics.

## Test plan

- Extend `src/hooks/useReviewProgress.test.tsx` with stale aggregate start,
  stale aggregate terminal, and post-reset aggregate acceptance cases.
- Preserve the existing focused stale-event test as coverage for the symmetric
  focused path.
- Assert state and log ownership, not only that a callback did not throw.
- Verification: `bun run test` -> all frontend tests pass.

## Done criteria

- [ ] Any mismatched run ID is ignored once `useReviewProgress` has an active
      run, whether or not the event has a focus.
- [ ] The first valid event after reset initializes the new run.
- [ ] Aggregate stale start and terminal regressions are covered by tests.
- [ ] `bun run test` exits 0.
- [ ] `bun run typecheck` exits 0.
- [ ] `bun run lint` exits 0 with no new diagnostics.
- [ ] Only the two in-scope files are modified.
- [ ] `plans/README.md` status row for plan 025 is updated.

## STOP conditions

Stop and report instead of improvising if:

- The backend uses a different event identity field or can legitimately reuse
  a run ID for a new review.
- `reset()` is no longer the only path that clears `runId`.
- Aggregate events are intentionally allowed to supersede focused events and a
  product/ADR document proves that policy.
- The aggregate terminal phase has changed and cannot be identified from the
  existing fixture/types without changing backend code.

## Maintenance notes

- Keep run ownership checks before phase reduction. A reducer should never see
  a stale event from another run.
- When adding a new aggregate event, include it in the stale-run test matrix.
- The frontend listener has no durable event replay; if replay or resumable
  review history is added, the run identity contract must be revisited.
