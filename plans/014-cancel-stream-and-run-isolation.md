# Plan 014: Cancel streaming + per-run event isolation

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 72819cd..HEAD -- src/hooks/useChatStream.ts src/hooks/useSessionManager.ts src/components/InputBar.tsx src-tauri/src/lib.rs src-tauri/src/session_turn.rs src-tauri/src/llm.rs`
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

Two related defects in the streaming core:

1. There is **no client-side way to cancel an in-flight stream**. A runaway
   or hung agent turn has no UI escape hatch — `InputBar.tsx:540` disables
   send while streaming, and `useSessionManager.ts` never tells the backend
   to stop. The only recovery is switching session/workspace (which plan 013
   now blocks during streaming, leaving the user fully stuck).
2. `useChatStream`'s listeners have **no per-run isolation**. All ten
   `listen(...)` registrations key solely off `currentTurnRef.current`. A late
   `llm-token` / `llm-tool-result` / `llm-done` event from a previous stream
   (or one that just got cancelled) attaches to the *new* turn. Plan 009
   (local session streaming fallback) closed part of this; the per-run id
   gap was not.

This plan adds a backend `cancel_streaming` command, a Stop control, and a
`run_id` printed on every `llm-*` payload so the FE can ignore stale events.
Both fixes share a single change to the backend stream event emitter.

## Current state

- `src/hooks/useChatStream.ts:115-417` — the listener registration block.
  `currentTurnRef = useRef<CurrentTurn | null>(null)` (line 99-108 region).
  - `updateCurrentTurn` lazily creates a turn via `createTurn()` when the ref
    is `null`.
  - `resetStream` (line 441 region) only nulls the ref; it does not inform
    the backend.
  - `llm-done` and `llm-error` finalize handlers read
    `currentTurnRef.current` and finalize whatever turn is active.
  - `startStream` (line 419 region) invokes `complete_streaming` and returns
    void with no abort handle.
- `src/hooks/useSessionManager.ts:96-116, 154-172, 212` — `handleSend` calls
  `startStream`; the workspace reset path gated on `statusRef`.
- `src/components/InputBar.tsx:540` — `sendDisabled = isStreaming ||
  models.length === 0`; the send button is the only right control.
- `src-tauri/src/lib.rs` — Tauri command surface. Search for `complete_streaming`
  (the existing stream-start command). No `cancel_streaming`,
  `stop_stream`, or `abort` command exists.
- `src-tauri/src/session_turn.rs` — represents the backend Session Turn (see
  CONTEXT.md "Session Turn" glossary). Emits events through a "Tauri adapter
  maps them to frontend events and Trace Log entries" (CONTEXT.md "Session
  Turn Event"). Search the file for `emit`, `llm-token`, `llm-done`,
  `llm-error`, `llm-tool-call`, `llm-tool-result` to find the emit sites —
  every site will need to add the current run id to the payload.
- `src-tauri/src/llm.rs` — the streaming LLM client; the actual generation
  loop is here. Search for `cancel`, `abort`, `Stop`, `Cancellation` to
  confirm whether the existing client supports cancellation at the tokio
  / task level (it almost certainly does via task abort — read the existing
  `tokio::spawn` site that owns the LLM generation task).
- DESIGN.md spec to honor (.inline): "Agent thinking: single `--accent-action`
  pulse bar… No spinner." (DESIGN.md:354). The Stop control must never be a
  spinner. The status indicator colors are at DESIGN.md:421-428:
  - Thinking: `--accent-action` with pulse
  - Acting: `--accent-structure` solid
  - Error: `--status-error` solid
  Stop is a mutation action; per DESIGN.md:224 "exactly: send button, active
  tab, thinking pulse, links, focus rings" — `--accent-action` is reserved
  for primary interactive. Stop is interruptive; use `--status-error` for the
  Stop affordance (consistent with "destructive uses `--status-error`" in
  DESIGN.md §4 Diff panel).
- No ADR covers stream cancellation; existing ADRs (0004, 0005) cover
  persistence separation, which this plan preserves.

## Commands you will need

| Purpose   | Command                                                                  | Expected on success |
|-----------|--------------------------------------------------------------------------|---------------------|
| Typecheck | `bun run typecheck`                                                       | exit 0             |
| Frontend tests | `bun run test`                                                       | all pass           |
| Backend tests | `cargo test --manifest-path src-tauri/Cargo.toml`                     | all pass           |
| Backend lint | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`    | exit 0             |
| Lint | `bun run lint`                                                              | exit 0 (warnings ok) |

## Scope

**In scope**:
- `src-tauri/src/session_turn.rs` (add `run_id` to every `llm-*` event payload)
- `src-tauri/src/llm.rs` (only the task-abort interface, if the cancellation
  surface needs to live at this layer; otherwise leave alone)
- `src-tauri/src/lib.rs` (add `cancel_streaming` Tauri command + register)
- `src/hooks/useChatStream.ts` (track active run id; ignore events with
  non-matching `run_id`; add a `cancelStream` callback that invokes
  `cancel_streaming`; cancelled-turn finalization remains owned by the
  idempotent `llm-error` event path)
- `src/hooks/useSessionManager.ts` (expose `cancelStream` from the manager)
- `src/components/InputBar.tsx` (render a Stop control when `isStreaming`)
- `src/hooks/useChatStream.test.ts` (update the `TODO(plan 014)` test to
  assert stale events are now ignored; add a new test for the cancel path)
- `src/hooks/useSessionManager.test.ts` (add a cancel test)

**Out of scope**:
- Persisting stop reasons in `Model History` (ADR-0005) — a controlled stop
  emits its own `llm-error`-style event with a cancellation reason; reuse the
  existing finalize path; no Model History schema change.
- Cancelling tool execution (only the LLM generation task is cancelled; an
  in-flight tool call completes on its own per the existing flow).
- Backend topology for multi-agent cancellation (single-agent scope).
- Plan 013's workspace-switch guard (independent; depends on this plan only
  to the extent that a "cancel and switch" UX becomes possible later).

## Git workflow

- Branch: `advisor/014-cancel-stream-and-run-isolation`
- Commit per step. Match the repo's `fix:`/`feat:` convention; example:
  `feat: cancel streaming mid-turn and isolate llm-* events per run id`.
- Do NOT push unless instructed.

## Steps

### Step 1: Backend — emit `run_id` on every stream event

In `src-tauri/src/session_turn.rs`:

1. Search the file for every `emit(`, `.emit(`, `app_handle.emit(`, and every
   event emission that produces a frontend `llm-*` event
   (`llm-token`, `llm-done`, `llm-error`, `llm-tool-call`,
   `llm-tool-result`, `approval-requested`, `approval-resolved`). Read the
   current payload struct for each event (likely a `serde::Serialize`d
   struct or `json!{}` inline object).
2. Add a `run_id: String` field to each `llm-*` payload (verification:
   `review-progress` events already carry a `run_id` per
   `src/hooks/useReviewProgress.ts` — mirror that field name exactly). The
   `run_id` value is the Session Turn's id (CONTEXT.md "Session Turn");
   search `session_turn.rs` for an existing turn/run id field; reuse it.
   If no canonical id exists, generate one per `startStream` invocation and
   thread it through the Session Turn state.
3. Update the Rust payload struct definitions in whichever module declares
   them (search for `llm-token` payload serialization sites). All serialization
   must use `#[serde(rename_all = "camelCase")]` so the FE field is `runId`.

**Verify**: `cargo build --manifest-path src-tauri/Cargo.toml` → exit 0.
`cargo test --manifest-path src-tauri/Cargo.toml` → all pass (no test asserts
the absence of `run_id`, but if any test asserts exact event payloads you'll
need to update them; report if you find one that requires semantic decision).

### Step 2: Backend — add `cancel_streaming` Tauri command

Search `src-tauri/src/llm.rs` and `src-tauri/src/session_turn.rs` for the
`tokio::spawn` site that owns the LLM generation task. The standard
cancellation surface is a `CancellationToken` (tokio utilities) or an
`Abortable`/`JoinHandle::abort` registered in a per-turn state map keyed by
the `run_id` from Step 1. Read the existing structure and pick whichever the
existing code uses; do not introduce a new async
primitive if the codebase already chose one.

In `src-tauri/src/lib.rs`, add a `cancel_streaming` command mirroring the
shape of other state-mutating commands in the file:

```rust
#[tauri::command]
fn cancel_streaming(
    state: tauri::State<AppState>,
    run_id: String,
) -> Result<(), String> {
    // Abort the in-flight run for this exact run id.
    // If none is in flight, return Ok(()) cleanly (idempotent).
    state
        .session_turn_handles  // or whatever AppState field holds the in-flight tasks
        .write()
        .map_err(|e| e.to_string())?
        .remove(&run_id)
        .map(|handle| handle.abort());
    Ok(())
}
```

Register it in `generate_handler!` after `complete_streaming` (search that
keyword in `lib.rs`).

As part of this backend step, define the stream-start response contract and
update `complete_streaming` to return it:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StreamStartResponse {
    run_id: String,
}
```

Generate the run id before starting the owned streaming task, register that
task under the same id, and return immediately with
`Ok(StreamStartResponse { run_id })` serialized to the frontend as
`{ runId: string }`. The command must not wait for the full stream before
returning the id. Update the command return type, Tauri command tests, and an
exact serialization test for the camelCase response. The per-run registry and
`cancel_streaming` command must both key cancellation by `run_id`, not only by
session id, so local fallback streams are cancellable too.

If `AppState` has no per-session handle map yet, add one (read the existing
`AppState` definition; this is the surface the plan adds — be conservative:
`HashMap<String, JoinHandle<()>>` guarded by a `Mutex`/`RwLock` matches
how the rest of `AppState` guards mutable state). Insert at the start of
streaming, remove on cancel or on `llm-done` (add the cleanup at the
finalization site so a run that completes cleanly also removes its handle).

**Verify**: `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
→ exit 0. Then `cargo test --manifest-path src-tauri/Cargo.toml` → all pass.

### Step 3: Frontend — track active run id, ignore stale events

In `src/hooks/useChatStream.ts`:

1. Add `const activeRunIdRef = useRef<string | null>(null);` next to
   `currentTurnRef`.
2. Add a frontend response type such as
   `type StreamStartResponse = { runId: string };`. In `startStream`, consume
   `const { runId } = await invoke<StreamStartResponse>("complete_streaming", ...);`
   and assign `activeRunIdRef.current = runId`. Update invoke mocks and tests
   to return `{ runId }`; active-run tracking must rely on this documented
   backend response rather than a locally inferred session id.
3. In `resetStream` and the `llm-done` / `llm-error` finalize handlers: null
   `activeRunIdRef.current = null` at the same point `currentTurnRef.current`
   is nulled.
4. In each `llm-*` listener (token, done, error, tool-call, tool-result,
   approval-*): add a guard at the very top:
   ```ts
   if (payload.runId != null && payload.runId !== activeRunIdRef.current) {
     return; // stale event from a previous run
   }
   ```
   Allow `payload.runId == null` to pass through during the transitional
   window (if any backend event has not yet been updated — but Step 1 added it
   to every `llm-*` event, so this is defense in depth).
5. Replace the lazy-`createTurn` path in the token listener: if
   `currentTurnRef.current` is `null` AND the event's `runId` does not match
   the active run id, **do not create a turn** — return early. This closes
   the late-event race plan 012 characterized.

**Verify**: `bun run typecheck` → exit 0.

### Step 4: Frontend — add `cancelStream` callback

In `src/hooks/useChatStream.ts`:

```ts
const cancelStream = useCallback(async () => {
  const runId = activeRunIdRef.current;
  if (!runId) return;
  try {
    await invoke<void>("cancel_streaming", { runId });
  } catch {
    // best-effort; the backend may already have finalized
  }
}, []);
```

Do not finalize the turn or clear `currentTurnRef` in `cancelStream`. The
backend emits one cancellation payload through the existing `llm-error`-style
event, and that event handler is the single idempotent owner that appends the
cancelled turn and clears `currentTurnRef` plus `activeRunIdRef`. Guard that
handler by the captured `runId` so a late cancellation event can never clear a
newer run. If cancellation is already finalized, the handler is a no-op.

Return `cancelStream` from the hook alongside `startStream` /
`resetStream` / `resolveApproval`.

In `src/hooks/useSessionManager.ts`: expose `cancelStream` from the
manager's return so AppShell / InputBar can call it.

**Verify**: `bun run typecheck` → exit 0.

### Step 5: Frontend — Stop control in InputBar

In `src/components/InputBar.tsx`:

1. Accept a new `onCancelStream?: () => void` prop and an `isStreaming` flag
   (already present, see line 540's `sendDisabled`).
2. When `isStreaming` is true, render a Stop control to the right of (or in
   place of) the send button. Use a square-stop icon from `lucide-react`
   (already a dependency — `grep "lucide-react" package.json`). Style with
   `--status-error` (per DESIGN.md color discipline: destructive/interruptive
   action). Do NOT use a different icon color; white-on-error-red is the
   canonical "Stop".
3. Stop is **distinct** from disabled — it's a real button while streaming;
   never both displayed at once. Insert-wise: put Stop where Send normally
   is (same 36×36 per DESIGN.md Input bar spec).
4. Keyboard: Enter continues to be disabled per existing `sendDisabled`
   logic during streaming; do not rebind Enter to cancel.

**Verify**: `bun run typecheck` → exit 0. `bun run test -- src/components/InputBar.test.tsx`
→ existing tests pass. Add at least one new InputBar test: "renders Stop
control when isStreaming is true and calls onCancelStream on click" (mirror
existing `InputBar.test.tsx` patterns for send-button click).

### Step 6: Wire AppShell

In `src/components/AppShell.tsx`: find the `<InputBar ... />` render (around
line 820) and pass `onCancelStream={() => void session.cancelStream()}`
(adjust to the actual property name `useSessionManager` exposes from
Step 4). Inside `useSessionManager` make sure the prop is wired through to
`useChatStream.cancelStream`.

**Verify**: `bun run typecheck` → exit 0. `bun run test` → full suite green.

### Step 7: Update streaming tests

In `src/hooks/useChatStream.test.ts` (created by plan 012):

- Update the `TODO(plan 014)` test: change it from "asserts the late event
  lazily re-creates a turn" to "asserts the late event is ignored because
  its `run_id` does not match the active run id". Remove the TODO marker.
- Add a test: `cancelStream targets the active run and cancellation finalizes once`, asserting:
  - `invoke` was called with `"cancel_streaming"` and the captured `runId`.
  - `cancelStream` does not append/finalize the turn locally.
  - The matching backend `llm-error` cancellation event finalizes the turn and
    clears local run state exactly once; a duplicate event is a no-op.
  - Subsequent `llm-token` events do *not* create a new turn (the run id is
    now null).

In `src/hooks/useSessionManager.test.ts`:

- Add a test: `cancelStream stops the in-flight turn and frees the workspace
  switch path` — render manager, start stream, call `cancelStream`, trigger the
  matching backend cancellation event, then select another session. Assert the
  switch is honored only after the event-owned finalizer clears streaming
  state. The cancelled turn must be persisted to the original session, not the
  new one.

**Verify**: `bun run test` → all pass, including the new tests and the
updated plan-012 test.

## Test plan

- New backend tests: update event payload assertions to include `runId`, assert
  `complete_streaming` serializes `{ runId }`, and test `cancel_streaming`
  idempotency (cancelling a non-running run returns `Ok(())`).
- New FE tests in `useChatStream.test.ts` (updated plan-012 test + cancel test).
- New FE tests in `useSessionManager.test.ts` (cancel-unblocks-workspace-switch).
- New `InputBar.test.tsx` test (Stop control renders + click handler).
- Pattern: `src/hooks/useSessionManager.test.ts` (`capturedListeners` + `triggerEvent`).

## Done criteria

- [ ] `cargo build --manifest-path src-tauri/Cargo.toml` exits 0
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0
- [ ] `bun run typecheck` exits 0
- [ ] `bun run test` exits 0; the plan-012 `TODO(plan 014)` test is updated and there are new cancel tests in 2 FE test files
- [ ] `rg "TODO\(plan 014\)" src/hooks/useChatStream.test.ts` returns no matches (the marker was removed)
- [ ] `rg "cancel_streaming" src-tauri/src/lib.rs` and `"cancelStream" src/hooks/useChatStream.ts` each return ≥1 match
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row for plan 014 updated

## STOP conditions

Stop and report back if:

- `session_turn.rs` has no existing run/turn id field and no obvious place to
  generate one — this plan's Step 1 needs a canonical id; report and propose
  the generation site (do not invent a second id system).
- The LLM client does not support task abort and retrofitting it requires
  changes to `provider_client.rs` or `llm.rs` beyond what Step 2 describes —
  report; the cancel surface may need to be a "send stop token to the
  provider" path instead, which changes Step 2.
- The existing `complete_streaming` command's return type cannot be widened
  to `{ runId: string }` without breaking other consumers (search the FE for
  `complete_streaming` callers).
- A `review-progress` or other non-LLM event payload also has `runId` and
  the inclusion causes a payload-shape test to fail in a way the plan can't
  fix without a semantic decision.
- Adding `run_id` to events changes the Trace Log JSON shape in a way
  `trace.rs`'s redaction or downstream parsers assume — surface the
  interaction with the redaction step in plan 019 (defer the redaction
  interaction to plan 019 if discovered).

## Maintenance notes

- The cancellation reason becomes part of the finalized turn; when adding
  new Turn states (e.g. tool-failed vs llm-failed), update the single
  idempotent `llm-error` finalizer rather than adding completion logic to
  `cancelStream`.
- The `run_id` is the SSOT for "is this event from the active run?" Any
  future event type added to `llm-*` family must include `runId` (add a
  test that asserts this invariant).
- Reviewer should check: (a) the `run_id` is camelCase in the FE payload
  (serde rename), (b) the `cancel_streaming` command is idempotent, (c)
  the Stop control matches DESIGN.md color discipline (`--status-error`),
  (d) workspace-switch is unblocked after cancel (verified by the
  `useSessionManager` test in Step 7).
- Follow-up deferred: cancelling in-flight tool calls (out of scope; only
  the LLM generation is cancelled here).
