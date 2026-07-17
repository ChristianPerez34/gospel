# Plan 007: Harden Skill Script Directory Sandbox

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 8c6e3ad..HEAD -- src-tauri/src/skills.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: MED
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `8c6e3ad`, 2026-07-17

## Why this matters

`run_skill_script` in `src-tauri/src/skills.rs` canonicalizes the skill
directory, joins `scripts/` to it, and then checks the *canonical* script path
against the *canonical* `scripts_dir` to ensure the script lives under
`scripts/`. But it never checks that the *canonical* `scripts_dir` itself
lives under the canonical skill directory. If `scripts/` is a symlink to a
directory outside the skill root, every script under it is reachable through
the agent and will be approved, executed, and reported as if it were a bundled
script. A user that ships a skill with a malicious symlink (or whose
workspace has been tampered with to add one) can use this to escape the
sandbox and run arbitrary code. This plan requires the canonical `scripts_dir`
to live under the canonical skill root before any script under it executes.

## Current state

The relevant file is `src-tauri/src/skills.rs`. The sandbox check inside
`run_skill_script` at lines 719-815 (excerpted):

```rust
pub async fn run_skill_script(
    skill: &Skill,
    script_name: &str,
    workspace_path: Option<&Path>,
) -> Result<ScriptResult, String> {
    let skill_dir = {
        let base = if skill.source == SkillSource::Workspace {
            workspace_path
                .ok_or("Workspace path is required for workspace skills")?
                .join(".agents")
                .join("skills")
                .join(&skill.name)
        } else {
            global_skills_dir()
                .ok_or("Global skills directory is not available")?
                .join(&skill.name)
        };
        match fs::canonicalize(&base) {
            Ok(p) => p,
            Err(e) => return Err(format!("Failed to resolve skill directory: {}", e)),
        }
    };

    let scripts_dir = skill_dir.join("scripts");
    let script_path = scripts_dir.join(script_name);

    let canonical_script = match fs::canonicalize(&script_path) {
        Ok(p) => p,
        Err(e) => return Err(format!("Script not found '{}': {}", script_name, e)),
    };

    let canonical_scripts_dir = match fs::canonicalize(&scripts_dir) {
        Ok(p) => p,
        Err(e) => return Err(format!("Failed to resolve scripts directory: {}", e)),
    };

    if !canonical_script.starts_with(&canonical_scripts_dir) {
        return Err(format!(
            "Script '{}' escapes the skill directory",
            script_name
        ));
    }
    // ...
}
```

The current code only verifies the script-under-script-dir relationship; it
does not verify scripts-dir-under-skill-dir. A symlinked `scripts/`
directory pointing outside the skill root passes the existing check and runs
its contents.

The pre-existing `run_skill_script_rejects_escape` test at line 1417 covers
the *direct-script-symlink* case (the symlink is the script file itself, not
the directory) and should keep passing unchanged. The new test in Step 4
covers the missing case.

## Commands you will need

| Purpose         | Command                                              | Expected on success               |
|-----------------|------------------------------------------------------|-----------------------------------|
| Compile check   | `cargo check --manifest-path src-tauri/Cargo.toml`   | exit 0, no errors                 |
| Lint            | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | exit 0       |
| Run tests       | `cargo test --manifest-path src-tauri/Cargo.toml`    | exit 0, all pass                  |

## Scope

**In scope**:
- `src-tauri/src/skills.rs` — `run_skill_script` function and its inline tests.

**Out of scope**:
- Changes to skill discovery (`discover_skills`, `discover_skills_in_dir`).
- Changes to approval flow in `RunSkillScriptTool` (Plan 005 already shipped
  that).
- Changes to `detect_interpreter` or other helpers.
- Changes to the `Skill` struct or to skill source discrimination.

## Git workflow

- Branch: `advisor/007-harden-skill-script-directory-sandbox`
- Commit message style: `security: require canonical scripts directory to live under the skill root`

## Steps

### Step 1: Add the directory-containment check

In `run_skill_script`, after computing `canonical_scripts_dir` (line 750) and
before checking `canonical_script.starts_with(&canonical_scripts_dir)`
(line 755), add a check that the canonical scripts directory itself lives
under the canonical skill directory. Use `Path::starts_with`, matching the
existing code's idiom.

The check is:

```rust
if !canonical_scripts_dir.starts_with(&skill_dir) {
    return Err(format!(
        "Scripts directory escapes the skill directory: {}",
        canonical_scripts_dir.display()
    ));
}
```

Place the new check immediately after the `match fs::canonicalize(&scripts_dir)`
block, before the existing `canonical_script.starts_with(&canonical_scripts_dir)`
check. This is a small, contained change that does not reorder any other
logic.

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` exits 0.

### Step 2: Confirm the existing direct-script-symlink rejection still works

The existing `run_skill_script_rejects_escape` test at line 1417-1442
exercises the direct-script-symlink case (a symlink at
`scripts_dir/escape.sh` that points to an outside script). It must continue
to pass: the new directory check must NOT trigger before the existing
script-under-script-dir check, because the direct-script symlink scenario
keeps `canonical_scripts_dir` under the skill dir and only fails the
script-under-script-dir check.

Confirm by re-reading the test setup: the symlink is created as
`scripts_dir.join("escape.sh")`, so `scripts_dir` itself is a real
directory under the skill dir, and the test must still hit the
"escapes the skill directory" error from the existing
`canonical_script.starts_with(&canonical_scripts_dir)` check, not the new
one.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml -- run_skill_script_rejects_escape` exits 0 and reports the test passing.

### Step 3: Confirm ordinary bundled scripts still work

The `run_skill_script_executes_bash_script` test at line 1384-1414 covers
the happy path (a real script under `scripts_dir`, no symlinks). It must
continue to pass. Re-run it after Step 1.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml -- run_skill_script_executes_bash_script` exits 0.

### Step 4: Add a regression test for the symlinked-scripts-directory case

In the `tests` module (the `mod tests` block inside `src-tauri/src/skills.rs`),
add a new `#[cfg(unix)] #[tokio::test]` test
`run_skill_script_rejects_symlinked_scripts_dir`, modeled on
`run_skill_script_rejects_escape` at line 1417.

```rust
#[cfg(unix)]
#[tokio::test]
async fn run_skill_script_rejects_symlinked_scripts_dir() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let skills_dir = dir.path().join(".agents").join("skills");
    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();

    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: test-skill\ndescription: test\n---\n\nbody",
    )
    .unwrap();

    // External directory with a real script inside.
    let outside = tempdir().unwrap();
    let outside_script_dir = outside.path().join("external-scripts");
    fs::create_dir_all(&outside_script_dir).unwrap();
    let outside_script = outside_script_dir.join("evil.sh");
    fs::write(&outside_script, "#!/bin/bash\necho pwned").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&outside_script, fs::Permissions::from_mode(0o755)).unwrap();

    // Replace the skill's scripts/ directory with a symlink to the outside.
    let scripts_link = skill_dir.join("scripts");
    symlink(&outside_script_dir, &scripts_link).unwrap();

    // Sanity: confirm the symlink escapes.
    let canonical_link = fs::canonicalize(&scripts_link).unwrap();
    let canonical_skill = fs::canonicalize(&skill_dir).unwrap();
    assert!(
        !canonical_link.starts_with(&canonical_skill),
        "test setup: scripts symlink should escape skill dir"
    );

    let skill = make_skill("test-skill", "test", SkillSource::Workspace);
    let result = run_skill_script(&skill, "evil.sh", Some(dir.path())).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("Scripts directory escapes") || err.contains("escapes"),
        "unexpected error: {}",
        err
    );
}
```

The `make_skill` helper is already defined in the same `tests` module (search
for `fn make_skill`). The `tempfile::tempdir` and `use std::fs` imports are
already in scope at the top of the `tests` module (line 832-834).

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml -- run_skill_script_rejects_symlinked_scripts_dir` exits 0 and reports the new test passing.

### Step 5: Run lint and full test suite

- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — exit 0.
- `cargo test --manifest-path src-tauri/Cargo.toml` — exit 0, all tests pass.

**Verify**: Both commands exit 0.

## Test plan

- New test (in `src-tauri/src/skills.rs`):
  - `run_skill_script_rejects_symlinked_scripts_dir` — `#[cfg(unix)]` because
    `std::os::unix::fs::symlink` is unix-only; matches the existing
    `rejects_symlink_escape` discovery test at line 959-988.
- Pre-existing tests that must continue to pass: `run_skill_script_executes_bash_script`,
  `run_skill_script_rejects_escape`, `run_skill_script_returns_error_for_missing_script`,
  and the rest of the `skills::tests` module.
- Verification: `cargo test --manifest-path src-tauri/Cargo.toml` exits 0 with
  1 more test than before.

## Done criteria

- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` exits 0
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0; the new
      `run_skill_script_rejects_symlinked_scripts_dir` test exists and passes
- [ ] No files outside `src-tauri/src/skills.rs` are modified
- [ ] `plans/README.md` status row for plan 007 updated to `DONE`

## STOP conditions

- The drift check shows non-empty output for `src-tauri/src/skills.rs` between
  `8c6e3ad` and `HEAD`. Re-open the file, reconcile, treat as a planning defect.
- The new directory check fires on a real bundled skill (any test in the
  `skills::tests` module regresses). STOP and re-read Step 1 — most likely
  cause: the new check uses `skill_dir` (the canonical one) but the variable
  was shadowed; verify it refers to the canonical path.
- A reviewer asks whether this also affects skill *discovery*; the answer is
  no (`discover_skills` has its own symlink check at the `Skill` layer), and
  the change must stay within `run_skill_script`.

## Maintenance notes

- The `scripts/` containment check is a one-shot path comparison. If a future
  plan adds nested scripts (e.g. `scripts/sub/dir/x.sh`), no change is needed
  here — `Path::starts_with` already covers that.
- The error message format intentionally distinguishes
  "Scripts directory escapes" from "Script X escapes the skill directory"
  so that the existing test `run_skill_script_rejects_escape` continues to
  assert on the latter without ambiguity.
- A future plan that adds a per-skill script whitelist should layer on top of
  this check, not replace it.
