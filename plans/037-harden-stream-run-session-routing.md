# Plan 037: Harden Stream Run/Session Routing in `useChatStream`

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 2982ea5..HEAD -- src/hooks/useChatStream.ts src/hooks/useSessionManager.ts src/hooks/useChatStream.test.ts src/hooks/useSessionManager.test.ts`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED — restructures the stream-buffer internals; the existing characterization tests (plan 012-era, ~830 LOC in `useChatStream.test.ts`) are the regression net and must keep passing
- **Depends on**: plans/035-route-approval-events-by-session.md (shared file — land 035 first; its `updateTurnForSession` helper and sid-scoped flush calls become this plan's baseline)
- **Category**: bug
- **Planned at**: commit `2982ea5`, 2026-10-10

## Why this matters

The multi-run task control plane routes each streamed event to a session via `runSessionMapRef` (runId → sessionId) — but three gaps in that routing corrupt or hide output:

1. **Local-only sessions stream invisibly.** When backend `create_session` fails, `useSessionManager` proceeds with a frontend-only session (`s-<timestamp>`) and calls `startStream` with `sessionId: null` (useSessionManager.ts:~316,320 — `streamSessionId = isLocalOnly ? null : ...`). `startStream` then records `runSessionMap.set(runId, null)` (useChatStream.ts:1030). `isBackground` sees `mapped === null`, `mapped !== focused` → **true**, so every token lands in `backgroundPendingRef` — but `ChatView` only renders `session.currentTurn` (AppShell.tsx:~904), so the user watches a frozen "Thinking…" until `llm-done`, when the whole reply pops in at once.
2. **Null-session completions corrupt the focused transcript.** `sessionForRun(runId)` resolves a `null` mapping via `?? getFocusedSession()` (line ~256), so a background local-only run's `llm-done`/`llm-error` appends to whichever session is focused at completion (`onMessagesForSession` → `handleMessagesForSession`, which for `sessionId == null` writes the visible transcript — useSessionManager.ts:~145-148). Cross-session transcript corruption.
3. **Focus-switch flush gap + draft-view interleave.** `pendingTextRef` is a single shared foreground buffer; a token arriving between the `sessionId` prop commit and the stash effect (useChatStream.ts:~327-395) is read as the *outgoing* session's `outgoingText` and spliced into its stash. And when `focused == null` (new-task interim), `isBackground` returns false for **all** runs — two concurrent runs' tokens interleave into one buffer.
4. **runId-less `llm-done` wipes the focused turn.** With `focusedBefore != null`, `completionSession = focusedBefore` unconditionally and `clearCurrentTurn()` runs (~lines 544-598) — a stray legacy/test payload discards the visible in-progress turn.

All four share one root: the run→session mapping stores the *backend* session id (or `null`), while routing actually needs the *frontend* `Session.id`; and the foreground text buffer is the only buffer not keyed by session.

## Current state

`src/hooks/useChatStream.ts` (~1,170 LOC) — the stream event hook. Relevant structures:

```typescript
  const runSessionMapRef = useRef<Map<string, string | null>>(new Map());        // ~165 — runId → sessionId (currently BACKEND id or null)
  const backgroundTurnsRef = useRef<Map<string | null, CurrentTurn>>(new Map()); // ~167 — stashed live turns
  const backgroundPendingRef = useRef<Map<string | null, string>>(new Map());   // ~168 — per-session pending text
  const backgroundFrameRef = useRef<Map<string | null, FrameHandle>>(new Map());// ~169 — per-session flush handles
  const pendingTextRef = useRef<string>("");                                    // ~179 — SHARED foreground buffer (the bug)
```

```typescript
  const sessionForRun = (runId: unknown): string | null => {                    // ~253-258
    if (typeof runId !== "string") return getFocusedSession();
    return runSessionMapRef.current.get(runId) ?? getFocusedSession();
  };
  const isBackground = (runId: unknown): boolean => {                           // ~425-435
    if (runId == null) return false;
    if (typeof runId !== "string") return false;
    const focused = getFocusedSession();
    if (focused == null) return false;          // ← draft-view: everything "foreground"
    const mapped = runSessionMapRef.current.get(runId);
    if (mapped === undefined) return false;
    return mapped !== focused;                  // ← mapped=null (local-only) → background
  };
```

The token handler (~463-491) routes background tokens to `backgroundPendingRef[sessionForRun(runId)]` + per-session `scheduleFlush` → `flushBackgroundPending(sessionId)`; foreground tokens append to the shared `pendingTextRef` → `flushPendingText()`. `flushBackgroundPending` is at ~286; `singleActiveSession` (runId-less completion disambiguation) at ~437-449; the focus-switch stash effect at ~327-395 reads `pendingTextRef.current` as `outgoingText`; `startStream` (~1024-1045) records `runSessionMapRef.current.set(runId, opts.sessionId ?? null)`; `llm-done` (~493-598) has a correct background branch but a focused-fallback foreground path.

`src/hooks/useSessionManager.ts` — the send path (~293-330):

```typescript
        const sessionId = backendSession?.id ?? `s-${Date.now()}`;   // FE Session.id — ALWAYS defined
        const isLocalOnly = !backendSession;
        ...
        streamSessionId = isLocalOnly ? null : sessionId;            // backend id or null → passed to startStream
```

and `handleMessagesForSession` (~140-155) — the `if (!sessionId) { setMessages(updater); return; }` branch is what mis-attributes null-session completions.

`src/types/index.ts` — `CurrentTurn` is `{ id: string; blocks: TurnBlock[]; createdAt: Date }` (~line 125).

**Design decision (inlined)**: `runSessionMapRef` should store the **frontend `Session.id`** (what `useSessionManager` calls `sessionId`, e.g. `s-<ts>` for local-only) — the id that `handleMessagesForSession`, `liveTurnsBySession`, and the focus machinery all key on. For backend-created sessions `Session.id === backendSession.id`, so nothing changes for them. With a concrete id per run, `null` mappings disappear and `isBackground`/`sessionForRun`/`singleActiveSession` all work correctly by construction.

## Commands you will need

| Purpose        | Command                       | Expected on success |
|----------------|-------------------------------|---------------------|
| Frontend tests | `bun run test`                | all pass (vitest)   |
| Targeted tests | `bun run test -- useChatStream` / `bun run test -- useSessionManager` | all pass |
| Typecheck      | `bun run typecheck`           | exit 0              |
| Lint           | `bunx biome check .`           | exit 0              |

Do NOT use `bun test` — Bun's native runner doesn't understand `vi.mock`/`vi.mocked`. Suite is Vitest via `bun run test`; setup lives in `src/test/setup.ts`.

## Scope

**In scope** (the only files you should modify):
- `src/hooks/useChatStream.ts` — `StartStreamOptions`, `startStream` mapping, per-session pending-text unification, runId-less completion disambiguation
- `src/hooks/useSessionManager.ts` — pass `runSessionId` (the FE `Session.id`) at both `startStream` call sites
- `src/hooks/useChatStream.test.ts`, `src/hooks/useSessionManager.test.ts` — regression tests

**Out of scope**:
- `src-tauri/*` — backend events already carry `runId`; no backend change.
- `src/components/AppShell.tsx`, `ChatView.tsx`, `SessionDrawer.tsx` — display path unchanged.
- `src/types/index.ts` — `StartStreamOptions` lives in `useChatStream.ts` (~line 111), not the types file.
- `cancelStreamForSession`'s backend reach for local-only runs (`invoke("cancel_streaming")` needs a backend session id — `lib.rs` documents local-only turns as uncancellable). That is a *backend* gap recorded separately in `plans/README.md`; do not fix it here.
- The `llm-verification`/`llm-tools-progress` events — backend emits them; FE intentionally has no listeners. Leave alone.

## Git workflow

- Branch: `advisor/037-stream-run-session-routing`
- Commit style: conventional commits, e.g. `fix(streaming): route runs by frontend session id, key pending text per session`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Map runs to the frontend session id

- `StartStreamOptions` (`useChatStream.ts:~111`): add `runSessionId?: string | null` — documented `/** Frontend Session.id used for event routing; may differ from backend sessionId for local-only sessions. */`
- `startStream` (~1030): change to
  ```typescript
  runSessionMapRef.current.set(runId, opts.runSessionId ?? opts.sessionId ?? null);
  ```
- `useSessionManager.ts`: at both `startStream({...})` call sites (~316-330 — the new-session branch and the existing-session branch), pass `runSessionId: sessionId` where `sessionId` is the local FE id variable (`backendSession?.id ?? \`s-${Date.now()}\`` in the new-session branch; `existing.id`/`activeSessionId`-equivalent in the existing-session branch — use the `Session.id`, never `streamSessionId`).
- `cancelStreamForSession` (~1048-1075): with local-only runs now mapped to FE ids, the existing lookup `mapped === sid` finds them. `sid` remains whatever the caller passes (FE id) — for backend sessions that IS the backend session id, so `invoke("cancel_streaming", { sessionId: sid })` still targets the right handle; for local-only ids it's a harmless no-op on the backend (documented gap). Leave as-is — but if the code compares `mapped` against backend ids anywhere else, audit and unify to FE ids.

**Verify**: `bun run typecheck` → 0; `bun run test` → existing tests pass (backend sessions unchanged because runSessionId === sessionId for them).

### Step 2: Key the foreground pending buffer by session

Replace the two-buffer split (`pendingTextRef` shared + `backgroundPendingRef` per-session) with one per-session map so a buffer can never be attributed to the wrong session:

- Rename/replace `backgroundPendingRef` with `pendingBySessionRef: Map<string | null, string>` used for **all** sessions, focused or not. (Keeping the existing name is acceptable — the point is one map.)
- Token handler (~463-491): for every non-stale token, `const sid = sessionForRun(runId);` append to `pendingBySessionRef[sid]`; schedule a per-session frame via a `Map<sid, FrameHandle>` (extend `backgroundFrameRef` semantics to all sids). The scheduled flush for `sid`: if `sid === getFocusedSession()` apply the existing foreground append-to-last-text-block logic to `currentTurnRef`/`setCurrentTurn`; else apply the existing `flushBackgroundPending` stash + `onLiveTurnForSession` path. Factor the shared tail into `flushPendingForSession(sid)` — i.e., `flushPendingText()` and `flushBackgroundPending(sid)` merge into one function taking a session id.
- Update every call site — enumerate with `grep -n "flushPendingText\|pendingTextRef\|flushBackgroundPending\|backgroundPendingRef" src/hooks/useChatStream.ts` and convert each to the sid-scoped call (event handlers flush `sessionForRun(payload.runId)`; the focus-switch effect flushes the outgoing sid `prev`; lifecycle/cancel paths flush the run's resolved sid). Do not leave a bare "flush whichever" call — that's the bug shape.
- Focus-switch stash effect (~327-395): `outgoingText` now reads `pendingBySessionRef.current.get(prev) ?? ""` and deletes it; the incoming-session restore must ALSO flush `pendingBySessionRef.current.get(focused)` into the restored turn (mirror the outgoing fold). The `focused == null` case: every run buffers under its own sid; when a session becomes focused, the restore path surfaces its stash — tokens are no longer interleaved or lost.
- Update `clearCurrentTurn` (~240-248) and the `llm-done`/`llm-error`/cancel finalizers to clear `pendingBySessionRef[sid]` for their resolved sid (in addition to current behavior).

**Verify**: `bun run test -- useChatStream` → all existing coalescing/isolation tests pass unchanged (they were written against the public event contract — if one now fails, check whether it was asserting the buggy shared-buffer shape and report before "fixing" the test).

### Step 3: runId-less completion disambiguation

In the `llm-done` foreground path (~544-560), drop the `focusedBefore == null` condition so the single-active-run disambiguation always runs:

```typescript
              const focusedBefore = getFocusedSession();
              const fallbackSingle = runId == null ? singleActiveSession() : null;
              const completionSession = fallbackSingle ?? focusedBefore;
```

(`singleActiveSession` at ~437-449 already returns the sole in-flight session or null.) Apply the identical change to `llm-error`'s equivalent path if it has the same shape — check it; if it already disambiguates, leave it.

Residual (document, don't fix): runId-less completion with multiple active runs and a focused session still falls back to focused — unresolvable without identity; real emitters always send `runId` (`session_turn.rs` `ui_event_payload`).

**Verify**: `bun run test -- useChatStream` → pass.

### Step 4: Regression tests

In `useChatStream.test.ts` (model on `describe("token and done listeners")` ~82 and the background-run tests that emit `runId` payloads — e.g. `late llm-token with a non-matching run_id is ignored` ~441):

1. `local-only run streams tokens into the focused currentTurn` — `startStream({ sessionId: null, runSessionId: "s-1" })` while `s-1` is focused → `llm-token` events produce visible `currentTurn` text blocks (previously: invisible).
2. `local-only run completion lands on its own transcript while another session is focused` — run mapped to `s-1`, focus `s-2`, emit `llm-done` → `onMessagesForSession` called with `"s-1"`, not `s-2`/null.
3. `token arriving between focus change and stash effect lands on the new session` — emit token for session B's run after `sessionId` prop switches A→B but before the effect runs (use the test harness's existing ability to control effect timing / `act` boundaries — model on the race tests added in the plan-013/014 era); assert B's buffer gets it and A's stash doesn't.
4. `two concurrent runs while focused==null keep separate buffers` — start two runs, no focus → tokens for each accumulate under their own sids; when focus lands on one, its buffer renders and the other stays stashed.
5. `runId-less llm-done with exactly one active run attributes to that session even when another is focused` — the Step 3 path.

In `useSessionManager.test.ts` (~1,230 LOC exists): one test asserting `startStream` receives `runSessionId` = the frontend session id for a local-only session (backend `create_session` failing path — the file already mocks invoke; find the existing local-session test ~its `console.warn("Backend session creation failed...")` path and extend it).

**Verify**: `bun run test` → all pass including ≥6 new tests; `bun run typecheck` → 0; `bunx biome check .` → 0.

## Test plan

Covered by Step 4. Structural patterns: `useChatStream.test.ts` (`describe("token and done listeners")` ~line 82, per-run isolation test ~line 441, cancelStream describe ~line 481); `useSessionManager.test.ts` for the send-path assertion.

Verification: `bun run test` → all pass.

## Done criteria

- [ ] `runSessionMapRef` stores frontend `Session.id`s; `startStream` records `opts.runSessionId ?? opts.sessionId ?? null`; `useSessionManager` passes `runSessionId` at all call sites
- [ ] No shared session-less pending buffer remains — `grep -n "pendingTextRef" src/hooks/useChatStream.ts` returns nothing (or only a renamed per-session structure); every flush call names its session
- [ ] runId-less `llm-done` attempts `singleActiveSession()` regardless of focus
- [ ] `bun run test` exits 0 incl. ≥6 new tests; `bun run typecheck` and `bunx biome check .` exit 0
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0 (unchanged, sanity)
- [ ] Only in-scope files modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The buffer/listener internals no longer match the excerpts (this file churns — the coalescing refactor landed recently).
- Plan 035 has NOT landed and its `updateTurnForSession`/sid-scoped flush calls are absent — either land 035 first, or implement this plan's sid-scoped flush to coexist with the current approval handlers and note the conflict; do not silently drop either shape.
- An existing characterization test fails post-change in a way that indicates the OLD behavior was intentional (e.g., a test asserting null-mapped runs buffer in background) — report which test and which behavior changed.
- `useSessionManager`'s send path has been restructured so "frontend session id" is no longer a distinct value from the backend session id.
- A verification command fails twice after a reasonable fix attempt.

## Maintenance notes

- The invariant: **`runSessionMapRef` keys frontend `Session.id`s**; every streamed event routes through `sessionForRun` + per-session buffers. Any new event type or new send path must populate the map with the FE id.
- Residual gaps recorded, not fixed here: (a) local-only runs can't be cancelled backend-side (`cancel_streaming` is session-id-keyed; a runId-keyed cancel channel is a future backend plan); (b) runId-less completions under multiple active runs + focus are unresolvable and keep the focused fallback; (c) approvals lacked run identity entirely — plan 035 adds `run_id` to approval payloads and routes them; if 037 lands first, re-run 035's Step 4 against this buffer shape.
- Reviewer should scrutinize: the merged `flushPendingForSession(sid)` — flushing the *wrong* sid is the old bug's shape; every call site should derive sid from the event payload (`sessionForRun(payload.runId)`) or the known focused session, never a bare global flush.
- Frontend render perf (no `React.memo` in `ChatView`, `animated` on finalized `Streamdown` blocks) is a separate deferred finding — this plan intentionally does not touch rendering.
