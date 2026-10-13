# Plan 036: Guard Session Archive/Delete/Maintenance Against In-Flight Turns

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 2982ea5..HEAD -- src-tauri/src/lib.rs src-tauri/src/session_store.rs`
> If either file changed since this plan was written, compare the "Current state"
> excerpts against the live code before proceeding; on a mismatch, treat it as
> a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED — touches the session persistence boundary; mitigated by refusing/skipping rather than changing persistence semantics
- **Depends on**: none
- **Category**: bug (silent data loss)
- **Planned at**: commit `2982ea5`, 2026-10-10

## Why this matters

A session can be removed while its streaming turn is still running, and the completed turn's output is then silently discarded. Three paths do it today:

1. `list_sessions` — a pure read command — runs `clean_stale_drafts()` + `run_archive_maintenance()` on **every** call (`lib.rs:~2413-2419`), so routine SessionDrawer refreshes trigger archival.
2. `archive_session` / `delete_session` IPC commands take no in-flight-turn check — `validate_session_access` covers workspace authorization only.
3. `run_archive_maintenance` archives any session whose `updated_at` is older than `auto_archive_hours` — a long-running turn on an otherwise-idle session qualifies.

When the in-flight turn then finishes, `persist_turn` hits `SessionStoreError::NotFound` and the failure path is only `tracing::warn!` (`session_turn.rs:~477-483`) — the turn's transcript and model history are lost with no error surfaced, and the in-memory conversation keeps going from stale history.

The fix has two shapes: user-initiated archive/delete of an in-flight session **refuses** with a clear error (the user can cancel the run or wait), while bulk maintenance/draft-sweeps **skip** in-flight session ids (they'll be handled on a later pass).

## Current state

`src-tauri/src/lib.rs` — `StreamingRunHandles` (~lines 120–167) is a `tauri::State`-managed map from session id to `(run_id, AbortHandle)`. It currently has **no read methods** — only mutation:

```rust
pub struct StreamingRunHandles {
    inner: Mutex<HashMap<String, (String, futures::future::AbortHandle)>>,
}

impl StreamingRunHandles {
    fn new() -> Self { ... }
    fn insert(&self, session_id: &str, run_id: &str, handle: futures::future::AbortHandle) { ... }
    fn cancel(&self, session_id: &str) { ... }
    /// Remove the handle only if it still belongs to the completing run.
    fn remove_if_run_matches(&self, session_id: &str, run_id: &str) { ... }
}
```

Lock convention in this struct: `.inner.lock().expect("streaming handles poisoned")`.

The commands that need guards (all sync `#[tauri::command]` fns):

- `list_sessions` (~lines 2405–2425) — maintenance on every call:

```rust
    match &session_store.store {
        Some(store) => {
            // Clean up stale drafts on list
            let _ = store.clean_stale_drafts();
            let _ = store.run_archive_maintenance(workspace_id.as_deref());
            store
                .list_sessions_for_workspace(workspace_id.as_deref())
                .map_err(|e| e.to_string())
        }
```

- `archive_session` (~lines 2556–2574) — already takes `app_config: tauri::State<'_, AppConfigState>` and calls `validate_session_access(store, &session_id, app_config.inner())?;` before `store.archive_session(&session_id)`.
- `delete_session_cmd` (~lines 2704–2718) — same shape: `validate_session_access` then `store.delete_session(&session_id)`.
- `run_archive_maintenance` command (~lines 2539–2553) — calls `validate_optional_workspace_id_access` then `store.run_archive_maintenance(workspace_id.as_deref())`.
- `cleanup_stale_drafts` command (~lines 2721–2730) — calls `store.clean_stale_drafts()`.
- `cancel_streaming` (~lines 1434–1441) — the injection pattern to copy: `streaming_handles: tauri::State<'_, StreamingRunHandles>`.

`src-tauri/src/session_store.rs` (~1,950 LOC) — the store methods:

```rust
    pub fn archive_sessions_older_than_hours(
        &self,
        workspace_id: Option<&str>,
        hours: i64,
    ) -> Result<Vec<ArchivedSessionRecord>, SessionStoreError> {
        if hours <= 0 {
            return Ok(Vec::new());
        }

        let ids = {
            let conn = self.conn.lock().unwrap();
            // SELECT id FROM sessions WHERE status != 'draft' AND updated_at < ... 
            //   ORDER BY updated_at ASC → collect Vec<String>
        };

        self.archive_sessions_by_ids(&ids)
    }
```

The `ids` `Vec<String>` is materialized before `archive_sessions_by_ids` runs — inserting a `retain` there is a one-line change.

```rust
    pub fn clean_stale_drafts(&self) -> Result<usize, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM sessions WHERE status = 'draft' AND display_transcript = '[]'
             AND updated_at < datetime('now', '-1 hour')",
            params![],
        )?;
        Ok(rows)
    }
```

`clean_stale_drafts` is a single predicate DELETE — a drafting session mid-first-turn has `status = 'draft'` and can be an in-flight target, so it needs the same exclusion (a draft's id IS in `StreamingRunHandles` once its stream starts).

```rust
    pub fn run_archive_maintenance(
        &self,
        workspace_id: Option<&str>,
    ) -> Result<ArchiveMaintenanceResult, SessionStoreError> {
        let policy = self.get_archive_policy(workspace_id)?;
        let archived_count = self
            .archive_sessions_older_than_hours(workspace_id, policy.auto_archive_hours)?
            .len();
        let deleted_count =
            self.delete_archived_sessions_older_than_days(workspace_id, policy.retention_days)?;
        ...
    }
```

`delete_archived_sessions_older_than_days` touches only `archived_sessions` — an archived session can't have an in-flight turn, so it needs no exclusion.

**Relevant CONTEXT.md vocabulary** (use these terms in names/comments): "Session", "Draft Session" (a session created on first send, `status='draft'`), "Display Transcript"/"Model History" (the payloads that get lost).

**Test conventions**: `session_store.rs` tests are in the same file's `#[cfg(test)] mod tests` — model on `archive_maintenance_archives_old_live_and_deletes_expired_archives` (~line 1782), `delete_session_removes_record` (~line 1619), `archive_rejects_draft_sessions` (~line 1676). They build an in-memory/temp-store; follow their setup helper (e.g. `set_archived_at` at ~line 1344 fabricates aged rows).

## Commands you will need

| Purpose   | Command                                              | Expected on success |
|-----------|------------------------------------------------------|---------------------|
| Rust tests (module) | `cargo test --manifest-path src-tauri/Cargo.toml session_store` | all pass |
| Rust tests (lib)    | `cargo test --manifest-path src-tauri/Cargo.toml streaming`     | pass (handle tests) |
| Full Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all pass |
| Typecheck   | `bun run typecheck`                                  | exit 0 (unchanged)  |
| Clippy    | `bun run clippy`                                     | exit 0              |

## Scope

**In scope** (the only files you should modify):
- `src-tauri/src/lib.rs` — `StreamingRunHandles` read methods + guard/signature threading in the five commands
- `src-tauri/src/session_store.rs` — exclusion params on `archive_sessions_older_than_hours`, `clean_stale_drafts`, `run_archive_maintenance` + tests

**Out of scope**:
- `src-tauri/src/session_turn.rs` — the `persist_turn`-failure `warn!` stays; with the guard, `NotFound` there becomes truly exceptional. Do not change the warn-into-ok persistence contract in this plan.
- `restore_archived_session` — an archived session cannot have an in-flight turn (the run would already have failed); no guard needed.
- `delete_archived_sessions_older_than_days` / archived-session commands — archived rows have no runs.
- Throttling maintenance off the per-`list_sessions` call — deliberately out of scope (a separately deferred perf item; the guard is what this plan fixes).
- Frontend changes — none needed; the new refusal error surfaces through the existing `Result<_, String>` IPC error path.

## Git workflow

- Branch: `advisor/036-inflight-session-guard`
- Commit style: conventional commits, e.g. `fix(sessions): refuse archive/delete while a turn is in flight`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add read methods to `StreamingRunHandles`

In `src-tauri/src/lib.rs`, next to `remove_if_run_matches`:

```rust
    /// True while a streaming turn is registered for the session id.
    fn contains(&self, session_id: &str) -> bool {
        self.inner
            .lock()
            .expect("streaming handles poisoned")
            .contains_key(session_id)
    }

    /// Snapshot of session ids with an in-flight streaming turn. Callers use
    /// this to exclude live sessions from bulk mutations like maintenance.
    fn in_flight_session_ids(&self) -> std::collections::HashSet<String> {
        self.inner
            .lock()
            .expect("streaming handles poisoned")
            .keys()
            .cloned()
            .collect()
    }
```

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml` → all pass (additive).

### Step 2: Refuse user-initiated archive/delete while in-flight

In `archive_session` and `delete_session_cmd` (lib.rs ~2556/~2704): add the `streaming_handles: tauri::State<'_, StreamingRunHandles>` parameter (copy `cancel_streaming`'s injection at ~1434), and after `validate_session_access` add:

```rust
            if streaming_handles.contains(&session_id) {
                return Err(
                    "session has an in-flight turn; cancel it or wait for it to finish"
                        .to_string(),
                );
            }
```

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml` → pass; `bun run clippy` → 0.

### Step 3: Exclusion params on the store's bulk paths

In `session_store.rs` (import `std::collections::HashSet` if absent):

- `pub fn archive_sessions_older_than_hours(&self, workspace_id: Option<&str>, hours: i64, exclude: &HashSet<String>)` — after collecting `ids`, before `archive_sessions_by_ids`: `ids.retain(|id| !exclude.contains(id));`
- `pub fn run_archive_maintenance(&self, workspace_id: Option<&str>, exclude: &HashSet<String>)` — pass `exclude` through to `archive_sessions_older_than_hours`; `delete_archived_sessions_older_than_days` unchanged.
- `pub fn clean_stale_drafts(&self, exclude: &HashSet<String>)` — replace the single predicate DELETE with: select stale draft ids under one lock, `retain` against `exclude`, then delete the survivors by id (loop `conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])` inside the same lock guard; a transaction is optional for parity — keep it simple, per-id deletes). Return the deleted count.

Update ALL callers — run `grep -rn "clean_stale_drafts\|run_archive_maintenance\|archive_sessions_older_than_hours" src-tauri/src/` and thread `&streaming_handles.in_flight_session_ids()` (commands) or `&HashSet::new()` (tests/other callers without handles) appropriately:

- `list_sessions` (~2413): inject `streaming_handles: tauri::State<'_, StreamingRunHandles>`, compute `let in_flight = streaming_handles.in_flight_session_ids();` once, pass `&in_flight` to both calls.
- `run_archive_maintenance` command (~2539): same injection + pass.
- `cleanup_stale_drafts` command (~2721): same injection + pass.
- Any other callers the grep finds (e.g., startup tasks, `session_turn` internals): same treatment; if a caller genuinely has no access to Tauri state, pass `&HashSet::new()` and note it.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml session_store` → pass.

### Step 4: Tests

In `session_store.rs`'s test module (model on `archive_maintenance_archives_old_live_and_deletes_expired_archives` ~1782 and `set_archived_at` ~1344):

- `archive_maintenance_skips_excluded_in_flight_ids` — two aged sessions, exclude one → only the other is archived.
- `clean_stale_drafts_skips_excluded_in_flight_ids` — two stale drafts, exclude one → only the other is deleted.
- `archive_sessions_older_than_hours_excludes_ids` — direct unit test of the retain.
- In `lib.rs` tests (or wherever `StreamingRunHandles` tests live — check for an existing test module near the struct): `streaming_handles_snapshot_lists_in_flight_sessions` — insert two handles, assert `in_flight_session_ids()` and `contains()` results.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml` → all pass incl. new tests; `bun run clippy` → 0.

## Test plan

Covered by Step 4 — store-level exclusion tests plus a handles-snapshot test. IPC-command-level testing of the guard isn't practical (Tauri `State` injection); the guard is two lines reading from a tested primitive — note this honestly in the PR description.

Verification: `cargo test --manifest-path src-tauri/Cargo.toml` → all pass.

## Done criteria

- [ ] `StreamingRunHandles` exposes `contains` + `in_flight_session_ids`
- [ ] `archive_session` and `delete_session_cmd` refuse in-flight sessions with the specified error message
- [ ] `archive_sessions_older_than_hours`, `run_archive_maintenance`, `clean_stale_drafts` accept and honor an exclusion set; all callers updated
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0; ≥4 new tests pass
- [ ] `bun run clippy` exits 0
- [ ] Only the two in-scope files modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- `StreamingRunHandles` has been restructured (e.g., run-id-keyed map without session keys) — the in-flight check must then be re-deriving session ids differently; report the new shape.
- A caller of the three store methods lives somewhere with no access to `StreamingRunHandles` state and an empty exclusion is wrong (e.g., a background maintenance task that runs while turns are active) — report rather than passing `HashSet::new()` blindly.
- `list_sessions` or the archive commands were refactored (the excerpts above no longer match).
- The draft-cleanup rewrite in Step 3 changes semantics for non-excluded rows — e.g., the predicate differs; report the difference.

## Maintenance notes

- The invariant: **bulk or destructive session mutations must consult `StreamingRunHandles`** (or whatever replaces it) before removing a row from `sessions`. Any future session-delete surface (bulk delete, workspace teardown, retention sweeps) must join the same guard.
- `persist_turn` failing `NotFound` mid-flight can still happen for a session deleted *between* the handle's removal (`remove_if_run_matches` at turn end) and persist — the window is now the normal persist path, not user/maintenance action. A reviewer should confirm no residual warn-spam.
- Deliberately not done: throttling `run_archive_maintenance`/`clean_stale_drafts` off every `list_sessions` call (perf finding, separately deferred); a turn that outlives `auto_archive_hours` still can't be auto-archived only because it's in-flight — after the turn completes and `updated_at` is old, the next maintenance pass archives it normally (intended).
