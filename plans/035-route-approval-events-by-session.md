# Plan 035: Route Approval Events to the Owning Session's Turn

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 2982ea5..HEAD -- src-tauri/src/approval_broker.rs src-tauri/src/session_turn.rs src-tauri/src/lib.rs src/types/index.ts src/hooks/useChatStream.ts src/hooks/useChatStream.test.ts`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED — touches the safety-critical approval path; mitigated by payload-additive changes and routing reuse
- **Depends on**: none (but run BEFORE plan 037 — both touch `useChatStream.ts`; this plan's handlers become 037's baseline)
- **Category**: bug (safety-surface correctness)
- **Planned at**: commit `2982ea5`, 2026-10-10

## Why this matters

Gospel's task control plane runs multiple sessions' turns concurrently — a background session's agent can keep working while the user watches another. When a *background* run hits a mutating command or external-path read, the approval flow currently emits a global `approval-requested` event carrying no session/run identity. The frontend handler unconditionally appends the approval card to the **focused** session's live turn (`updateCurrentTurn`), so the user sees — and can approve — a command belonging to a task they are not looking at. The approval *decision* itself reaches the correct backend request (the broker resolves by globally-unique request id), but the safety property that matters here is human comprehension: the card's displayed command must belong to the turn it's displayed on, and a background run's approval must be recorded in that run's own transcript.

Every other streamed event (`llm-token`, `llm-done`, `llm-tool-*`) already carries a `runId` in its payload and routes through `isBackground`/`sessionForRun` in `useChatStream.ts`. This plan gives approval events the same identity and routes them through the same primitives — additive payload fields, no changes to the broker's resolve-by-id semantics.

## Current state

### Backend

`src-tauri/src/approval_broker.rs` — the approval domain module. `ApprovalRequest` (lines ~50–57) has no run/session field:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub kind: ApprovalKind,
    pub tool_name: String,
    pub title: String,
    pub summary: String,
    pub reason: String,
    pub risk: ApprovalRisk,
}
```

Constructors `ApprovalRequest::command(tool_name, command_label, reason, destructive)` (~lines 61–88) and `ApprovalRequest::external_path(tool_name, path, reason)` (~lines 89–120) mint the id internally. `ApprovalResolution` (~lines 135–137):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResolution {
    pub id: String,
    pub outcome: ApprovalOutcome,
}
```

`ApprovalBroker::request` (~lines 182–214) registers a oneshot sender in `pending: Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>`, emits the request, awaits the decision/timeout, then emits the resolution — it constructs the `ApprovalResolution` itself (~line 206), so it can echo the request's `run_id` without touching the pending map:

```rust
        let outcome = ApprovalOutcome::from_decision(decision, timed_out);
        self.emitter
            .emit_resolved(&ApprovalResolution { id, outcome });
```

`resolve(id, decision)` (~lines 224–235) removes the sender and notifies — it does NOT emit; the resolution event always comes from `request()`. So `resolve` needs no signature change.

`src-tauri/src/lib.rs` — the emit adapter (~lines 195–216) forwards broker events to the webview unchanged; the approval *wrappers* are built per-turn inside `TauriSessionTurnAdapters::stream_completion` (~lines 1177–1187), inside whose `SessionTurnStreamRequest` scope the session/run identity lives:

```rust
            let broker = self
                .app
                .state::<ApprovalBrokerState>()
                .broker()
                .expect("approval broker is installed during app setup");
            let external_path_approval: Arc<dyn ExternalPathApproval> =
                Arc::new(BrokerExternalPathApproval {
                    broker: broker.clone(),
                });
            let command_approval: Arc<dyn CommandApproval> = Arc::new(BrokerCommandApproval {
                broker: broker.clone(),
            });
```

`BrokerCommandApproval::request_approval` (~lines 251–268) builds `ApprovalRequest::command(request.tool_name, request.command_label, request.reason, destructive)`; `BrokerExternalPathApproval::request_approval` (~lines 223–248) builds `ApprovalRequest::external_path(...)`. These are the only two constructors in production paths.

`src-tauri/src/session_turn.rs` — `SessionTurnStreamRequest` (~lines 114–132) already carries `session_id: Option<&'a str>` but NOT `run_id`; the orchestrator owns `run_id` (`let run_id = request.run_id;` ~line 435, from `pub run_id: String` on the outer request ~line 27) and builds the stream request nearby (~lines 440–460). All streamed UI events carry `runId` via `ui_event_payload` (~lines 944–978: `json!({ "runId": run_id, "token": token })`).

**Decision (inlined — do not relitigate)**: tag approvals with `run_id`, not `session_id`. Every other streamed event is `runId`-keyed and the FE routing primitives (`isBackground`, `sessionForRun`, `runSessionMapRef`) are runId-keyed; local-only sessions (backend `session_id: null`) still get runIds. `session_id` alone cannot route a local-only run.

### Frontend

`src/types/index.ts` (~lines 34–47) — wire types mirror the Rust structs in snake_case (serde default):

```typescript
export interface ApprovalRequest {   // fields include id, kind, tool_name, title, summary, reason, risk
export interface ApprovalResolution { // id, outcome
```

`src/hooks/useChatStream.ts` — routing primitives already exist:

```typescript
      const sessionForRun = (runId: unknown): string | null => {   // ~lines 253-258
        if (typeof runId !== "string") return getFocusedSession();
        return runSessionMapRef.current.get(runId) ?? getFocusedSession();
      };
      const isBackground = (runId: unknown): boolean => {           // ~lines 425-435
        ...
        const mapped = runSessionMapRef.current.get(runId);
        if (mapped === undefined) return false;
        return mapped !== focused;
      };
```

The approval handlers (~lines 929–975) ignore all of that and always touch the focused turn:

```typescript
          track(
            listen<ApprovalRequest>("approval-requested", (event) => {
              // Flush buffered text before appending an approval block so
              // the approval card appears after trailing text.
              flushPendingText();
              updateCurrentTurn((turn) => {
                // ...dedupe by event.payload.id, then append
                // { kind: "approval", id, toolName, approvalKind, title,
                //   summary, reason, risk, status: "pending" }
              });
            })
          ),
          track(
            listen<ApprovalResolution>("approval-resolved", (event) => {
              flushPendingText();
              const status = event.payload.outcome;
              updateCurrentTurn((turn) => ({
                ...turn,
                blocks: turn.blocks.map((block) =>
                  block.kind === "approval" && block.id === event.payload.id
                    ? { ...block, status } : block ),
              }));
            })
          ),
```

Background-turn machinery already exists for exactly this: `backgroundTurnsRef: Map<string|null, CurrentTurn>` (~line 167) stores stashed live turns, `backgroundPendingRef` (~line 168) buffers their text, and `optionsRef.current.onLiveTurnForSession?.(sid, turn)` publishes updates (see `flushBackgroundPending` ~line 286 and the `llm-done` background branch ~lines 497-520 for the exact usage pattern). `CurrentTurn` shape (`src/types/index.ts:125`): `{ id: string; blocks: TurnBlock[]; createdAt: Date }`; the approval `TurnBlock` variant is at ~lines 108–122.

## Commands you will need

| Purpose        | Command                                             | Expected on success |
|----------------|-----------------------------------------------------|---------------------|
| Rust tests     | `cargo test --manifest-path src-tauri/Cargo.toml`   | all pass            |
| Frontend tests | `bun run test`                                      | all pass (vitest)   |
| Typecheck      | `bun run typecheck`                                 | exit 0              |
| Lint           | `bunx biome check .`                                 | exit 0              |
| Clippy         | `bun run clippy`                                    | exit 0              |

Do NOT use `bun test` — it runs Bun's native runner, which does not understand `vi.mock`/`vi.mocked`. The suite is Vitest: `bun run test`.

## Scope

**In scope** (the only files you should modify):
- `src-tauri/src/approval_broker.rs` — payload fields, `with_run_id` builder, resolution echo, unit tests
- `src-tauri/src/session_turn.rs` — `SessionTurnStreamRequest.run_id` field + construction site
- `src-tauri/src/lib.rs` — `run_id` plumbing through `BrokerCommandApproval`/`BrokerExternalPathApproval`
- `src/types/index.ts` — `run_id` on `ApprovalRequest`/`ApprovalResolution`
- `src/hooks/useChatStream.ts` — route the two approval handlers
- `src/hooks/useChatStream.test.ts` — new routing tests

**Out of scope**:
- `resolve_approval` IPC command (`lib.rs:~1521`) and the FE `resolveApproval` — decisions already resolve by globally-unique id; unchanged.
- `TauriApprovalEventEmitter` (lib.rs:~195–216) — still forwards payloads opaquely; unchanged.
- Any new session-scoped event channel (e.g., emitting approvals through `SessionTurnEvent`) — deliberately not chosen; the broker stays the single approval channel.
- `useSessionManager.ts`, `AppShell.tsx`, `ApprovalCard.tsx` — display components need no change; the fix is event routing only.
- Verification/exploration sub-agent approval paths — their harness profiles set `command_approval: None` (`harness_profile.rs:546-547`), so no other constructor call sites exist; if you find one, note it but still route via `with_run_id`.

## Git workflow

- Branch: `advisor/035-route-approval-events`
- Commit style: conventional commits, e.g. `fix(approvals): tag approval events with run_id and route to owning turn`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add `run_id` to the approval payloads

In `src-tauri/src/approval_broker.rs`:

- Add `pub run_id: Option<String>` to `ApprovalRequest` and `ApprovalResolution`.
- Update the two constructors to set `run_id: None` initially, and add a builder method on `ApprovalRequest`:

```rust
    /// Tag the request with the owning streaming run so the frontend can
    /// route the approval card to the correct session's turn.
    pub fn with_run_id(mut self, run_id: Option<&str>) -> Self {
        self.run_id = run_id.map(str::to_string);
        self
    }
```

- In `request()`, capture the id AND the run_id before awaiting (`let run_id = request.run_id.clone();` next to `let id = request.id.clone();`), then emit `ApprovalResolution { id, outcome, run_id }`.
- Update existing constructor call sites inside the broker's own tests to compile (they'll set `run_id: None` via the constructors as before).
- New unit test `resolution_echoes_request_run_id` in the broker's existing `#[cfg(test)] mod tests` (model on `request_emits_event_and_waits_for_decision` ~line 280): build a request `.with_run_id(Some("r-1"))`, approve it, assert the emitted `ApprovalResolution.run_id == Some("r-1")` on the `RecordingEmitter` capture.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml approval_broker` → pass.

### Step 2: Plumb `run_id` into the stream request and the broker wrappers

- `session_turn.rs`: add `pub run_id: Option<&'a str>` to `SessionTurnStreamRequest` (next to `session_id`, ~line 131). Find every construction site of `SessionTurnStreamRequest` (`grep -n "SessionTurnStreamRequest" src-tauri/src/`) — the orchestrator sets `run_id: Some(&request.run_id)` (the outer request owns `run_id: String`); any test/replay constructors set `run_id: None`.
- `lib.rs`: add `run_id: Option<String>` to `BrokerCommandApproval` and `BrokerExternalPathApproval`; at the construction site (~lines 1177–1187) set `run_id: request.run_id.map(str::to_string)`. In each `request_approval` impl, append `.with_run_id(self.run_id.as_deref())` to the `ApprovalRequest::command(...)` / `external_path(...)` call.
- Also check `skill_script_tool` — it receives `command_approval.clone()` at ~line 1190, so skill-script approvals inherit `run_id` automatically. Confirm no other place constructs `BrokerCommandApproval`/`BrokerExternalPathApproval` (`grep -n "BrokerCommandApproval\|BrokerExternalPathApproval" src-tauri/src/`).

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml` → all pass; `bun run clippy` → 0.

### Step 3: Update the wire types

`src/types/index.ts`: add `run_id?: string | null` to both `ApprovalRequest` and `ApprovalResolution` interfaces, matching the snake_case serde output.

**Verify**: `bun run typecheck` → exit 0.

### Step 4: Route the frontend handlers by run

In `useChatStream.ts`, introduce a session-routed sibling of `updateCurrentTurn` and use it in both approval handlers:

```typescript
      // Mutate a turn for an arbitrary session: focused turns go through
      // updateCurrentTurn; background turns update the stash and republish.
      const updateTurnForSession = (
        sessionId: string | null,
        updater: (turn: CurrentTurn) => CurrentTurn
      ) => {
        if (sessionId === getFocusedSession()) {
          updateCurrentTurn(updater);
          return;
        }
        const existing = backgroundTurnsRef.current.get(sessionId) ?? {
          id: generateTurnId(),
          blocks: [],
          createdAt: new Date(),
        };
        const next = updater(existing);
        backgroundTurnsRef.current.set(sessionId, next);
        optionsRef.current.onLiveTurnForSession?.(sessionId, next);
      };
```

(`generateTurnId` and `CurrentTurn` are already imported/defined in this file — reuse whatever names exist; check imports.)

Then rewrite `approval-requested`:

```typescript
            listen<ApprovalRequest>("approval-requested", (event) => {
              const runId = event.payload.run_id ?? null;
              const sid = sessionForRun(runId);
              const isFocused = sid === getFocusedSession();
              if (isFocused) flushPendingText(); else flushBackgroundPending(sid);
              updateTurnForSession(sid, (turn) => {
                // identical dedupe + append logic as before, verbatim
              });
            })
```

and `approval-resolved` the same way with the existing status-mutation body. Payloads lacking `run_id` resolve through `sessionForRun`'s `?? getFocusedSession()` fallback — preserving today's behavior for any emitter that doesn't send it.

**Verify**: `bun run typecheck` → 0; `bun run test` → existing tests pass (the :300 approval lifecycle test still passes since foreground routing is unchanged).

### Step 5: Frontend routing tests

In `useChatStream.test.ts`, inside `describe("tool and approval lifecycle")` (~line 242) — model on `approval-requested emits the request; approval-resolved transitions the card` (~line 300):

- `approval-requested with a background run_id stashes the card under that session, not currentTurn` — start a stream mapped to session B, focus session A, emit the approval with `run_id` = B's run; assert `currentTurn` unchanged and `onLiveTurnForSession` received a turn containing the approval block for B.
- `approval-resolved for a background run updates the stashed card` — continue: emit resolved; assert stashed block status changed, `currentTurn` untouched.
- `approval-requested with no run_id falls back to focused session` — legacy payload shape keeps current behavior.

**Verify**: `bun run test` → all pass including ≥3 new tests.

## Test plan

Covered by Steps 1 and 5 — one broker unit test (model on `request_emits_event_and_waits_for_decision`, `approval_broker.rs:~280`) and ≥3 hook tests (model on `useChatStream.test.ts:300`).

Verification: `cargo test --manifest-path src-tauri/Cargo.toml` and `bun run test` → all pass.

## Done criteria

- [ ] `ApprovalRequest` and `ApprovalResolution` carry `run_id` end-to-end (grep shows the field on both structs and both TS interfaces)
- [ ] `SessionTurnStreamRequest` carries `run_id`; the production construction site passes the turn's run id
- [ ] `approval-requested`/`approval-resolved` route through `sessionForRun`/`updateTurnForSession`; no unconditional `updateCurrentTurn` in either handler
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0 (incl. `resolution_echoes_request_run_id`)
- [ ] `bun run test` exits 0 (incl. ≥3 new routing tests); `bun run typecheck`, `bunx biome check .`, `bun run clippy` all exit 0
- [ ] Only in-scope files modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The approval handler code at `useChatStream.ts:~929-975` no longer matches the excerpt — e.g., plan 037 or another change restructured `updateCurrentTurn`/the stash machinery. If 037 landed first, rebase the routing onto its per-session buffer shape rather than reverting it.
- `SessionTurnStreamRequest` is constructed somewhere that cannot produce a run id and `Option` is insufficient (e.g., a consumer requires it non-null).
- A second approval constructor path exists outside `BrokerCommandApproval`/`BrokerExternalPathApproval` that carries different identity semantics.
- Any verification fails twice after a reasonable fix attempt.

## Maintenance notes

- The invariant going forward: **every event the backend emits about a run carries `run_id`** so `useChatStream` can route it. When adding new event types, follow `ui_event_payload`'s `runId` convention (snake `run_id` on serde structs).
- Known residual gap, out of scope: approvals for *local-only* sessions (backend `session_id: null`) still carry the run's `run_id` — routing works because `runSessionMapRef` maps runId→frontend session id. If plan 037 hasn't landed, local-only approvals fall back to focused-session behavior (status quo, no regression).
- `resolve_approval` deliberately unchanged: a malicious/buggy FE could resolve any pending id, but the threat model is a single webview; revisit only if multi-window lands.
- Reviewer should scrutinize: the `updateTurnForSession` lazy-turn creation (a background approval can arrive before any text — creating an empty `CurrentTurn` matches the "llm-token with no current turn lazily creates one" precedent in the tests).
