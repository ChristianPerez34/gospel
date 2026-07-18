# Plan 009: Fix Local Session Streaming Fallback

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 8c6e3ad..HEAD -- src/hooks/useSessionManager.ts src/hooks/useSessionManager.test.ts`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: MED
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `8c6e3ad`, 2026-07-17

## Why this matters

When backend session creation fails, `useSessionManager` falls back to a
synthetic local session ID and passes that ID to `complete_streaming` on
`src-tauri/src/lib.rs:1275`. The backend's session adapter (line 1018-1025)
looks up the ID in the session store and rejects unknown sessions with
"Session not found", so every local-only session's first turn fails with
that error in the UI. The fix keeps the local-only session visible in the UI
but passes `null` as the session ID to the backend on every turn belonging
to that local-only session, so the backend starts an unanchored turn and
returns a usable stream.

## Current state

The relevant file is `src/hooks/useSessionManager.ts`. The fallback path at
lines 200-247:

```ts
let effectiveSessionId = activeSessionId;
if (!activeSessionId) {
  const title = userMsg.content.slice(0, 50) + (userMsg.content.length > 50 ? "..." : "");
  const mode = draftSessionMode;

  // Try backend session creation first
  let backendSession: { id: string } | null = null;
  try {
    backendSession = await invoke<{ id: string }>("create_session", {
      title,
      provider: selectedModel.provider,
      model: selectedModel.model,
      variant: selectedModel.variant ?? null,
      workspaceId: activeWorkspaceId ?? null,
      mode,
    });
  } catch (e) {
    console.warn("Backend session creation failed, using local session:", e);
  }

  const sessionId = backendSession?.id ?? `s-${Date.now()}`;
  const newSession: Session = {
    id: sessionId,
    title,
    provider: selectedModel.provider,
    model: selectedModel.model,
    variant: selectedModel.variant ?? null,
    mode,
    timestamp: new Date(),
    messages: [userMsg],
    status: "active",
    backendCreated: !!backendSession,
    workspaceId: activeWorkspaceId,
  };
  onSessionsChange((prev) => [newSession, ...prev]);
  setActiveSessionId(sessionId);
  activeSessionIdRef.current = sessionId;
  effectiveSessionId = sessionId;
}

try {
  await startStream({
    provider: selectedModel.provider,
    prompt: message,
    model: selectedModel.model,
    variant: selectedModel.variant ?? null,
    sessionId: effectiveSessionId,
    invokedSkill: invokedSkill ?? null,
  });
} catch (e) {
  setStatus("error");
  resetStream();
  onError?.(`Failed to send: ${e}`, {
    label: "Open Settings",
    onClick: () => onOpenSettings?.(),
  });
}
```

When `create_session` rejects, `effectiveSessionId` is set to the synthetic
`s-${Date.now()}` ID, which the backend does not know about. The backend's
session adapter `session_mode` at `src-tauri/src/lib.rs:1018-1025`:

```rust
fn session_mode(&self, session_id: &str) -> Result<String, String> {
    match &self.session_store_state.store {
        Some(store) => store
            .get_session(session_id)
            .map_err(|e| e.to_string())?
            .map(|session| session.mode)
            .ok_or_else(|| format!("Session not found: {}", session_id)),
        None => Err("session store unavailable".to_string()),
    }
}
```

returns `"Session not found: s-..."` for the synthetic ID. The error is
surfaced as a `LlmError::ProviderError` and eventually bubbles to
`onError` as `"Failed to send: ..."`. On the user's next turn with the
same local session, the synthetic ID is reused.

## Commands you will need

| Purpose         | Command                  | Expected on success               |
|-----------------|--------------------------|-----------------------------------|
| Typecheck       | `bun run typecheck`      | exit 0, no errors                 |
| Lint            | `bun run lint`           | exit 0                            |
| Run tests       | `bun run test`           | exit 0, all pass (incl. new tests)|

## Scope

**In scope**:
- `src/hooks/useSessionManager.ts` — `handleSend` and any helpers needed
  to compute the stream-time session ID.
- `src/hooks/useSessionManager.test.ts` — new regression tests.

**Out of scope**:
- Changes to `useChatStream.ts` or any other hook.
- Changes to the backend (`src-tauri/src/lib.rs`, `src-tauri/src/session_turn.rs`).
- Changes to the `Session` type or to `onSessionsChange` semantics.
- Changes to other consumers of `useSessionManager`.

## Git workflow

- Branch: `advisor/009-fix-local-session-streaming-fallback`
- Commit message style: `fix: pass null to backend when local-only session streams`

## Steps

### Step 1: Compute the stream-time session ID separately from the UI session ID

In `src/hooks/useSessionManager.ts`, replace the inline `effectiveSessionId`
calculation so it tracks whether the session is local-only. Introduce a
single boolean (`isLocalOnly`) computed after the create-or-fallback
branch and use it to decide what to pass as `sessionId` to `startStream`.

Concretely, the new control flow in `handleSend`:

1. Keep the existing `if (!activeSessionId)` block. Inside it, set
   `isLocalOnly = !backendSession` (true when the fallback fired). Push
   the local-only session into the sessions list as today.
2. When `activeSessionId` was already set (the user is on an existing
   session), `isLocalOnly` is `false` if `activeSession.backendCreated`,
   and `true` if the existing session was previously created in
   local-only mode.
3. Compute `streamSessionId = isLocalOnly ? null : sessionId` and pass
   that to `startStream`.

The simplest implementation: read the resulting `Session` object out of
the `onSessionsChange` updater. The existing `newSession` object already
records `backendCreated`, so the answer is already in scope. Sketch:

```ts
let streamSessionId: string | null = effectiveSessionId;
let isLocalOnly = false;
if (!activeSessionId) {
  // ... existing create-or-fallback ...

  const sessionId = backendSession?.id ?? `s-${Date.now()}`;
  isLocalOnly = !backendSession;
  const newSession: Session = {
    id: sessionId,
    // ... same fields ...
    backendCreated: !!backendSession,
    // ...
  };
  onSessionsChange((prev) => [newSession, ...prev]);
  setActiveSessionId(sessionId);
  activeSessionIdRef.current = sessionId;
  effectiveSessionId = sessionId;
  streamSessionId = isLocalOnly ? null : sessionId;
} else {
  // Existing active session — preserve prior local-only status.
  const existing = sessions.find((s) => s.id === activeSessionId);
  isLocalOnly = existing ? !existing.backendCreated : false;
  streamSessionId = isLocalOnly ? null : effectiveSessionId;
}

try {
  await startStream({
    provider: selectedModel.provider,
    prompt: message,
    model: selectedModel.model,
    variant: selectedModel.variant ?? null,
    sessionId: streamSessionId,
    invokedSkill: invokedSkill ?? null,
  });
} catch (e) {
  setStatus("error");
  resetStream();
  onError?.(`Failed to send: ${e}`, {
    label: "Open Settings",
    onClick: () => onOpenSettings?.(),
  });
}
```

Note the `streamSessionId: string | null` type — the existing
`SessionManagerStreamOptions.sessionId` at line 28 is already typed as
`string | null`, so this assignment is type-safe.

**Verify**: `bun run typecheck` exits 0.

### Step 2: Preserve local-only status across multiple turns

The new `isLocalOnly = existing ? !existing.backendCreated : false` branch
handles subsequent turns: if a previous turn fell back to a local-only
session, `existing.backendCreated` is `false`, so the next turn also
passes `null`. A reviewer should confirm the local-only UI session keeps
its synthetic ID internally (so messages and state stay consistent) and
*only* the stream call sees `null`. That is exactly what the snippet in
Step 1 does.

**Verify**: `bun run typecheck` exits 0.

### Step 3: Add regression tests

Open `src/hooks/useSessionManager.test.ts`. The "message orchestration"
`describe` block at line 462 contains the closest existing patterns
(handling `complete_streaming` invocations). Add new tests inside that
block, modeled on `invokes startStream with the correct provider, model,
prompt, and sessionId when a valid model is selected` at line 463-480.

The `vi.mocked(invoke).mockImplementation` pattern in the existing
`beforeEach` (line 89-92) is the right place to wire a rejected
`create_session`. Add three tests:

1. **`falls back to a local session and passes null sessionId on first send when create_session fails`**

   ```ts
   vi.mocked(invoke).mockImplementation(async (cmd: string) => {
     if (cmd === "list_sessions") return [];
     if (cmd === "create_session") throw new Error("backend down");
     return undefined;
   });

   const { result } = renderSessionManager();

   await act(async () => {
     await result.current.handleSend("hello world");
   });

   // The local session is in the UI with backendCreated: false.
   expect(result.current.sessions).toHaveLength(1);
   expect(result.current.sessions[0]).toMatchObject({
     backendCreated: false,
   });
   // ... and the synthetic ID starts with "s-".
   expect(result.current.sessions[0]!.id).toMatch(/^s-/);
   // activeSessionId is the synthetic ID, not null.
   expect(result.current.activeSessionId).toBe(result.current.sessions[0]!.id);
   // But the stream call receives sessionId: null.
   expect(invoke).toHaveBeenCalledWith(
     "complete_streaming",
     expect.objectContaining({
       prompt: "hello world",
       sessionId: null,
     })
   );
   ```

2. **`keeps the local session local-only on a subsequent send`**

   Use the same setup. After the first send, call
   `result.current.handleSend("second prompt")`. Verify
   `complete_streaming` is called again with `sessionId: null` and the
   local session remains in the list with `backendCreated: false` and
   the same synthetic ID. Assert no second `create_session` invocation.

3. **`still passes the backend session ID when create_session succeeds`**

   Sanity: when `create_session` resolves with `{ id: "backend-session" }`,
   the stream call receives `sessionId: "backend-session"`. This test
   already exists in spirit at line 119-148, but make sure the
   `complete_streaming` assertion is present (it is via
   `invokes startStream with the correct provider, model, prompt, and sessionId`).
   If that test does not assert on the actual `sessionId` value sent to
   `complete_streaming`, add the assertion; do not duplicate the entire
   test.

The `mockImplementation` is reset in `afterEach` (line 95-97), so each
test gets a clean slate.

**Verify**: `bun run test -- useSessionManager` exits 0 and reports the new
tests passing.

### Step 4: Run lint and full frontend test suite

- `bun run lint` — exit 0.
- `bun run test` — exit 0, all 111+ tests pass plus the new ones.
- `bun run typecheck` — exit 0 (run again as a final guard).

**Verify**: All three commands exit 0.

## Test plan

- New tests (in `src/hooks/useSessionManager.test.ts`):
  - `falls back to a local session and passes null sessionId on first send when create_session fails`
  - `keeps the local session local-only on a subsequent send`
  - Existing happy-path test continues to cover the backend-success path.
- Pre-existing tests that must continue to pass: the entire
  `useSessionManager.test.ts` suite (~30 tests).
- Verification: `bun run test` exits 0 with 2 more tests than before.

## Done criteria

- [ ] `bun run typecheck` exits 0
- [ ] `bun run lint` exits 0
- [ ] `bun run test` exits 0; the 2 new tests exist and pass
- [ ] No files outside the in-scope list are modified
- [ ] `plans/README.md` status row for plan 009 updated to `DONE`

## STOP conditions

- The drift check shows non-empty output for any in-scope file between
  `8c6e3ad` and `HEAD`. Re-open, reconcile, treat as a planning defect.
- The `isLocalOnly` branch breaks a pre-existing test that asserts the
  synthetic ID is passed to `complete_streaming`. STOP — that test is
  encoding the bug being fixed; update it (the change in expected
  behavior is the point of this plan) rather than re-introducing the
  bug.
- The `effectiveSessionId` variable is removed entirely in the new flow,
  and another part of the function reads it after Step 1. Re-grep
  `useSessionManager.ts` for `effectiveSessionId` after the change to be
  sure no other consumer exists.
- The `Session.backendCreated` field is removed by a future refactor; this
  plan should then use the new field. If a drift check shows the field is
  gone, STOP and rewrite Step 1.

## Maintenance notes

- A reviewer should confirm the local-only session does not silently grow
  stale: the messages list still updates on every turn because the
  `useEffect` at line 156-169 watches `messages` regardless of session
  source. The synthetic ID is stable across turns because it is captured
  in the React state.
- If a future plan adds a retry path that re-attempts `create_session`
  for an existing local-only session, the local-only branch here should
  flip to `streamSessionId = sessionId` once backend creation succeeds.
  The flag-based design in Step 1 supports that extension.
- The `console.warn` at line 217 is preserved — keeping the diagnostic
  log helps the user understand why their session is local-only. Do not
  remove it as part of this change.
