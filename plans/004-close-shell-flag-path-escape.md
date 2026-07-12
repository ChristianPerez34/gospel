# Plan 004: Close Shell Flag-Style Path Escape

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 2e5bd36..HEAD -- src-tauri/src/shell_tools.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: MED
- **Depends on**: plans/003-fix-loop-detector-false-positive.md
- **Category**: security
- **Planned at**: commit `2e5bd36`, 2026-07-11

## Why this matters

Gospel restricts command execution tools to the active workspace boundaries, checking arguments for relative/absolute paths that escape the workspace root. However, the path escape checker currently skips any command-line argument that starts with `-`.
Consequently, an agent could package a path escape inside a flag, such as `--output=../secret.txt` or `-I../outside`, bypassing the safety policy.
This plan fixes the path-escape detector to parse flag-style inputs and check their embedded values, closing this security hole.

## Current state

- In `src-tauri/src/shell_tools.rs` lines 655-679:
```rust
fn has_path_escape(args: &[String], workspace_root: &Path) -> Option<String> {
    let workspace_canonical = match std::fs::canonicalize(workspace_root) {
        Ok(c) => c,
        Err(e) => return Some(format!("failed to canonicalize workspace root: {}", e)),
    };

    for arg in args {
        if arg.starts_with('-') {
            continue;
        }
        let path = PathBuf::from(arg);
        if !path.is_absolute() && arg.contains("..") {
            return Some(format!("relative path may escape workspace: {}", arg));
        }
        let candidate = if path.is_absolute() {
            path
        } else {
            workspace_canonical.join(path)
        };
        if candidate_escapes_workspace(&candidate, &workspace_canonical) {
            return Some(format!("path escapes workspace: {}", arg));
        }
    }
    None
}
```

## Commands you will need

| Purpose     | Command                                            | Expected on success |
|-------------|----------------------------------------------------|---------------------|
| Run tests   | `cargo test --manifest-path src-tauri/Cargo.toml`  | exit 0, all pass    |

## Scope

**In scope**:
- `src-tauri/src/shell_tools.rs` - update `has_path_escape` and add corresponding tests.

**Out of scope**:
- Changing the overall `CommandSafety` classification logic.
- Modifying command execution logic in other tools or platforms.

## Git workflow

- Branch: `advisor/004-close-shell-flag-path-escape`
- Commit message style: `security: prevent flag-style path escapes in command execution`

## Steps

### Step 1: Update has_path_escape to parse flag-style arguments
Modify `has_path_escape` in `src-tauri/src/shell_tools.rs` to extract embedded values from flags.
Specifically:
1. If the argument contains `=`, extract the value following the first `=` (e.g. `--file=value` -> `value`).
2. If the argument starts with a single dash `-` (but not `--`) and is longer than 2 characters, extract the suffix from index 2 onwards (e.g. `-Ivalue` -> `value`).
3. Subject the extracted values to the same absolute and relative directory checks.

```rust
fn has_path_escape(args: &[String], workspace_root: &Path) -> Option<String> {
    let workspace_canonical = match std::fs::canonicalize(workspace_root) {
        Ok(c) => c,
        Err(e) => return Some(format!("failed to canonicalize workspace root: {}", e)),
    };

    for arg in args {
        let mut paths_to_check = Vec::new();
        if arg.starts_with('-') {
            if let Some(pos) = arg.find('=') {
                paths_to_check.push(arg[pos + 1..].to_string());
            } else if !arg.starts_with("--") && arg.len() > 2 {
                paths_to_check.push(arg[2..].to_string());
            }
        } else {
            paths_to_check.push(arg.clone());
        }

        for path_str in paths_to_check {
            if path_str.is_empty() {
                continue;
            }
            let path = PathBuf::from(&path_str);
            if !path.is_absolute() && path_str.contains("..") {
                return Some(format!("relative path may escape workspace: {}", path_str));
            }
            let candidate = if path.is_absolute() {
                path
            } else {
                workspace_canonical.join(path)
            };
            if candidate_escapes_workspace(&candidate, &workspace_canonical) {
                return Some(format!("path escapes workspace: {}", path_str));
            }
        }
    }
    None
}
```

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` compiles successfully.

### Step 2: Add unit tests for flag-style escape detection
Add unit tests verifying:
- Equal-sign flag escape (e.g., `--output=../outside.txt`) is blocked.
- Absolute path equal-sign escape (e.g., `--file=/etc/passwd`) is blocked.
- Short flag glued escape (e.g., `-o../outside.txt`) is blocked.
- Safe flag inputs (e.g., `--verbose`, `-v`, `--output=file.txt`) are permitted.

```rust
    #[test]
    fn classify_shell_requires_approval_for_flag_style_path_escape() {
        let policy = CommandPolicy;
        
        // Equal-sign flag escape
        let safety = policy.classify_shell("cat", &["--output=../outside.txt".to_string()], &workspace());
        assert_eq!(safety, CommandSafety::Mutating);

        let safety = policy.classify_shell("cat", &["--file=/etc/passwd".to_string()], &workspace());
        assert_eq!(safety, CommandSafety::Mutating);

        // Short-flag glued escape
        let safety = policy.classify_shell("cat", &["-o../outside.txt".to_string()], &workspace());
        assert_eq!(safety, CommandSafety::Mutating);

        // Permitted inputs
        let safety = policy.classify_shell("cat", &["--verbose".to_string()], &workspace());
        assert_eq!(safety, CommandSafety::Safe);

        let safety = policy.classify_shell("cat", &["-v".to_string()], &workspace());
        assert_eq!(safety, CommandSafety::Safe);

        let safety = policy.classify_shell("cat", &["--output=file.txt".to_string()], &workspace());
        assert_eq!(safety, CommandSafety::Safe);
    }
```

**Verify**: Run `cargo test --manifest-path src-tauri/Cargo.toml` and confirm all tests pass.

## Done criteria

- [x] `cargo test` passes all tests.
- [x] New unit tests verifying flag-style path escapes are executed and pass.
- [x] No files outside the in-scope list are modified, except the required plan status files.

## STOP conditions

- If compiling the project fails due to changes.
- If path escape check false-positives block standard Git/Shell options without `=` or glued values.
