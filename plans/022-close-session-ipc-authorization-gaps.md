# Plan 022: Close Cross-Workspace Session IPC Authorization Gaps

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer dispatched you and told you they maintain
> the index.
>
> **Drift check (run first)**: `git diff --stat 0ec1edd..HEAD -- src-tauri/src/lib.rs`
> If `src-tauri/src/lib.rs` changed since this plan was written, compare the
> current command bodies and authorization helpers against the excerpts below;
> on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `0ec1edd`, 2026-07-31

## Why this matters

Gospel uses the active workspace as the authorization boundary for workspace-
affine sessions. Several Tauri commands already enforce that boundary, but
neighboring commands accept caller-supplied workspace IDs or session IDs and
call the store directly. A renderer caller that knows another workspace or
session ID can therefore list records or mutate session metadata outside the
active workspace. This plan makes the command layer consistently enforce the
existing policy without changing the session schema or store queries.

## Current state

Relevant files:

- `src-tauri/src/lib.rs` — Tauri IPC command definitions, active-workspace
  lookup, and existing session/archive authorization helpers.
- `src-tauri/src/session_store.rs` — persistence operations and the canonical
  workspace-binding checks used by `validate_session_access`.
- `src-tauri/src/lib.rs:3121-3208` — existing in-memory AppConfig/SessionStore
  test setup exemplar.

The command layer currently accepts an arbitrary workspace when creating a
session:

```rust
// src-tauri/src/lib.rs:2122-2148
fn create_session(..., workspace_id: Option<String>, ...) -> Result<SessionRecord, String> {
    ...
    store.create_session_with_selection(..., workspace_id.as_deref(), ...)
}
```

The three session metadata mutators do not validate the session's workspace:

```rust
// src-tauri/src/lib.rs:2151-2201
fn update_session_model_selection(..., session_id: String, ...) -> Result<(), String> {
    store.update_model_selection(&session_id, ...)
}

fn update_session_mode(..., session_id: String, mode: String) -> Result<(), String> {
    store.update_session_mode(&session_id, &mode)
}

fn update_session_title(..., session_id: String, title: String) -> Result<(), String> {
    store.update_session_title(&session_id, &title)
}
```

The list and archive-policy commands also pass caller-provided workspace IDs
through without checking them:

```rust
// src-tauri/src/lib.rs:2252-2320
fn list_sessions(..., workspace_id: Option<String>) -> Result<Vec<SessionRecord>, String> {
    ...
    store.list_sessions_for_workspace(workspace_id.as_deref())
}

fn list_archived_sessions(..., workspace_id: Option<String>) -> Result<Vec<ArchivedSessionRecord>, String> {
    ...
    store.list_archived_sessions_for_workspace(workspace_id.as_deref())
}

fn get_archive_policy(..., workspace_id: Option<String>) -> Result<ArchivePolicy, String> {
    store.get_archive_policy(workspace_id.as_deref())
}
```

Additional workspace-scoped command bodies at `src-tauri/src/lib.rs:2370-2393`
(`get_archive_stats` and `run_archive_maintenance`) and
`src-tauri/src/lib.rs:2682-2689` (`get_workspace_session_count`) have the same
shape. `import_archived_sessions`, `set_archive_policy`, and
`clear_workspace_archive_policy` already validate explicit workspace IDs.

The existing guard is:

```rust
// src-tauri/src/lib.rs:2773-2797
fn validate_workspace_id_access(workspace_id: &str, app_config: &AppConfigState) -> Result<(), String> {
    let active_ws_id = active_workspace_id(app_config)
        .ok_or_else(|| "No active workspace is available".to_string())?;
    if workspace_id != active_ws_id { ... }
    Ok(())
}

fn validate_session_access(store: &SessionStore, session_id: &str, app_config: &AppConfigState) -> Result<(), String> {
    let active_ws_id = active_workspace_id(app_config);
    store.validate_workspace_binding(session_id, active_ws_id.as_deref()).map_err(|e| e.to_string())
}
```

`SessionStore::validate_workspace_binding` intentionally permits an unscoped
session only when no workspace is active and permits a workspace-affine session
only when its workspace equals the active workspace. Preserve that behavior;
do not weaken it to an existence-only check.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Backend access tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml -- session_access` | all new authorization tests pass |
| Backend full tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml` | exit 0 |
| Backend lint | `cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` | exit 0 |
| Frontend typecheck | `bun run typecheck` | exit 0, no errors |

## Scope

**In scope** (the only files to modify):

- `src-tauri/src/lib.rs` — add the narrow helper(s), apply guards to every
  session/archive command identified below, and add Rust unit tests.

**Out of scope** (do not touch):

- `src-tauri/src/session_store.rs` — its binding semantics are the policy this
  plan consumes, not a target for redesign.
- SQLite schema, migrations, session DTOs, or frontend command call sites.
- `complete_streaming` and `cancel_streaming` lifecycle behavior; streaming
  already validates session binding through the `SessionTurnSessions` adapter.
- Global operations with no caller-supplied workspace scope, such as
  `cleanup_stale_drafts`; document the reason in code only if needed to make
  the authorization audit clear.

## Git workflow

- Branch: `advisor/022-close-session-ipc-authorization`
- Use the repository's existing conventional commit style, for example
  `fix: enforce workspace authorization on session commands`.
- Do not push or open a PR unless explicitly instructed.

## Steps

### Step 1: Enumerate the command authorization matrix

Before editing, inspect every `#[tauri::command]` in `src-tauri/src/lib.rs`
that accepts `workspace_id`, `session_id`, `session_ids`, or a note ID. Write a
small matrix in the test module or as a succinct comment only if it is useful:

- Session ID commands must call `validate_session_access` before reading or
  mutating the record.
- Archived session ID commands must call
  `validate_archived_session_access` before reading or mutating the record.
- An explicit workspace ID must call `validate_workspace_id_access` before a
  workspace-scoped read or mutation.
- `None` means the existing unscoped/global behavior and must not be converted
  into an arbitrary workspace.

At minimum, cover `create_session`, the three session update commands,
`list_sessions`, `list_archived_sessions`, `get_archive_policy`,
`get_archive_stats`, `run_archive_maintenance`,
`delete_expired_archived_sessions`, and `get_workspace_session_count`.

**Verify**: `rg -n "workspace_id|session_id|session_ids|validate_.*access" src-tauri/src/lib.rs` -> every listed command and existing guard is visible for the next step.

### Step 2: Apply the existing access policy at every command boundary

Add only the smallest helper needed to avoid repeating the explicit optional
workspace check. It must call `validate_workspace_id_access` when the option is
`Some` and preserve `None` unchanged.

Apply `validate_session_access` before each session metadata update. In
`create_session`, validate `workspace_id` before calling the store. For list,
archive-policy, archive-maintenance, expiry, stats, and count commands,
validate any explicit workspace ID before the store call. Keep the current
active-workspace fallback used by `list_sessions` and
`list_archived_sessions`; validate the caller's explicit value before that
fallback is used.

Do not rely on the frontend to send the active workspace. Do not turn a
cross-workspace request into a successful empty list; return the same existing
authorization error shape used by `validate_workspace_id_access` and
`validate_session_access`.

**Verify**: `cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` -> exit 0.

### Step 3: Add cross-workspace and unscoped regression tests

Add a `#[cfg(test)]` module named `session_access` in `src-tauri/src/lib.rs`,
following the in-memory setup style in
`workspace_response_tests` (`src-tauri/src/lib.rs:3121-3208`). Create two
temporary workspace records, set the first active, and create sessions bound
to both workspaces plus one unscoped session.

Test at least:

- Explicit second-workspace IDs are rejected by the optional workspace helper.
- The active workspace ID is accepted.
- A session bound to the second workspace is rejected by
  `validate_session_access` while the first is active.
- A session bound to the first workspace is accepted.
- An unscoped session is rejected while a workspace is active and accepted
  when the active workspace is cleared.
- The command authorization matrix does not leave a workspace-scoped command
  without a guard. Prefer direct helper tests plus command-level tests where
  the Tauri `State` wrapper is practical; do not create a new integration
  harness just for this plan.

Use synthetic workspace IDs and paths only. Do not read or write the real
workspace or app data directory.

**Verify**: `cargo test --locked --manifest-path src-tauri/Cargo.toml -- session_access` -> all new tests pass.

### Step 4: Run the full verification gates

Review the diff and confirm every in-scope command has a guard before running
the full suite. Preserve unrelated worktree changes if any appear; do not
reset them.

**Verify**: `cargo test --locked --manifest-path src-tauri/Cargo.toml` -> exit 0; `cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` -> exit 0; `bun run typecheck` -> exit 0.

## Test plan

- Add the `session_access` test module in `src-tauri/src/lib.rs`.
- Cover active, inactive, and unscoped workspace bindings.
- Cover explicit workspace IDs on reads and mutations, not only the existing
  `get_session` path.
- Model the setup on `workspace_response_tests` in the same file and on
  `SessionStore` binding tests at `src-tauri/src/session_store.rs:1649-1660`.
- Verification: `cargo test --locked --manifest-path src-tauri/Cargo.toml` -> all backend tests pass.

## Done criteria

- [ ] `create_session` rejects an explicit workspace ID that is not active.
- [ ] Session model/mode/title updates validate the target session's workspace.
- [ ] Session and archive list/policy/stats/maintenance/count commands validate
      explicit workspace IDs.
- [ ] Existing active-workspace and unscoped semantics remain unchanged.
- [ ] `cargo test --locked --manifest-path src-tauri/Cargo.toml` exits 0.
- [ ] `cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0.
- [ ] `bun run typecheck` exits 0.
- [ ] No files outside `src-tauri/src/lib.rs` are modified.
- [ ] `plans/README.md` status row for plan 022 is updated.

## STOP conditions

Stop and report instead of improvising if:

- The current command bodies or helper semantics do not match the excerpts,
  or a command has moved to another file.
- A correct guard requires changing `SessionStore` queries or the schema.
- The active workspace policy is found to intentionally allow cross-workspace
  session access; record the documented decision and stop rather than adding a
  conflicting restriction.
- A command with `workspace_id: None` is intended to mean “all workspaces”
  rather than global/unscoped scope and no existing policy documents that
  behavior.
- Any test needs real app data, credentials, network access, or a Tauri window.

## Maintenance notes

- Any new Tauri command that accepts a session or workspace identifier must join
  this authorization matrix before it calls `SessionStore`.
- Reviewers should check the guard occurs before the store read/write, not only
  after a record has already been loaded.
- Keep `SessionTurnSessions::validate_workspace_binding` and the command-layer
  `validate_session_access` aligned; a future session command must not rely on
  only one of those paths.
- This plan intentionally does not introduce authenticated user identities;
  it hardens the existing local renderer-to-backend workspace boundary.
