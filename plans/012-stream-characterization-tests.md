# Plan 012: Characterization tests for `useChatStream` (streaming foundation)

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 72819cd..HEAD -- src/hooks/useChatStream.ts src/hooks/useSessionManager.test.ts`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW
- **Depends on**: none
- **Category**: tests
- **Planned at**: commit `72819cd`, 2026-07-20

## Why this matters

`useChatStream` is the streaming core of the chat feature. It's 451 lines with
ten event listeners, two finalize handlers, a reasoning-block stripper, an
orphan-tool-result appender, and live approval lifecycle. It has **zero**
direct tests — it is only exercised indirectly through `useSessionManager.test.ts`.

This is the verification foundation for plans 013 (gate workspace switcher
during streaming) and 014 (cancel stream + per-run event isolation).
Refactoring listeners or finalize logic without a regression net will pass the
current suite while breaking the most async-hazard-heavy module in the repo.

These are characterization tests: they pin existing behavior so plans 013/014
can refactor safely. Where existing behavior is buggy (the late-event-after-reset
race that plan 014 fixes), the test should assert the *current* behavior with a
`TODO` comment pointing at plan 014, so the test changes meaningfully when 014
lands.

## Current state

- `src/hooks/useChatStream.ts` — the hook. Exports:
  ```ts
  export function useChatStream(options: UseChatStreamOptions = {}) {
    const currentTurnRef = useRef<CurrentTurn | null>(null);
    ...
    return { currentTurn, startStream, resetStream, resolveApproval };
  }
  ```
  Key surfaces (search by exact string in the file):
  - `llm-token` listener: builds `updateCurrentTurn` (lazy `createTurn()` if
    `currentTurnRef.current` is `null`)
  - `llm-done` listener: strips reasoning blocks, finalizes, calls optional
    `onTurnComplete`
  - `llm-error` listener: finalizes with error
  - `llm-tool-call`, `llm-tool-result` listeners; the latter warns and appends
    when `id` doesn't match a pending tool call (search `appending as completed`)
  - `approval-requested`, `approval-resolved` listeners
  - `startStream(opts)` invokes `complete_streaming` and returns void
  - `resetStream()` only nulls `currentTurnRef.current` (the leak plan 014 fixes)

- `src/hooks/useSessionManager.test.ts` — the **test harness pattern to mirror**.
  Captures `listen` callbacks via a `capturedListeners` map and `triggerEvent`
  helper:
  ```ts
  const capturedListeners: CapturedListeners = {};
  vi.mock("@tauri-apps/api/event", () => ({
    listen: (name: string, cb: ListenerCallback) => {
      capturedListeners[name] = capturedListeners[name]
        ? [...capturedListeners[name], cb]
        : [cb];
      return Promise.resolve(() => { /* unlisten */ });
    },
  }));
  ```
  (Read the actual mock in `useSessionManager.test.ts:1-60` and copy verbatim.)
  Test bodies drive `triggerEvent("llm-token", {...})` then assert on the
  result of the hook's `result.current`.
- Setup file: `src/test/setup.ts` — `vi.mock`/`vi.mocked` plumbing lives here.
- Vitest config: `vitest.config.ts` — read it to confirm `environment` (happy-dom)
  and setup file path before writing imports.
- No design-doc or ADR constraints apply to test internals.

## Commands you will need

| Purpose   | Command                                                                  | Expected on success |
|-----------|--------------------------------------------------------------------------|---------------------|
| Typecheck | `bun run typecheck`                                                       | exit 0             |
| Frontend tests | `bun run test -- src/hooks/useChatStream.test.ts`                 | tests pass         |
| Full suite | `bun run test`                                                          | all pass           |

## Scope

**In scope**:
- `src/hooks/useChatStream.test.ts` (create)

**Out of scope**:
- `src/hooks/useChatStream.ts` (no production code change; this plan is read-only characterization)
- The races in plan 014 — tests here characterize the current behavior with `TODO`
  markers; they will be updated (not deleted) when 014 lands
- Test utilities that already exist (reuse `capturedListeners` / `triggerEvent`
  by mirroring, not extracting, to keep the diff small)

## Git workflow

- Branch: `advisor/012-stream-characterization-tests`
- Commit per logical group (e.g. "test: characterize useChatStream token/done
  listeners", "test: characterize useChatStream approval lifecycle",
  "test: characterize useChatStream late-event-after-reset race").
- Do NOT push unless instructed.

## Steps

### Step 1: Set up the test file + harness

Create `src/hooks/useChatStream.test.ts`. Import and copy the `vi.mock`
harness from `src/hooks/useSessionManager.test.ts:1-60` for `@tauri-apps/api/event`
and `@tauri-apps/api/core`. The mocks to copy:
- `listen` → captures into `capturedListeners`, returns a noop unlisten
- `invoke` (from `@tauri-apps/api/core`) → returns a `vi.fn()` that resolves
  by command name (`complete_streaming` resolves `undefined`, etc.)

The hook under test depends on `UseChatStreamOptions`. The empty-default path
in `useChatStream(options = {})` is fine for most tests; supply per-test
callbacks for `onTurnComplete`, `onApprovalRequested`, etc.

Add a `beforeEach` that resets `capturedListeners` (same as the existing file)
and a `renderHook`-based helper that mounts `useChatStream` with default
options.

**Verify**: `bun run test -- src/hooks/useChatStream.test.ts` → exit 0
(no tests yet, but the file must compile and the harness must load).

### Step 2: Characterize the token → done happy path

Add a `describe("token and done listeners")` block:

1. `llm-token appends a token to currentTurn.blocks`
   - `triggerEvent("llm-token", "Hello")`
   - Assert `result.current.currentTurn` is non-null and its `blocks` contains
     the streamed content. Inspect the actual block shape by reading
     `createTurn` + `updateCurrentTurn` in `useChatStream.ts` first; the test
     must assert against the real shape.
2. `llm-token with no current turn lazily creates one`
   - Don't trigger `startStream`; directly `triggerEvent("llm-token", ...)`.
     Assert a turn was created (current behavior; the lazy-create is the
     legacy fallback).
3. `llm-done finalizes the turn, strips reasoning blocks, calls onTurnComplete`
   - Stream a token, then trigger `llm-done` with a payload that includes a
     reasoning block (search the real `llm-done` payload shape in
     `useChatStream.ts:155-187`).
   - Assert the finalized turn has no reasoning block.
   - Pass an `onTurnComplete: vi.fn()` and assert it was called with the
     correct finalized payload.
4. `llm-done with object payload prefers "response" when present`
   - Read the exact branch in `useChatStream:155-187` (it does
     `payload.response ?? payload.message` or similar — match the live
     branch).
5. `llm-error finalizes the turn with an error state`
   - Trigger `llm-error` with a message; assert `currentTurn` is in the
     error/finalized state per the hook's real shape.

**Verify**: `bun run test -- src/hooks/useChatStream.test.ts` → all 5 pass.

### Step 3: Characterize the orphan tool-result and approval lifecycle

Add a `describe("tool and approval lifecycle")` block:

6. `llm-tool-call adds a pending tool block; matching llm-tool-result resolves it`
   - Trigger `llm-tool-call` with an id; trigger `llm-tool-result` with the
     same id; assert the block transitions to a resolved state.
7. `llm-tool-result with an unknown id logs a warning and appends as completed`
   - Spy on `console.warn`; trigger `llm-tool-result` with an id not preceded
     by a tool call; assert `console.warn` was called (use the existing
     log-capture pattern from the repo or `vi.spyOn(console, "warn")`); assert
     the block was appended as completed.
8. `approval-requested emits the request; resolveApproval transitions the card`
   - Trigger `approval-requested`; assert `result.current` exposes an
     `approvalRequest` (or whatever the hook's state shape is — read and
     match). Call `result.current.resolveApproval(decision)` and verify the
     corresponding `approval-resolved` event would have been the equivalent
     manual trigger (i.e. the card transitions the same way as if
     `triggerEvent("approval-resolved", { id, decision })` had been fired).
9. `resolveApproval is idempotent when called twice`
   - Same setup; call `resolveApproval` twice with the same decision; assert
     no second invoke of `complete_streaming` or double-resolution of the
     card.

**Verify**: `bun run test -- src/hooks/useChatStream.test.ts` → all 9 pass.

### Step 4: Characterize listener lifecycle (mount/unmount + stale-event)

Add a `describe("listener lifecycle")` block:

10. `unmounting the hook calls each listener's unlisten`
    - Augment the captured-listener mock to record the unlisten's call: track
      a counter and assert it equals the number of registered listeners after
      `result.unmount()`.
11. `late llm-token after resetStream lazily re-creates a turn` (CHARACTERIZES
    the bug plan 014 fixes — leave as-is with a TODO marker)
    - `act(() => { result.current.resetStream(); })`
    - `triggerEvent("llm-token", { content: "stale" })`
    - **Assert**: `result.current.currentTurn` is non-null (this is the
      current buggy behavior — the stale event attaches to a fresh turn).
    - Add comment: `// TODO(plan 014): after per-run isolation lands, this
    //  test must change to assert the late event is ignored.`

Do NOT write a test that asserts the "correct" (post-014) behavior here —
that is plan 014's job. This plan's value is pinning the current state so 014
has a clear behavioral diff to switch on.

**Verify**: `bun run test -- src/hooks/useChatStream.test.ts` → all 11 pass.
Then `bun run test` → full suite green.

## Test plan

This plan *is* the test plan. The 11 tests above are the deliverable.
Pattern: `src/hooks/useSessionManager.test.ts` (the `capturedListeners` /
`triggerEvent` / `vi.mock` block at its top).

## Done criteria

- [ ] `bun run typecheck` exits 0
- [ ] `bun run test -- src/hooks/useChatStream.test.ts` exits 0 with at least 11 tests passing (3 describe blocks)
- [ ] `bun run test` (full suite) exits 0
- [ ] `src/hooks/useChatStream.ts` is unchanged (`git diff src/hooks/useChatStream.ts` is empty)
- [ ] `grep -n "TODO(plan 014)" src/hooks/useChatStream.test.ts` returns the late-event test marker
- [ ] `plans/README.md` status row for plan 012 updated

## STOP conditions

Stop and report back if:

- `useChatStream.ts` does not export `useChatStream` / does not return
  `{ currentTurn, startStream, resetStream, resolveApproval }` (drift).
- The `llm-done` / `llm-token` / `approval-*` payload shapes in the file
  differ from what's assumed above and the assertion surface becomes unclear —
  adapt the assertions to the live payload, but stop if the payload divergence
  invalidates the test's intent.
- You discover `useChatStream` is no longer consumed by `useSessionManager`
  (would mean the module is dead and these tests are moot).
- Anything in `useSessionManager.test.ts:1-60` (the mock pattern) doesn't
  work when copied into the new file — report instead of authoring a
  different harness.

## Maintenance notes

- These tests are the safety net for plans 013/014 and any future streaming
  refactor. When you change `useChatStream.ts`, run these tests first.
- The `TODO(plan 014)` marked test changes from "asserts re-creation" to
  "asserts the late event is ignored" when plan 014 lands — keep the marker
  until then.
- Reviewer should confirm none of these tests were written to pass against
  post-fix behavior (they must pin the *current* behavior, bugs included).
