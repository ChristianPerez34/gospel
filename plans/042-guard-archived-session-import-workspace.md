# Plan 042: Prevent Cross-Workspace Session Injection and Overwrite in `import_archived_sessions`

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 04a60d3..HEAD -- src-tauri/src/lib.rs src-tauri/src/session_store.rs`
> If either file changed since this plan was written, compare the "Current
> state" excerpts against the live code before proceeding; on a mismatch,
> treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW — re-importing an archive into the same authorized workspace continues to work; only cross-workspace injection and cross-workspace ID collisions are blocked.
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `04a60d3`, 2026-11-25

## Why this matters

Plan 022 closed cross-workspace authorization gaps on session creation, listing, archiving, and maintenance, but `import_archived_sessions` retained two cross-workspace bypasses:

1. In `src-tauri/src/lib.rs:3277-3296`, `import_archived_sessions` only calls `validate_workspace_id_access` when `workspace_id` is `Some`. When `workspace_id` is `None`, the IPC command passes `None` to `SessionStore::import_archived_sessions`, which falls back to `session.workspace_id` from the untrusted JSON import payload — letting an import write archived sessions into *any* workspace regardless of which workspace is active.
2. Even when `workspace_id` is `Some(active_ws)`, `SessionStore::import_archived_sessions` runs `INSERT INTO archived_sessions ... ON CONFLICT(id) DO UPDATE SET ... workspace_id = excluded.workspace_id` followed by `DELETE FROM archived_session_notes WHERE session_id = ?1`. A colliding `session.id` in the import payload silently hijacks and overwrites an archived session belonging to a *different* workspace and destroys its notes.

## Current state

Relevant files:

- `src-tauri/src/lib.rs` — `import_archived_sessions` Tauri command (lines 3277–3296), `active_workspace_id` / `validate_workspace_id_access` helpers, and `mod session_access` test module.
- `src-tauri/src/session_store.rs` — `SessionStore::import_archived_sessions` (lines 1131–1225) and existing round-trip test `export_import_archived_sessions_round_trips_notes`.
- `src/components/AppShell.tsx` (read-only context — out of scope) — passes `workspaceId: activeWorkspace?.id ?? null` when invoking `import_archived_sessions`, so `None` is a legitimate call in unscoped mode (no active workspace) and `Some(active_id)` is passed when a workspace is active.

Current excerpts:

```rust
// src-tauri/src/lib.rs:3277-3296
#[tauri::command]
fn import_archived_sessions(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    payload: String,
    workspace_id: Option<String>,
) -> Result<Vec<ArchivedSessionRecord>, String> {
    if let Some(ref workspace_id) = workspace_id {
        validate_workspace_id_access(workspace_id, app_config.inner())?;
    }

    match &session_store.store {
        Some(store) => store
            .import_archived_sessions(&payload, workspace_id.as_deref())
            .map_err(|e| e.to_string()),
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}
```

```rust
// src-tauri/src/session_store.rs:1131-1180
    pub fn import_archived_sessions(
        &self,
        payload: &str,
        workspace_id_override: Option<&str>,
    ) -> Result<Vec<ArchivedSessionRecord>, SessionStoreError> {
        let export: ArchivedSessionExport = serde_json::from_str(payload)
            .map_err(|e| SessionStoreError::InvalidStatus(e.to_string()))?;
        let mut imported_ids = Vec::new();
        {
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.transaction()?;
            for session in export.sessions {
                let live_exists = tx
                    .query_row(
                        "SELECT 1 FROM sessions WHERE id = ?1",
                        params![session.id],
                        |_| Ok(()),
                    )
                    .optional()?;
                if live_exists.is_some() {
                    return Err(SessionStoreError::InvalidStatus(format!(
                        "session already exists outside archive: {}",
                        session.id
                    )));
                }

                let target_workspace_id = workspace_id_override
                    .map(str::to_string)
                    .or_else(|| session.workspace_id.clone());
                tx.execute(
                    "INSERT INTO archived_sessions (
                        id, title, provider, model, variant, status, workspace_id,
                        display_transcript, model_history, created_at, updated_at,
                        archived_at, deleted_at
                     )
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL)
                     ON CONFLICT(id) DO UPDATE SET
```

Documented constraints to honor (`CONTEXT.md`):

- *"Workspace-Affine Session: A Session bound to a specific workspace. Only appears in the session list when that workspace is active. Backend rejects attempts to continue a workspace-affine Session from a different active workspace."*
- *"Unscoped Session: A Session with no workspace binding. Appears only in unscoped mode (no active workspace)."*

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Clippy | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | exit 0, no warnings |
| Targeted Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml session_store::tests && cargo test --manifest-path src-tauri/Cargo.toml session_access::` | all pass |
| Full Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all pass |

## Scope

**In scope** (the only files you should modify):
- `src-tauri/src/session_store.rs`
- `src-tauri/src/lib.rs`

**Out of scope** (do NOT touch):
- `src/components/AppShell.tsx` — already sends `workspaceId: activeWorkspace?.id ?? null`.
- `export_archived_sessions` — already validates workspace access on exported IDs.
- Any other session IPC command.

## Git workflow

- Branch: `advisor/042-guard-archived-session-import-workspace`
- Commit style: conventional commits matching `git log` (e.g., `fix(sessions): enforce workspace boundary on archived session import`)
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Enforce strict target-workspace binding and conflict ownership checks in `SessionStore::import_archived_sessions`

In `src-tauri/src/session_store.rs`:

1. Change `target_workspace_id` resolution inside `import_archived_sessions(payload: &str, target_workspace_id: Option<&str>)`:
   - Always bind imported sessions to `target_workspace_id` (`let target_workspace_id = target_workspace_id.map(str::to_string);`), NEVER falling back to `session.workspace_id` from the JSON payload.
   - Rationale: when the caller authorizes workspace `"ws1"`, imported sessions belong to `"ws1"`; when the caller authorizes unscoped mode (`None`), imported sessions become unscoped (`NULL`) rather than injecting into whatever `workspace_id` was written in the JSON file.
2. Before executing the `INSERT INTO archived_sessions ... ON CONFLICT(id) DO UPDATE`, query any existing row in `archived_sessions` with `id = ?1`:
   ```rust
   let existing_archived_ws: Option<Option<String>> = tx
       .query_row(
           "SELECT workspace_id FROM archived_sessions WHERE id = ?1",
           params![session.id],
           |row| row.get::<_, Option<String>>(0),
       )
       .optional()?;
   if let Some(existing_ws) = existing_archived_ws {
       if existing_ws != target_workspace_id {
           return Err(SessionStoreError::InvalidStatus(format!(
               "archived session already exists in a different workspace: {}",
               session.id
           )));
       }
   }
   ```
   - Because this runs inside the existing SQLite transaction (`tx`), a single conflicting session aborts the whole import cleanly without partial writes or note deletion.
   - Re-importing an archive into the *same* workspace (`existing_ws == target_workspace_id`) still succeeds as an idempotent upsert.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml session_store::tests` → all existing tests pass.

### Step 2: Bind `import_archived_sessions` IPC command to the active workspace in `src-tauri/src/lib.rs`

In `src-tauri/src/lib.rs`:

1. Extract a testable core helper `fn import_archived_sessions_checked(store: &SessionStore, app_config: &AppConfigState, payload: &str, workspace_id: Option<&str>) -> Result<Vec<ArchivedSessionRecord>, String>` (matching the pattern used by `mod session_access` in `lib.rs`):
   - Call `validate_optional_workspace_id_access(workspace_id, app_config)?;`
   - Determine the authorized target scope:
     - `let active_ws_id = active_workspace_id(app_config);`
     - If `workspace_id` is `None` AND `active_ws_id` is `Some(_)`, either reject (`return Err("workspace_id is required while a workspace is active".to_string())`) OR bind to `active_ws_id.as_deref()`. **Choose rejecting mismatched scope** (consistent with `validate_workspace_binding` for unscoped vs. workspace-affine sessions in `CONTEXT.md`):
       - Specifically: `let authorized_workspace = match (workspace_id, active_ws_id.as_deref()) { (Some(requested), Some(active)) if requested == active => Some(active), (None, None) => None, (Some(_), _) => return Err(...), (None, Some(_)) => return Err("Cannot import unscoped sessions while a workspace is active".to_string()), };`
       - Wait — double-check `AppShell.tsx:1000-1010`: `AppShell.tsx` ALWAYS passes `workspaceId: activeWorkspace?.id ?? null`, so when a workspace is active it passes `Some(active_id)`, and when no workspace is active it passes `None`. Therefore `(Some(active), Some(active))` and `(None, None)` are the exact two legitimate UI cases, and `(None, Some(_))` is cleanly rejected!
   - Call `store.import_archived_sessions(payload, authorized_workspace).map_err(|e| e.to_string())`.
2. Update the `#[tauri::command] fn import_archived_sessions(...)` handler to delegate directly to `import_archived_sessions_checked`.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml session_access::` → all existing tests pass.

### Step 3: Add regression tests in `session_store.rs` and `lib.rs` (`mod session_access`)

Add the unit tests described in the Test plan, then run Clippy and the full Rust test suite.

**Verify**: `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings && cargo test --manifest-path src-tauri/Cargo.toml` → exit 0, all tests pass.

## Test plan

1. **`src-tauri/src/session_store.rs`** (`mod tests`, modeled after `export_import_archived_sessions_round_trips_notes`):
   - `import_archived_sessions_ignores_payload_workspace_id_when_unscoped`: exporting a session from `"ws1"` and importing it with `target_workspace_id = None` binds the imported session to `workspace_id == None` (not `Some("ws1")`).
   - `import_archived_sessions_rejects_cross_workspace_id_collision_without_deleting_notes`: archive a session `s` with a note in `"ws1"`, export it, leave `s` archived in `"ws1"`, and attempt `import_archived_sessions(&export, Some("ws2"))` — assert `Err(SessionStoreError::InvalidStatus(_))` and assert `s` remains in `"ws1"` with its archived note intact.
   - `import_archived_sessions_allows_same_workspace_reimport`: re-importing `s` into `"ws1"` succeeds idempotently.
2. **`src-tauri/src/lib.rs`** (`mod session_access`, modeled after existing `setup()` tests):
   - `import_archived_sessions_rejects_inactive_workspace_and_unscoped_when_workspace_active`: with `first_workspace` active, `import_archived_sessions_checked(..., Some(&second_workspace))` returns `Err`, and `import_archived_sessions_checked(..., None)` returns `Err`.
   - `import_archived_sessions_allows_active_workspace_and_unscoped_when_no_workspace_active`: succeeds when `workspace_id` matches the active workspace, and succeeds with `None` when no workspace is active (binding imported sessions to `None`, never to payload `workspace_id`).

**Verification**: `cargo test --manifest-path src-tauri/Cargo.toml session_store::tests && cargo test --manifest-path src-tauri/Cargo.toml session_access::` → all pass, including 5 new tests.

## Done criteria

- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0, including the 5 new import-isolation tests
- [ ] `SessionStore::import_archived_sessions` never falls back to `session.workspace_id` from the JSON payload and never overwrites an `archived_sessions` row owned by a different workspace
- [ ] Only in-scope files are modified (`git status`)
- [ ] `plans/README.md` status row for `042` updated to `DONE`

## STOP conditions

Stop and report back (do not improvise) if:

- Any existing test in `session_store.rs` or `lib.rs` expects `import_archived_sessions(&export, None)` to preserve `session.workspace_id` from the payload — check and report before changing test expectations.
- `import_archived_sessions` in `lib.rs` or `session_store.rs` does not match the "Current state" excerpts.
- A step's verification fails twice after a reasonable fix attempt.

## Maintenance notes

- If Gospel later adds a "multi-workspace backup restore" feature, it must require explicit user confirmation and an opt-in multi-workspace restore command rather than loosening single-workspace `import_archived_sessions`.
- Keep checking `archived_sessions` ownership before any `ON CONFLICT(id) DO UPDATE` query added to `session_store.rs`.
