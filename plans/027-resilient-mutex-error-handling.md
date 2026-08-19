# Plan 027: Resilient Mutex Lock Error Handling in AppConfigStore

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md` — unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 9df8439..HEAD -- src-tauri/src/app_config.rs`
> If any in-scope file changed since this plan was written, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `9df8439`, 2026-08-02

## Why this matters

`AppConfigStore` manages application-global configuration settings stored in SQLite. It wraps its database connection in `std::sync::Mutex<rusqlite::Connection>`. Currently, methods throughout `app_config.rs` call `self.conn.lock().unwrap()`. If a thread panics while holding the lock guard, standard library Mutexes become *poisoned*. Subsequent calls to `.lock().unwrap()` will panic immediately across all threads. This permanently breaks all configuration IPC commands and crashes the Tauri desktop process.

Replacing `.lock().unwrap()` with a resilient helper method (`lock_conn`) that handles lock acquisition cleanly (either recovering the poisoned guard via `unwrap_or_else(|e| e.into_inner())` or returning a typed `AppConfigError::DatabaseError`) eliminates single-panic process cascading failures.

## Current state

- File: [`src-tauri/src/app_config.rs`](file:///Users/lifelinelogics/RustProjects/gospel/src-tauri/src/app_config.rs) — manages app config in SQLite (lines 140–250).
- Excerpt from `src-tauri/src/app_config.rs`:
```rust
pub fn provider_visibility(&self, provider: &str) -> Result<bool, AppConfigError> {
    validate_provider(provider)?;
    let conn = self.conn.lock().unwrap();
    let visible = conn
        .query_row(
            "SELECT visible FROM provider_settings WHERE provider_id = ?1",
            params![provider],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    Ok(visible.map(|v| v != 0).unwrap_or(true))
}

pub fn set_provider_visibility(
    &self,
    provider: &str,
    visible: bool,
) -> Result<(), AppConfigError> {
    validate_provider(provider)?;
    let conn = self.conn.lock().unwrap();
    // ...
```
- Conventions: Custom error type `AppConfigError` exists in `app_config.rs` with `DatabaseError(#[from] rusqlite::Error)` variant.

## Commands you will need

| Purpose   | Command                                               | Expected on success |
|-----------|-------------------------------------------------------|---------------------|
| Check Rust| `cargo check --manifest-path src-tauri/Cargo.toml`    | exit 0, no errors   |
| Clippy    | `bun run clippy`                                      | exit 0, clean       |
| Rust test | `cargo test --manifest-path src-tauri/Cargo.toml -- lib::app_config` | all pass |
| Full tests| `cargo test --manifest-path src-tauri/Cargo.toml`    | 454+ tests pass     |

## Scope

**In scope**:
- `src-tauri/src/app_config.rs`

**Out of scope**:
- Any other SQLite database module (`session_store.rs`, `context_search.rs`).
- Public API signatures of `AppConfigStore` methods (return type `Result<T, AppConfigError>` must remain identical).

## Git workflow

- Branch: `advisor/027-resilient-mutex-error-handling`
- Commit message style: `fix(app_config): replace panicking Mutex unwrap with resilient lock helper`

## Steps

### Step 1: Add resilient `lock_conn` helper to `AppConfigStore`

In `src-tauri/src/app_config.rs`, implement a private helper method on `AppConfigStore`:

```rust
fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, AppConfigError> {
    self.conn
        .lock()
        .map_err(|e| AppConfigError::DatabaseError(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_INTERNAL),
            Some(format!("AppConfigStore mutex poisoned: {}", e)),
        )))
}
```
Or recover the lock guard directly via `unwrap_or_else(|e| e.into_inner())` if poisoned state still contains a valid `Connection`.

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` → exits 0.

### Step 2: Replace all `self.conn.lock().unwrap()` call-sites with `self.lock_conn()?`

In `src-tauri/src/app_config.rs`, locate all occurrences of `self.conn.lock().unwrap()` (~20 occurrences across methods like `provider_visibility`, `set_provider_visibility`, `get_session_mode_preference`, `set_session_mode_preference`, `list_provider_settings`, etc.) and replace them with `let conn = self.lock_conn()?;` or `let mut conn = self.lock_conn()?;`.

**Verify**: `bun run clippy` → exits 0 without warnings.

### Step 3: Add unit test for lock recovery / error safety

In `src-tauri/src/app_config.rs` under `mod tests`, write a unit test `test_poisoned_mutex_handling` verifying that `AppConfigStore` does not panic even if a lock was previously acquired during a thread panic.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml -- lib::app_config` → all tests pass.

## Test plan

- Unit test `test_poisoned_mutex_handling`:
  - Create an `AppConfigStore::in_memory_for_test()`.
  - Intentionally poison the mutex in a spawned thread (`std::thread::spawn`).
  - Call `store.provider_visibility("openai")` from the main thread.
  - Assert that it returns an `Ok` or `Err(AppConfigError)` instead of panicking.

## Done criteria

- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` exits 0
- [ ] `bun run clippy` exits 0 with zero warnings
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` passes all tests
- [ ] Zero occurrences of `conn.lock().unwrap()` remaining in `src-tauri/src/app_config.rs`
- [ ] `plans/README.md` status row updated

## STOP conditions

- If `self.conn` is not a `std::sync::Mutex<Connection>`, stop and report.
- If replacing `.unwrap()` breaks public method signatures, stop and report.

## Maintenance notes

- When adding new methods to `AppConfigStore` in the future, always use `self.lock_conn()?` instead of calling `.lock().unwrap()`.
