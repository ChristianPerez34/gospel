# Plan 026: Coalesce Streamed Text Updates Without Reordering Blocks

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer dispatched you and told you they maintain
> the index.
>
> **Drift check (run first)**: `git diff --stat 0ec1edd..HEAD -- src/hooks/useChatStream.ts src/hooks/useChatStream.test.ts`
> If the stream listener, current-turn model, or test harness differs from the
> excerpts, stop and report the drift before editing.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none; preserve the characterization coverage from plan 012
- **Category**: perf
- **Planned at**: commit `0ec1edd`, 2026-07-31

## Why this matters

The backend emits each streamed text item as an individual `llm-token` event.
The frontend currently copies the entire block array and publishes a React
state update for every token. Long responses therefore create a high-frequency
render/allocation loop even though text can be flushed at frame cadence. The
optimization must not change visible text, block ordering, tool-result pairing,
reasoning behavior, cancellation, or finalization. This plan adds a buffered
text path with explicit flush points around every non-text event.

## Current state

Relevant files:

- `src/hooks/useChatStream.ts` — current-turn ref/state, event listeners, stream
  finalization, cancellation, and reset.
- `src/hooks/useChatStream.test.ts` — event harness and 14 existing stream
  characterization tests.
- `src/types/index.ts` — `CurrentTurn` and `TurnBlock` shapes consumed by the
  hook.
- `src-tauri/src/llm.rs` — backend emits a separate `StreamEvent::Text` for
  each streamed text item at lines 737-745; do not change the backend in this
  plan.

The current state update writes each token immediately:

```ts
// src/hooks/useChatStream.ts:130-137 and 173-195
const updateCurrentTurn = useCallback((updater) => {
  const existing = currentTurnRef.current ?? createTurn();
  const next = updater(existing);
  currentTurnRef.current = next;
  setCurrentTurn(next);
  return next;
}, [createTurn]);

updateCurrentTurn((turn) => {
  const blocks = [...turn.blocks];
  const last = blocks[blocks.length - 1];
  ...
  blocks[blocks.length - 1] = { ...last, text: last.text + token };
  return { ...turn, blocks };
});
```

The backend loop emits each text item independently:

```rust
// src-tauri/src/llm.rs:737-745
full_response.push_str(&text.text);
on_event(StreamEvent::Text(text.text.clone()));
```

Non-text handlers currently read `currentTurnRef.current` or append/update
blocks directly. Finalization reads the ref at

The test harness triggers listeners directly in
`src/hooks/useChatStream.test.ts:18-23`; existing token tests expect current
turn state after an `act` block at lines 81-110. Any scheduler introduced by
this plan must be made deterministic in that harness.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Stream tests | `bun run test -- src/hooks/useChatStream.test.ts` | all stream tests pass |
| Frontend full tests | `bun run test` | all frontend tests pass |
| Typecheck | `bun run typecheck` | exit 0, no errors |
| Lint | `bun run lint` | exit 0; existing warnings may remain, no new errors |

## Scope

**In scope** (the only files to modify):

- `src/hooks/useChatStream.ts` — buffered text ref, frame scheduler, flush
  points, and cleanup.
- `src/hooks/useChatStream.test.ts` — deterministic scheduler setup and
  regression/performance-shape tests.

**Out of scope** (do not touch):

- `src-tauri/src/llm.rs` or any backend event protocol.
- `src/types/index.ts` or the persisted message shape.
- Changes to token text, backend response authority, reasoning filtering,
  approval semantics, workspace switching, or cancellation policy.
- Global React memoization or unrelated component render optimization.

## Git workflow

- Branch: `advisor/026-coalesce-streamed-text-updates`
- Commit style example: `perf: coalesce streamed text state updates`.
- Do not push or open a PR unless explicitly instructed.

## Steps

### Step 1: Add a buffered text scheduler with explicit flush semantics

Add refs for pending text and the scheduled frame/handle. On an accepted
`llm-token` event, append the token to the pending buffer and schedule at most
one animation-frame flush. The flush must append the whole buffer to the last
text block or create one text block when needed, using the existing
`updateCurrentTurn`/`currentTurnRef` pattern.

Add a synchronous `flushPendingText` helper that:

- Cancels any scheduled frame before applying the buffer.
- Does nothing when the buffer is empty.
- Clears the buffer before or atomically with the update so a reentrant event
  cannot duplicate text.
- Preserves the existing turn ID and text-block occurrence order.

Use the browser frame scheduler already available to the webview (with a small
test-safe fallback only if the environment lacks it). Do not use an unbounded
interval or schedule one timer per token.

**Verify**: `bun run typecheck` -> exit 0; `bun run test -- src/hooks/useChatStream.test.ts` -> existing tests compile and pass after the test scheduler is adjusted as needed.

### Step 2: Flush before every ordering-sensitive event and lifecycle end

Call `flushPendingText` before handlers that can append or mutate blocks:

- `llm-tool-call`
- `llm-tool-result`
- `llm-reasoning`
- `approval-requested`
- `approval-resolved`
- `llm-done`
- `llm-error`
- local `cancelStream`

Do not flush stale events. Keep the existing `isStale` checks before any buffer
mutation. Ensure `clearCurrentTurn` cancels a pending frame and clears pending
text so reset, unmount, cancellation, and completion cannot leak text into a
later run. `llm-done` must flush before it captures `currentTurnRef.current`;
the backend response remains authoritative for final `content` exactly as it is
now.

Do not change corpus toast or model-variant warning behavior, which does not
participate in text-block ordering.

**Verify**: `bun run test -- src/hooks/useChatStream.test.ts` -> tool/text,
reasoning/text, done, error, cancellation, and reset tests pass.

### Step 3: Make scheduler behavior deterministic and prove coalescing

Extend the existing test harness with a controllable frame queue or equivalent
test-only scheduler. Do not rely on wall-clock sleeps. Add tests that:

- Multiple accepted token events before one frame produce one visible text
  update and the concatenated text in original order.
- A tool call arriving after buffered text flushes text before the tool block.
- `llm-done`, `llm-error`, and cancellation flush buffered text even when the
  frame callback has not run.
- A stale token does not enter the pending buffer.
- Reset/unmount cancels pending work and a later run starts with an empty buffer.
- The existing authoritative done-response, reasoning stripping, approval,
  tool-result pairing, and per-run isolation assertions still pass.

If the test harness cannot observe render/update counts without changing the
public hook API, assert the scheduler's single flush/update behavior through
the captured `currentTurn` and `onMessages` callbacks instead of exporting an
internal helper.

**Verify**: `bun run test -- src/hooks/useChatStream.test.ts` -> all focused
tests pass, including new buffering and flush-order cases.

### Step 4: Run the full frontend gates

Review the final diff for every path that reads `currentTurnRef.current`. There
must be no finalization/cancellation path that reads the ref before flushing
pending text. Confirm the frame handle cannot survive unmount or reset.

**Verify**: `bun run test` -> all tests pass; `bun run typecheck` -> exit 0; `bun run lint` -> exit 0 with no new diagnostics.

## Test plan

- Extend `src/hooks/useChatStream.test.ts` using its existing mocked `listen`,
  `invoke`, `triggerEvent`, and `renderHook` helpers.
- Add deterministic frame-queue tests for coalescing, ordering, finalization,
  cancellation, stale events, and reset/unmount cleanup.
- Preserve all existing characterization tests from plan 012 and the later
  cancellation/isolation tests from plan 014.
- Verification: `bun run test` -> all frontend tests pass.

## Done criteria

- [ ] Accepted token events no longer call `setCurrentTurn` once per token;
      multiple tokens in one frame produce one flush.
- [ ] Text is flushed before all ordering-sensitive non-token events and before
      done/error/cancel/reset cleanup.
- [ ] Existing visible text, block order, tool pairing, reasoning stripping,
      cancellation, and stale-run behavior remain covered and passing.
- [ ] `bun run test` exits 0.
- [ ] `bun run typecheck` exits 0.
- [ ] `bun run lint` exits 0 with no new diagnostics.
- [ ] Only `useChatStream.ts` and `useChatStream.test.ts` are modified.
- [ ] `plans/README.md` status row for plan 026 is updated.

## STOP conditions

Stop and report instead of improvising if:

- The current-turn or event model has changed so a buffered text block cannot
  preserve occurrence order without changing `src/types/index.ts`.
- The runtime webview lacks both `requestAnimationFrame` and a safe scheduling
  primitive; do not invent a global timer service in this plan.
- Existing tests or consumers require a synchronous state update after every
  individual token as a public contract.
- A flush-order fix requires changing backend event emission or persisted DTOs.
- A test fails only because it depends on real wall-clock timing; replace the
  timing assumption with a deterministic scheduler or stop and report it.

## Maintenance notes

- Any new event that appends or mutates `CurrentTurn.blocks` must flush pending
  text first, or it can reorder the visible timeline.
- Reviewers should inspect every finalization and cleanup path, not only the
  token listener; most buffering regressions lose text during cancel/error.
- If profiling later shows tool/result events dominate render cost, address
  those updates separately rather than increasing the text frame window.
