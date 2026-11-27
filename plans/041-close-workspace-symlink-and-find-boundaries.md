# Plan 041: Close Workspace Boundary Gaps in `symlink_guard`, `skill_opt`, and `CommandPolicy`

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 04a60d3..HEAD -- src-tauri/src/symlink_guard.rs src-tauri/src/skill_opt.rs src-tauri/src/shell_tools.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW — rejecting parent-directory traversal after missing components, validating symlinks before `skill_opt` writes, treating dangling external symlinks as workspace-escaping, and prompting on `find` output-writing flags only close bypasses without altering normal workspace workflows.
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `04a60d3`, 2026-11-25

## Why this matters

Three closely related workspace-boundary seams currently fail open:

1. **`symlink_guard::validate_existing_ancestors`** breaks out of its component loop on the first `ErrorKind::NotFound` without inspecting remaining components, so a path like `<root>/missing/../../outside` passes `strip_prefix(root)` and returns `Ok(())`.
2. **`skill_opt` disk writes** (`stage_skill`, `adopt_skill`, `write_report`, `record_rejected_edits`) call `fs::create_dir_all` and write directly to `.gospel/skill-opt/<name>/...` and `.agents/skills/<name>/SKILL.md` without `symlink_guard::validate_existing_ancestors`, and `atomic_write` writes via `fs::write` to a predictable PID-based filename (`.<file>.tmp.<pid>`) instead of an exclusive `tempfile::NamedTempFile`. In a workspace with planted symlinks, skill optimization or adoption can overwrite files outside the workspace.
3. **`CommandPolicy` (`shell_tools.rs`)** has two Ask-mode gaps: `candidate_escapes_workspace` fails open (`false`) on a workspace-local dangling symlink pointing outside the workspace (because `canonicalize` fails on the broken target), misclassifying the command as `workspace_scoped = true`; and `classify_find_action` omits GNU `find` file-writing actions (`-fprint`, `-fprint0`, `-fprintf`, `-fls`), letting them run as `ReadOnly` without approval.

## Current state

Relevant files:

- `src-tauri/src/symlink_guard.rs` — workspace symlink containment helper (`canonical`, `is_within`, `validate_existing_ancestors`, lines 1–69).
- `src-tauri/src/skill_opt.rs` — skill staging/adoption/report/rejection writes (`stage_skill` lines 683–696, `write_report` lines 715–734, `adopt_skill` lines 861–872, `record_rejected_edits` lines 926–955, `atomic_write` lines 957–971).
- `src-tauri/src/shell_tools.rs` — `classify_find_action` (lines 762–774) and `candidate_escapes_workspace` (lines 1022–1046).

Current excerpts:

```rust
// src-tauri/src/symlink_guard.rs:25-54
pub fn validate_existing_ancestors(root: &Path, target: &Path) -> io::Result<()> {
    let relative = target.strip_prefix(root).map_err(|_| {
        io::Error::other(format!(
            "path {} is not under workspace root {}",
            target.display(),
            root.display()
        ))
    })?;
    let mut current = root.to_path_buf();

    for component in relative.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(_) => {
                let resolved = canonical(&current)?;
                if !is_within(root, &resolved) {
                    return Err(io::Error::other(format!(
                        "path {} escapes workspace root {}",
                        resolved.display(),
                        root.display()
                    )));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }

    Ok(())
}
```

```rust
// src-tauri/src/skill_opt.rs:861-872,957-971
pub fn adopt_skill(workspace: &Path, name: &str) -> Result<PathBuf, SkillOptIoError> {
    validate_skill_name(name)?;
    let staged = staged_skill_path(workspace, name);
    let skill_md = fs::read_to_string(&staged).map_err(|_| SkillOptIoError::MissingStage)?;
    validate_skill_document(name, &skill_md)?;
    let adopted = discovered_skill_path(workspace, name);
    if let Some(parent) = adopted.parent() {
        fs::create_dir_all(parent).map_err(|e| SkillOptIoError::Io(e.to_string()))?;
    }
    atomic_write(&adopted, skill_md.as_bytes())?;
    Ok(adopted)
}
// ...
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), SkillOptIoError> {
    let parent = path
        .parent()
        .ok_or_else(|| SkillOptIoError::Io("write target has no parent".to_string()))?;
    let temp_path = parent.join(format!(
        ".{}.tmp.{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("skill"),
        std::process::id()
    ));
    fs::write(&temp_path, bytes).map_err(|e| SkillOptIoError::Io(e.to_string()))?;
    fs::rename(&temp_path, path).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        SkillOptIoError::Io(e.to_string())
    })
}
```

```rust
// src-tauri/src/shell_tools.rs:762-774,1022-1046
fn classify_find_action(args: &[String]) -> Option<CommandSafety> {
    if args.iter().any(|a| a == "-delete") {
        return Some(CommandSafety::Destructive);
    }
    if args
        .iter()
        .any(|a| a == "-exec" || a == "-execdir" || a == "-ok" || a == "-okdir")
    {
        return Some(CommandSafety::Mutating);
    }
    None
}
// ...
fn candidate_escapes_workspace(candidate: &Path, workspace_canonical: &Path) -> bool {
    if let Ok(canonical) = std::fs::canonicalize(candidate) {
        return !canonical.starts_with(workspace_canonical);
    }

    if candidate.is_absolute()
        && !normalize_path_lexically(candidate).starts_with(workspace_canonical)
    {
        return true;
    }

    let mut existing_ancestor = candidate;
    while !path_exists_or_is_symlink(existing_ancestor) {
        let Some(parent) = existing_ancestor.parent() else {
            return false;
        };
        if parent == existing_ancestor {
            return false;
        }
        existing_ancestor = parent;
    }

    std::fs::canonicalize(existing_ancestor)
        .is_ok_and(|canonical| !canonical.starts_with(workspace_canonical))
}
```

Repo convention exemplars:

- Exclusive temp-file writes: `src-tauri/src/keychain.rs:48-59` uses `tempfile::Builder::new().prefix(...).suffix(".tmp").tempfile_in(&dir)?` + `temp.persist(target)`.
- Symlink-safe workspace writes: `src-tauri/src/workspace_tools.rs` rejects symlink targets and uses `symlink_guard`.
- `AGENTS.md`: *"Ask mode can always-allow eligible workspace-scoped mutating commands. Workspace-escaping command calls require per-request approval."*

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Clippy | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | exit 0, no warnings |
| Targeted Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml symlink_guard::tests && cargo test --manifest-path src-tauri/Cargo.toml skill_opt::tests && cargo test --manifest-path src-tauri/Cargo.toml shell_tools::tests` | all pass |
| Full Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all pass |

## Scope

**In scope** (the only files you should modify):
- `src-tauri/src/symlink_guard.rs`
- `src-tauri/src/skill_opt.rs`
- `src-tauri/src/shell_tools.rs`

**Out of scope** (do NOT touch):
- `src-tauri/src/workspace_tools.rs` — already enforces symlink boundaries and exclusive temp files.
- `src-tauri/src/approval_broker.rs` — Auto mode rules (ADR-0014) are unchanged.
- `src-tauri/Cargo.toml` — `tempfile` is already a dependency.

## Git workflow

- Branch: `advisor/041-close-workspace-symlink-and-find-boundaries`
- Commit style: conventional commits matching `git log` (e.g., `fix(security): harden symlink_guard, skill_opt writes, and CommandPolicy`)
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Reject `ParentDir` / non-normal components in `symlink_guard::validate_existing_ancestors`

In `src-tauri/src/symlink_guard.rs`:

1. Import `std::path::Component`.
2. Right after `let relative = target.strip_prefix(root)...?;`, iterate over `relative.components()` BEFORE the filesystem loop and reject any `Component::ParentDir | Component::RootDir | Component::Prefix(_)` with `Err(io::Error::other(format!("path {} contains forbidden traversal component", target.display())))`.
3. Also verify that if an existing component is a dangling symlink (`std::fs::symlink_metadata(&current)` returns `Ok(meta)` where `meta.file_type().is_symlink()`, and `canonical(&current)` returns `Err`), `canonical(&current)?` already returns `Err` — keep that fail-closed behavior and add a test proving it.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml symlink_guard::tests` → all pass.

### Step 2: Enforce `symlink_guard` and exclusive temp files in `src-tauri/src/skill_opt.rs`

In `src-tauri/src/skill_opt.rs`:

1. Add a small helper `fn validate_workspace_path(workspace: &Path, target: &Path) -> Result<PathBuf, SkillOptIoError>`:
   - Resolve `let canonical_root = crate::symlink_guard::canonical(workspace).map_err(|e| SkillOptIoError::Io(e.to_string()))?;`
   - Rebase `target` under `canonical_root` (if `target` starts with `workspace`, strip `workspace` and join onto `canonical_root`; otherwise return `Err(SkillOptIoError::Io(...))`).
   - Call `crate::symlink_guard::validate_existing_ancestors(&canonical_root, &rebased).map_err(|e| SkillOptIoError::Io(e.to_string()))?;`
   - If `rebased` exists as a symlink (`fs::symlink_metadata(&rebased).is_ok_and(|m| m.file_type().is_symlink())`), return `Err(SkillOptIoError::Io("refusing to follow symlink target".to_string()))`.
   - Return `Ok(rebased)`.
2. Call `validate_workspace_path(workspace, &path)?` in:
   - `stage_skill` (before `create_dir_all` and `atomic_write`)
   - `write_report` (before `create_dir_all` and `atomic_write`)
   - `adopt_skill` (on BOTH `staged` before `read_to_string` AND `adopted` before `create_dir_all` and `atomic_write`)
   - `record_rejected_edits` (before `create_dir_all`, `read_to_string`, and `atomic_write`)
   - Also check any sibling readers in `skill_opt.rs` (e.g., `read_discovered_skill` right above `adopt_skill`, `read_staged_skill` if present) and validate their paths with `validate_workspace_path` so symlinks cannot be read outside the workspace either.
3. Replace `atomic_write` in `src-tauri/src/skill_opt.rs`:
   - Use `tempfile::Builder::new().prefix(".skill-opt-").suffix(".tmp").tempfile_in(parent)` (which creates an exclusive, randomized temp file with `O_EXCL`), write `bytes` via `std::io::Write::write_all`, flush, and call `temp.persist(path)`.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml skill_opt::tests` → all existing tests pass.

### Step 3: Close dangling-symlink fail-open and `find` write actions in `src-tauri/src/shell_tools.rs`

In `src-tauri/src/shell_tools.rs`:

1. In `classify_find_action(args: &[String])`:
   - Add `"-fprint" | "-fprint0" | "-fprintf" | "-fls"` to the `CommandSafety::Mutating` match list alongside `"-exec" | "-execdir" | "-ok" | "-okdir"`.
2. In `candidate_escapes_workspace(candidate: &Path, workspace_canonical: &Path) -> bool`:
   - After the ancestor loop stops at `existing_ancestor`, inspect `std::fs::canonicalize(existing_ancestor)`:
     - If `Ok(canonical)` → return `!canonical.starts_with(workspace_canonical)`.
     - If `Err(_)` AND `std::fs::symlink_metadata(existing_ancestor).is_ok_and(|m| m.file_type().is_symlink())` → check `std::fs::read_link(existing_ancestor)`:
       - Resolve relative link targets against `existing_ancestor.parent()`, normalize lexically (`normalize_path_lexically`), and return `true` unless the normalized target starts with `workspace_canonical` (or even simpler and strictly safer: if `canonicalize` fails on a symlink — a broken/cyclic symlink — return `true` to fail closed so it requires per-request approval!). Returning `true` whenever `existing_ancestor` is a symlink whose `canonicalize` fails is strictly fail-closed and prevents broken-symlink chains from bypassing escape detection.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml shell_tools::tests` → all existing tests pass.

### Step 4: Add unit tests across all three modules

Add the tests described in the Test plan, then run Clippy and the full Rust test suite.

**Verify**: `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings && cargo test --manifest-path src-tauri/Cargo.toml` → exit 0, all tests pass.

## Test plan

1. **`src-tauri/src/symlink_guard.rs`** (`mod tests`):
   - `validate_existing_ancestors_rejects_parent_dir_after_missing_segment`: `validate_existing_ancestors(&canonical_dir, &canonical_dir.join("missing/../../outside"))` returns `Err`.
   - `validate_existing_ancestors_rejects_dangling_symlink` (Unix `#[cfg(unix)]`): symlink inside `canonical_dir` pointing to nonexistent external target causes `validate_existing_ancestors` to return `Err`.
2. **`src-tauri/src/skill_opt.rs`** (`mod tests`, `#[cfg(unix)]`):
   - `stage_skill_rejects_symlink_escape`: symlinking `.gospel/skill-opt/<name>` to a sibling tempdir outside the workspace causes `stage_skill` to return `Err` and leaves the external directory untouched.
   - `adopt_skill_rejects_symlink_escape`: symlinking `.agents/skills/<name>` (or `SKILL.md`) to a path outside the workspace causes `adopt_skill` to return `Err` and leaves the external path untouched.
   - `record_rejected_edits_rejects_symlink_escape`: symlinking `.gospel/skill-opt/<name>/rejected.json` outside the workspace fails without modifying the target.
3. **`src-tauri/src/shell_tools.rs`** (`mod tests`):
   - `find_file_writing_flags_require_approval`: `find . -fprint out.txt`, `-fprint0 out.txt`, `-fprintf out.txt %p`, and `-fls out.txt` are each classified as `CommandSafety::Mutating` (NOT `ReadOnly`).
   - `dangling_symlink_escaping_workspace_is_not_workspace_scoped` (`#[cfg(unix)]`): creating a broken symlink `<workspace>/escape_link -> <outside_dir>/does_not_exist` and classifying a command referencing `escape_link` (or `escape_link/file`) marks it as workspace-escaping (`workspace_scoped == false` / `candidate_escapes_workspace == true`).

**Verification**: `cargo test --manifest-path src-tauri/Cargo.toml symlink_guard::tests && cargo test --manifest-path src-tauri/Cargo.toml skill_opt::tests && cargo test --manifest-path src-tauri/Cargo.toml shell_tools::tests` → all pass, including ≥ 7 new tests.

## Done criteria

- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0, including the ≥ 7 new tests across `symlink_guard`, `skill_opt`, and `shell_tools`
- [ ] `rg "std::process::id" src-tauri/src/skill_opt.rs` returns no matches
- [ ] Only in-scope files are modified (`git status`)
- [ ] `plans/README.md` status row for `041` updated to `DONE`

## STOP conditions

Stop and report back (do not improvise) if:

- Any existing test in `skill_opt::tests` passes a non-existent `workspace` root directory to `stage_skill`/`adopt_skill` (if so, check whether `canonical(workspace)` must stay compatible with test fixtures that create `workspace` lazily; report before relaxing `validate_workspace_path`).
- Any excerpt in "Current state" does not match the live file.
- A step's verification fails twice after a reasonable fix attempt.

## Maintenance notes

- Any new workspace file writer added under `.gospel/` or `.agents/` should call `symlink_guard::validate_existing_ancestors` and write via `tempfile::Builder::new().tempfile_in(parent)` + `persist`.
- When adding any new command to `is_read_only_shell_program` in `shell_tools.rs`, audit its flags for output-writing actions (like `find -fprint`, `rg --pre`, `git --output`).
