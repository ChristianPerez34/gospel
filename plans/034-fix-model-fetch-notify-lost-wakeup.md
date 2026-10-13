# Plan 034: Fix the Model-Fetch Waiter Lost-Wakeup

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 2982ea5..HEAD -- src-tauri/src/models.rs`
> If this file changed since this plan was written, compare the "Current state"
> excerpts against the live code before proceeding; on a mismatch, treat it as
> a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `2982ea5`, 2026-10-10

## Why this matters

`models.rs`'s `get_or_fetch` deduplicates concurrent model-list fetches: the first caller ("leader") fetches; later callers ("waiters") block on a shared `tokio::sync::Notify` and then read the populated cache. The wakeup is race-prone in a way that can hang the waiter **permanently**: `Notify::notify_waiters()` only wakes tasks that have already registered interest (first poll or `enable()`), and stores no permit for latecomers. A waiter that drops the `PENDING_REQUESTS` write lock but hasn't yet polled `notified()` when the leader calls `notify_waiters()` is never woken — and `notify_waiters()` also requires at least one registered waiter, so the notification is simply dropped.

The user-visible symptom: `build_model_availability` / model-picker refresh (`src-tauri/src/lib.rs` `build_model_availability`) hangs forever for that caller — the IPC promise never resolves and the models panel spins indefinitely. It needs only an unlucky interleave on the multi-threaded runtime: two requests for the same provider+key where the leader finishes between the waiter's map read and its first poll.

## Current state

`src-tauri/src/models.rs` (~1,300 LOC) owns the model registry, per-provider fetch, and the shared fetch cache.

Globals at lines ~453–457:

```rust
pub static MODEL_CACHE: LazyLock<Arc<RwLock<HashMap<String, CachedModelList>>>> =
    ...
pub static PENDING_REQUESTS: LazyLock<PendingMap> =
    ...
```

The waiter/leader logic in `get_or_fetch`, lines ~709–755:

```rust
        // Register/check pending request atomically to avoid races and duplicate fetches
        let (is_waiter, notify, must_rejoin) = {
            let mut pending = PENDING_REQUESTS.write().await;
            if let Some(notify) = pending.get(cache_key) {
                (true, notify.clone(), force_refresh)
            } else {
                let notify = Arc::new(Notify::new());
                pending.insert(cache_key.to_string(), notify.clone());
                (false, notify, false)
            }
        };

        if is_waiter {
            notify.notified().await;                    // ← lost wakeup window
            if must_rejoin {
                drop(notify);
                return Self::fetch_and_cache_impl(cache_key, provider, fetch_fn).await;
            }
            let cache = MODEL_CACHE.read().await;
            if let Some(entry) = cache.get(cache_key) {
                return ModelInfoWithFreshness { ... };  // fresh path
            }
            return ModelInfoWithFreshness { ... "fetch_failed" ... };
        }

        let result = Self::fetch_and_cache_impl(cache_key, provider, fetch_fn).await;

        // Notify waiters and clean up
        {
            let mut pending = PENDING_REQUESTS.write().await;
            pending.remove(cache_key);
        }
        notify.notify_waiters();                        // ← only wakes already-registered waiters

        result
```

The window: the waiter clones the `Arc<Notify>` at the `.get(cache_key)` line, the block ends and the `RwLock` write guard drops, then — one or more executor yields later — `notify.notified().await` creates **and first-polls** the `Notified` future. `Notified` registers interest only on first poll (or explicit `enable()`). If the leader completes and calls `notify_waiters()` in that gap, nobody is registered → the wake is lost → the waiter waits forever. `notify_waiters` (unlike `notify_one`) also does not store a permit.

**Fix shape** (do this, don't redesign): create the `Notified` future and call `.enable()` on it **while still inside the write-lock block**, so registration happens-before the guard releases. `tokio::sync::futures::Notified::enable()` registers the future in the notify list without polling — after that, a `notify_waiters()` stores a permit for it even before it is `.await`ed. `Notified` is `!Unpin`, so box-pin it to move it out of the block:

```rust
        let (is_waiter, notify, must_rejoin, wait) = {
            let mut pending = PENDING_REQUESTS.write().await;
            if let Some(notify) = pending.get(cache_key) {
                let notify = notify.clone();
                let mut wait = Box::pin(notify.notified());
                wait.as_mut().enable();
                (true, notify, force_refresh, Some(wait))
            } else {
                let notify = Arc::new(Notify::new());
                pending.insert(cache_key.to_string(), notify.clone());
                (false, notify, false, None)
            }
        };

        if is_waiter {
            wait.expect("waiter future is registered while holding the lock").await;
            if must_rejoin {
                drop(notify);
                return Self::fetch_and_cache_impl(cache_key, provider, fetch_fn).await;
            }
            // ... unchanged cache-read tail
        }
```

The pinned `Notified` borrows `notify` (the `Arc` clone held in the same tuple), so `wait` must be awaited while `notify` is still in scope — the code above already does that.

**Test conventions**: this file's tests sit in `#[cfg(test)] mod tests` at the bottom; `test_get_or_fetch_waits_for_inflight_request` (~line 1281) is the existing concurrent-wait test — model the new test on it. Rust async tests use `#[tokio::test]`. Wrap any wait path in `tokio::time::timeout` so a regression fails as a timeout, not a CI hang. Check how existing tests reset `MODEL_CACHE`/`PENDING_REQUESTS` between cases (they are shared `static`s — reuse whatever isolation pattern the neighboring tests use; do not invent a new reset helper if one exists).

## Commands you will need

| Purpose   | Command                                              | Expected on success |
|-----------|------------------------------------------------------|---------------------|
| Rust tests (module) | `cargo test --manifest-path src-tauri/Cargo.toml models` | all tests pass |
| Full Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all pass |
| Clippy    | `bun run clippy`                                     | exit 0, no warnings |

## Scope

**In scope**:
- `src-tauri/src/models.rs` — the waiter-registration block in `get_or_fetch` + tests

**Out of scope**:
- `PENDING_REQUESTS`/`MODEL_CACHE` types and TTL logic (`GOSPEL_MODEL_CACHE_TTL_SECONDS`) — unchanged.
- `fetch_and_cache_impl`, provider registry, `build_model_availability` call sites in `lib.rs` — unchanged.
- Do NOT replace `Notify` with a `watch`/`broadcast` channel — a defensible bigger refactor, but this plan intentionally takes the minimal registration fix; the channel approach is recorded as an alternative if `enable()` proves awkward (see STOP conditions).
- Adding a timeout around the wait — deliberately not in this plan; the hang is fixed at the source.

## Git workflow

- Branch: `advisor/034-model-fetch-notify-wakeup`
- Commit style: conventional commits, e.g. `fix(models): register notified future before releasing pending lock`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Register the `Notified` inside the write-lock block

Apply the fix shape from "Current state": change the destructure tuple to carry `Option<Pin<Box<Notified<'_>>>>` (via `Box::pin` + `enable()` inside the lock block) and await it in the `is_waiter` branch. Keep everything else identical — `must_rejoin` rejoin behavior, the cache-read tail, the leader's remove-then-`notify_waiters` tail.

Careful with lifetimes: `notify.notified()` borrows through the `Arc<Notify>` clone stored in `notify`; the `Box::pin`'d future is only valid while `notify` lives — the existing code shape already keeps both alive in the same scope.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml models` → all existing tests pass, including `test_get_or_fetch_waits_for_inflight_request`.

### Step 2: Add the regression test

New `#[tokio::test]` modeled on `test_get_or_fetch_waits_for_inflight_request`:

- `waiter_wakes_when_leader_finished_before_first_poll`: orchestrate the exact lost-wakeup interleave —
  1. Clear the relevant `MODEL_CACHE` and `PENDING_REQUESTS` state per the module's existing test-isolation pattern.
  2. Pre-insert a `Notify` into `PENDING_REQUESTS` under a fresh cache key and keep an `Arc` clone — this makes the next `get_or_fetch` call take the waiter path.
  3. Spawn `get_or_fetch` (wrapped in `tokio::time::timeout(Duration::from_secs(5), ...)`) without yet awaiting it, or await it inside a `tokio::spawn`.
  4. Populate `MODEL_CACHE` for that key with a valid entry, then call `notify_waiters()` on the stored `Arc` — firing the notification while the spawned waiter may not have polled.
  5. Assert the spawned call resolves within the timeout to the cached `ModelInfoWithFreshness` (not `fetch_failed`, not a hang).

  Because the old code's failure is an interleave race, this test is *probabilistically* red pre-fix and deterministically green post-fix — that is acceptable; the enabled-registration makes the wake unconditional regardless of poll timing. Run it a few times (`cargo test ... models::tests::waiter_wakes -- --nocapture` repeated 3×) to smoke out flakiness.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml models` → all pass; `bun run clippy` → exit 0.

## Test plan

Covered by Step 2 — one new `#[tokio::test]` in the file's test module, modeled on `test_get_or_fetch_waits_for_inflight_request` (~line 1281).

Verification: `cargo test --manifest-path src-tauri/Cargo.toml` → all pass.

## Done criteria

- [ ] The `Notified` future is created and `enable()`d before the `PENDING_REQUESTS` write guard drops (visible in the diff)
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0; `waiter_wakes_when_leader_finished_before_first_poll` exists and passes 3 consecutive runs
- [ ] `bun run clippy` exits 0
- [ ] `bunx biome check .` exits 0 (unchanged — Rust file only)
- [ ] Only `src-tauri/src/models.rs` modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The waiter/leader block no longer matches the excerpt (models.rs churns — it had 11 commits in the last 60 days).
- `Box::pin` + `enable()` doesn't satisfy the borrow checker the way written (e.g., the `Notified` lifetime can't escape the block) — the fallback is rearchitecting to a `tokio::sync::watch`/`broadcast` channel in `PENDING_REQUESTS`; that is a bigger change than this plan intends, so report first.
- `PendingMap`'s type changed such that storing an `Arc<Notify>` is no longer the mechanism.
- Any verification fails twice after a reasonable fix attempt.

## Maintenance notes

- The same "register before release" rule applies to any future `Notify` fan-out added to this codebase: always `enable()` (or poll) the `Notified` while still holding whatever guard makes the registration atomic with the pending-map read.
- Reviewer should scrutinize: the `must_rejoin` path (a `force_refresh` waiter rejoins as leader — it must still drop its enabled `Notified` before recursing, which the existing `drop(notify)` partially does; confirm the enabled-but-never-awaited `Notified` drops cleanly).
- Deferred: no timeout wraps the wait; if the leader itself hangs (provider fetch stall), waiters still hang — that's a separate "model fetch cancellation/timeout" issue, not this bug.
