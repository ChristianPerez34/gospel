# Plan 011: Persist TopBar inline session-title edits

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 72819cd..HEAD -- src/components/TopBar.tsx src/components/AppShell.tsx src-tauri/src/session_store.rs src-tauri/src/lib.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `72819cd`, 2026-07-20

## Why this matters

TopBar has an inline "click to edit the session title" affordance — the UI
mounts an input, lets the user type, then discards the result on submit. Users
rename a session, hit Enter/blur, see the field close, and assume it's saved;
nothing is persisted and the title reverts on the next render. Silent data loss
of user intent on a primary metadata field.

## Current state

- `src/components/TopBar.tsx` — top bar. Lines 6–18 declare `TopBarProps`
  (no `onSessionTitleChange` prop exists). Lines 33–50 hold the editing
  state; the broken submit is:
  ```ts
  const [editing, setEditing] = useState(false);
  const [title, setTitle]   = useState(sessionTitle);
  ...
  const handleSubmit = () => {
    setEditing(false);   // <-- no invoke, no prop call
  };
  ```
- `src/components/AppShell.tsx` — owns `session` (from `useSessionManager`),
  renders `<TopBar ... />` higher up in the file (search `TopBar`). It already
  mirrors an analogous "optimistic update + rollback on error" pattern for the
  model selector — see `applyModelSelection` / `applyVariantSelection` in
  `AppShell.tsx` (around the `InputBar` block at lines 820–822) and the
  `useSessionManager.ts:154-172` sync effect that writes messages back to
  `setSessions`. Use that pattern: optimistic local state + a Tauri invoke to
  persist, revert on error.
- `src-tauri/src/session_store.rs` — persistence layer. Existing update
  methods to mirror exactly (same signature shape, same `Result<(),
  SessionStoreError>`):
  - `src-tauri/src/session_store.rs:553` `pub fn update_status(&self, id: &str, status: &str) -> Result<(), SessionStoreError>`
  - `src-tauri/src/session_store.rs:565` `pub fn update_session_mode(&self, id: &str, mode: &str) -> Result<(), SessionStoreError>`
  - `src-tauri/src/session_store.rs:580` `pub fn update_model_selection(...)`
  Each runs a parameterized `UPDATE sessions SET ... WHERE id = ?` and returns
  the row-mapped error. The `title` column is `TEXT NOT NULL` (confirmed by `rg
  "title TEXT" src-tauri/src/session_store.rs`).
- `src-tauri/src/lib.rs` — Tauri command surface. The existing analogous
  commands (search these exact names) and their registration in the
  `generate_handler!` list:
  - `lib.rs:1991` `update_session_model_selection`
  - `lib.rs:2010` `update_session_mode` (registered at `lib.rs:2764`)
  Web `invoke()` callers call these by their snake_case names.
- `src/hooks/useSessionManager.ts:212` — contains the only `handleSend`;
  this file is the natural place to add a `renameSession` callback that the
  AppShell passes down, OR AppShell can own the rename directly. The plan
  below picks the simpler option (AppShell owns it) to keep this plan's
  touch surface small.

No design-doc constraints apply specifically to title editing (DESIGN.md only
specifies "session title (editable inline)" in §3 Top bar). No ADR covers this.

## Commands you will need

| Purpose   | Command                                                                  | Expected on success |
|-----------|--------------------------------------------------------------------------|---------------------|
| Typecheck | `bun run typecheck`                                                       | exit 0, no errors   |
| Lint      | `bun run lint`                                                            | exit 0 (warnings allowed) |
| Frontend tests | `bun run test -- src/components/TopBar.test.tsx`                    | all pass            |
| Backend tests | `cargo test --manifest-path src-tauri/Cargo.toml -- sess_store`       | all pass            |
| Rust lint | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`       | exit 0, no warnings |

## Scope

**In scope**:
- `src-tauri/src/session_store.rs` (add `update_session_title`, plus a unit test mirroring `update_session_mode_round_trips_through_get_and_list` at line 1447)
- `src-tauri/src/lib.rs` (add `update_session_title` Tauri command + register in `generate_handler!`)
- `src/components/TopBar.tsx` (add `onSessionTitleChange: (title: string) => void` prop; call it from `handleSubmit` when the trimmed title differs from `sessionTitle`)
- `src/components/AppShell.tsx` (pass the new prop; optimistic update + rollback on error)
- `src/components/TopBar.test.tsx` (add a test asserting the callback fires; update an existing test if it asserted the no-op)

**Out of scope**:
- Bulk rename, rename from SessionDrawer context menu, undo/redo
- Any change to the Model History vs Display Transcript separation (ADR-0005) — titles are metadata, not transcript
- Backend concurrency primitives or schema migrations beyond the new `UPDATE`

## Git workflow

- Branch: `advisor/011-persist-session-title`
- Commit per step. The repo uses both `feat:`/`fix:`/`chore:`/`docs:` prefixes and short plain-language commits (see `git log --oneline -20`). Match the prevailing style; example: `fix: persist TopBar inline session-title edits via new update_session_title command`.
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add the backend update + test

In `src-tauri/src/session_store.rs`, add (immediately after `update_session_mode` at line 565):

```rust
pub fn update_session_title(&self, id: &str, title: &str) -> Result<(), SessionStoreError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(SessionStoreError::InvalidInput(
            "session title must not be empty".to_string(),
        ));
    }
    self.conn
        .execute(
            "UPDATE sessions SET title = ?, updated_at = ? WHERE id = ?",
            params![trimmed, now_rfc3339(), id],
        )
        .map_err(|e| SessionStoreError::from(e))?;
    Ok(())
}
```

If `SessionStoreError::InvalidInput` does not exist, use the closest existing
variant by grepping `enum SessionStoreError` in the same file; if none fits,
add the variant. Mirror the `now_rfc3339()` helper already used by sibling
updates (search `now_rfc3339`).

Add a unit test next to `update_session_mode_round_trips_through_get_and_list`
(line 1447) named `update_session_title_round_trips_through_get_and_list`,
asserting: (1) the title changes, (2) empty/whitespace input errors, (3) the
error does not mutate the row. Use the existing test's `store` fixture.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml -- update_session_title`
→ the new test passes; existing title tests still pass.

### Step 2: Expose the Tauri command

In `src-tauri/src/lib.rs`, add a command next to `update_session_mode`
(line 2010). Use the exact shape of `update_session_mode` (which fetches the
shared `SessionStore` state via the existing helper, calls the store method,
and maps `SessionStoreError` to a `String`):

```rust
#[tauri::command]
fn update_session_title(
    state: tauri::State<AppState>,
    session_id: String,
    title: String,
) -> Result<(), String> {
    state
        .session_store
        .update_session_title(&session_id, &title)
        .map_err(|e| e.to_string())
}
```

(Adjust the `state` helper name if `AppState`'s field is named differently —
search `update_session_mode`'s body and copy its access pattern verbatim.)

Register `update_session_title` in the `generate_handler!` array right after
`update_session_mode` (currently `lib.rs:2764`).

**Verify**: `cargo build --manifest-path src-tauri/Cargo.toml` → exit 0.
Then `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
→ exit 0 (the `check` script in package.json runs this).

### Step 3: Plumb the prop through TopBar

In `src/components/TopBar.tsx`:
- Add to `TopBarProps` (lines 6–18): `onSessionTitleChange: (title: string) => void;`
- Add it to the destructured props list (lines 20–32).
- Replace `handleSubmit` (line 48):
  ```ts
  const handleSubmit = () => {
    const next = title.trim();
    if (next.length > 0 && next !== sessionTitle) {
      onSessionTitleChange(next);
    }
    setEditing(false);
  };
  ```
- Keep `setTitle(sessionTitle)` in the `useEffect` at line 37 so escape/cancel
  still reverts local state.

### Step 4: Wire AppShell to the backend

In `src/components/AppShell.tsx`, find the `<TopBar ... />` render. Add a new
handler that optimistically updates the local session list, calls the backend,
and rolls back on error:

```tsx
const handleSessionTitleChange = async (sessionId: string, title: string) => {
  const prev = allSessions;
  setAllSessions((curr) =>
    curr.map((s) => (s.id === sessionId ? { ...s, title } : s)),
  );
  try {
    await invoke<void>("update_session_title", { sessionId, title });
  } catch (e) {
    setAllSessions(prev);
    showError(`Failed to rename session: ${e}`);
  }
};
```

`showError` is already used in AppShell (see line ~893). Pass
`onSessionTitleChange={(title) => activeSession?.id
  ? void handleSessionTitleChange(activeSession.id, title)
  : undefined}` to TopBar. Adjust the destructured `activeSession` reference to
match whatever AppShell actually names the current session object (search
`TopBar` in AppShell and read the prop values it already passes).

**Verify**: `bun run typecheck` → exit 0. Then `bun run test -- src/components/TopBar.test.tsx`
→ all existing tests pass.

### Step 5: Update TopBar tests

In `src/components/TopBar.test.tsx`:
- Update any existing test that previously asserted the silent no-op (search
  for `handleSubmit` / `onChange` / `editing`). The existing pattern in this
  file is `render(<TopBar ... />)` with `vi.fn()` props (search `vi.fn`).
- Add a test: `asserts onSessionTitleChange is called with the trimmed title on Enter`
  - Render TopBar with `sessionTitle="Old"` and a `vi.fn()` for
    `onSessionTitleChange`.
  - Trigger the edit affordance (find it by searching how the existing tests
    open editing — likely clicking the title element with `aria-label` or
    similar).
  - Type `"New Title  "` (trailing whitespace) into the input, dispatch an
    Enter keydown, and assert the mock was called with `"New Title"`.
- Add a test: `does not call onSessionTitleChange when the trimmed title equals the current title`
  - Same setup; type the unchanged title; assert the mock was not called.

**Verify**: `bun run test -- src/components/TopBar.test.tsx` → all pass, including the two new ones.

## Test plan

- New `update_session_title_round_trips_through_get_and_list` Rust unit test
  in `src-tauri/src/session_store.rs` (Step 1).
- New `update_session_title_*` Tauri command smoke behavior is covered
  transitively by the FE test (full FE→BE path is not in scope; the unit test
  + FE test together cover the contract).
- Two new TopBar tests in `src/components/TopBar.test.tsx` (Step 5).

## Done criteria

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml -- sess_store` exits 0; the new `update_session_title_round_trips_through_get_and_list` test passes
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] `bun run typecheck` exits 0
- [ ] `bun run test` exits 0; the two new TopBar tests pass
- [ ] `rg "setEditing\(false\)$" src/components/TopBar.tsx` returns no matches where the call is the sole body of `handleSubmit` (i.e. there is now a guarded call to `onSessionTitleChange`)
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row for plan 011 updated

## STOP conditions

Stop and report back if:

- The code at `src/components/TopBar.tsx:33-50`, `src-tauri/src/session_store.rs:553-620`, or `src-tauri/src/lib.rs:2010-2050` doesn't match the excerpts above (drift since plan was written).
- `SessionStoreError` has no `InvalidInput`-equivalent variant and the closest existing variant isn't obvious (report and propose).
- AppShell already has a `renameSession` mechanism (in which case Step 4 becomes "wire TopBar to the existing handler" — don't duplicate).
- The TopBar edit trigger in tests can't be located by reading `TopBar.test.tsx`'s existing tests.

## Maintenance notes

- Future work that adds a SessionDrawer context-menu "Rename" should reuse the
  same `update_session_title` Tauri command; don't add a second one.
- Reviewer should check that the optimistic update in AppShell keys off the
  same session-id comparison as the back-end `WHERE id = ?` (id mismatch is
  the silent failure mode).
- Out of scope: debounce/coalescing of rapid edits — current behavior fires
  one invoke per `handleSubmit`. Acceptable because the editor closes on
  submit; revisit if a free-text inline mode is added later.