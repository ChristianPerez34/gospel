# Plan 006: Close Whitespace-Containing Shell Path Bypass

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 8c6e3ad..HEAD -- src-tauri/src/shell_tools.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `8c6e3ad`, 2026-07-17

## Why this matters

`CommandPolicy::classify_shell` only checks a command's arguments for workspace
escapes when the argument's embedded path value passes `is_path_like`, which
filters out anything containing whitespace or control characters. A user can
exploit that filter to pass absolute or relative paths containing spaces — e.g.
`cat "/etc/some spaced dir/secret"` or `head /Users/me/My Documents/x.txt` — and
have them classified as `ReadOnly` and executed without approval, because the
whitespace-bearing argument never reaches `has_path_escape`. Read-only
classifications run without any user confirmation, so a single positional
argument that bypasses the path checker reads arbitrary files outside the
workspace. This plan closes the bypass by applying the existing escape
detection to whitespace-bearing arguments whose content looks like a path, while
leaving free-form message arguments (prompts, JSON blobs) untouched.

## Current state

The relevant file is `src-tauri/src/shell_tools.rs`. The argument-parsing
helpers at lines 599-611:

```rust
fn argument_path_value(arg: &str) -> Option<&str> {
    let value = if let Some(option) = arg.strip_prefix("--") {
        option.split_once('=').map(|(_, value)| value)
    } else if arg.starts_with('-') {
        arg.split_once('=')
            .map(|(_, value)| value)
            .or_else(|| arg.get(2..))
    } else {
        Some(arg)
    };

    value.filter(|v| is_path_like(v))
}
```

The `is_path_like` filter at lines 570-583:

```rust
fn is_path_like(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Reject whitespace-containing values
    if s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    // Reject other clearly non-path-like free-form values (e.g. containing quotes or JSON-like braces/brackets)
    if s.chars().any(|c| matches!(c, '"' | '\'' | '{' | '}' | '[' | ']')) {
        return false;
    }
    true
}
```

And the escape detector at lines 752-780:

```rust
fn has_path_escape(args: &[String], workspace_root: &Path) -> Option<String> {
    let workspace_canonical = match std::fs::canonicalize(workspace_root) {
        Ok(c) => c,
        Err(e) => return Some(format!("failed to canonicalize workspace root: {}", e)),
    };

    for arg in args {
        let Some(path_string) = argument_path_value(arg).filter(|value| !value.is_empty()) else {
            continue;
        };
        // ... absolute/relative checks follow
    }
    None
}
```

Because `argument_path_value` returns `None` for whitespace-bearing values,
`has_path_escape` simply skips them, and `classify_shell` then falls through to
the read-only allowlist at lines 152-155:

```rust
// Small read-only allowlist runs directly.
if program_is_bare_name && is_read_only_shell_program(&executable_lower, args) {
    return CommandSafety::ReadOnly;
}
```

Read-only classifications reach `CommandExecutor::run_with_approval` at line 945
and execute immediately without user approval.

## Commands you will need

| Purpose         | Command                                              | Expected on success               |
|-----------------|------------------------------------------------------|-----------------------------------|
| Compile check   | `cargo check --manifest-path src-tauri/Cargo.toml`   | exit 0, no errors                 |
| Lint            | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | exit 0       |
| Run tests       | `cargo test --manifest-path src-tauri/Cargo.toml`    | exit 0, all pass (incl. new tests)|

## Scope

**In scope**:
- `src-tauri/src/shell_tools.rs` — argument parsing helpers, escape detection, and inline tests.

**Out of scope**:
- Changes to the `CommandSafety` enum or to read-only classification logic.
- Changes to approval / dialog behavior.
- Changes to other tools or platforms.
- Changes to argument validation in other modules (e.g. `workspace_tools.rs`).

## Git workflow

- Branch: `advisor/006-close-whitespace-shell-path-bypass`
- Commit message style: `security: detect workspace escapes in whitespace-bearing path arguments`
- Match the existing single-purpose commits in `git log --oneline` for the
  shell-tools module.

## Steps

### Step 1: Distinguish path-shaped whitespace arguments from free-form text

Modify `argument_path_value` so it still returns `Some(value)` for arguments
that look like filesystem paths even when the value contains whitespace, while
still returning `None` for arguments that are obviously free-form text (prompts,
JSON snippets, prose with spaces, single-quoted text).

Decision rule (apply only to the value that the function would currently
return — i.e. the post-`=` part for flags, the post-`-` glued part for short
flags, or the whole argument otherwise):

1. The value must contain at least one path separator (`/` on Unix; on Windows
   additionally `\`), OR begin with a drive letter (`X:`), OR begin with
   `~` (home-relative), OR begin with `.` followed by `/` (relative).
2. The first character must not be a quote (`'`, `"`) — those are shell-style
   quoted strings, treated as free-form.
3. The value must not contain any shell metacharacter already rejected by
   `contains_shell_metacharacter` (`; | & $ \` < >` newlines, NUL).

Refactor by adding a new helper `is_path_like_extended(value: &str) -> bool`
that returns `true` when the strict-but-permissive rule above is satisfied, and
use it from `argument_path_value` in place of `is_path_like`. Keep
`is_path_like` for any other callers (search for usages — there are none
today, but the helper is exported and worth keeping for clarity).

Sketch:

```rust
fn is_path_like_extended(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    // Free-form strings wrapped in quotes should not be treated as paths.
    if let Some(first) = value.chars().next() {
        if first == '"' || first == '\'' {
            return false;
        }
    }
    // Reject shell metacharacters and control characters outright; existing
    // argument-safety checks already handle these, but a path should never
    // contain them either.
    if value.chars().any(|c| {
        c.is_control() || matches!(c, ';' | '|' | '&' | '$' | '`' | '<' | '>')
    }) {
        return false;
    }
    // Must contain a path-shape signal: a separator, a drive root, a tilde,
    // or an explicit relative marker.
    let has_separator = value.contains('/')
        || (cfg!(windows) && value.contains('\\'));
    let starts_with_drive = value.len() >= 2
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value.as_bytes()[1] == b':';
    let starts_with_tilde = value.starts_with('~');
    let starts_with_dot_slash = value.starts_with("./") || value.starts_with("../");
    has_separator || starts_with_drive || starts_with_tilde || starts_with_dot_slash
}
```

Then update `argument_path_value`:

```rust
fn argument_path_value(arg: &str) -> Option<&str> {
    let value = if let Some(option) = arg.strip_prefix("--") {
        option.split_once('=').map(|(_, value)| value)
    } else if arg.starts_with('-') {
        arg.split_once('=')
            .map(|(_, value)| value)
            .or_else(|| arg.get(2..))
    } else {
        Some(arg)
    };

    value.filter(|v| is_path_like_extended(v))
}
```

Drive-letter detection in Rust is a small fixup — replace the snippet above
with this cleaner version:

```rust
let starts_with_drive = value.len() >= 2
    && value.as_bytes()[0].is_ascii_alphabetic()
    && value.as_bytes()[1] == b':';
```

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` exits 0.

### Step 2: Reject `/etc`-style arguments with spaces from the read-only allowlist

The existing `is_read_only_shell_program` allowlist at lines 663-694 and
`classify_shell` lines 152-155 must not classify commands with path-shaped
whitespace arguments as `ReadOnly`. The mechanism for that is already in place:
once `argument_path_value` returns the whitespace-bearing path, `has_path_escape`
returns `Some(...)` and `classify_shell` returns `CommandSafety::Mutating` at
line 148-150:

```rust
if has_path_escape(args, workspace_root).is_some() {
    return CommandSafety::Mutating;
}
```

Confirm that line ordering (read-only allowlist at line 153 comes **after** the
escape check at line 148) is preserved. Do not reorder it.

For the path to actually fail the escape check, `candidate_escapes_workspace`
must canonicalize it. For an absolute path the function calls
`std::fs::canonicalize` directly at lines 782-806, which works for paths with
spaces because canonicalize operates on the raw OS path. Existing tests confirm
the helper handles absolute and relative paths (see
`classify_shell_requires_approval_for_external_path` at lines 1650-1658 and
`classify_shell_requires_approval_for_flag_style_path_escape` at lines 1660-1686).

No new logic in `has_path_escape` or `candidate_escapes_workspace` is required;
Step 1's change in `argument_path_value` is sufficient. Read the function
bodies end-to-end to confirm, and update any in-scope comments that name
`is_path_like` so they refer to `is_path_like_extended` instead.

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` exits 0.

### Step 3: Add regression tests for whitespace-bearing path arguments

Add inline tests in `src-tauri/src/shell_tools.rs` (the `mod tests` block at
line 1360). The existing `classify_shell_requires_approval_for_external_path`
test at line 1650 is a good template. New tests, all platform-portable:

1. **`classify_shell_requires_approval_for_external_spaced_path`** — `cat
   "/etc/some spaced dir/passwd"` (or with a real existing spaced path under
   `tempfile::tempdir()`) is classified as `Mutating`, not `ReadOnly`. The
   argument value passed to `classify_shell` should be a single `String`
   containing a space. Verify it is **not** classified as `ReadOnly` (the
   exact safety must be `Mutating` when escape is detected, `Blocked` only
   when the secret-like filter trips — that filter rejects names like `.env`
   and would not trip on `/etc/some spaced dir/passwd`, so the expected
   safety is `Mutating`).

2. **`classify_shell_permits_safe_in_workspace_spaced_path`** — a path-shaped
   value that lives inside the workspace and contains a space is classified
   as `ReadOnly` (e.g. `cat "docs/some file.txt"` inside the project root).
   This guards against the new check being too eager: a real path with
   spaces inside the workspace should still be allowed as read-only.

3. **`classify_shell_ignores_freeform_text_with_spaces`** — an argument that
   is clearly free-form text (e.g. `cat "hello world how are you"`) is
   classified by the read-only allowlist (i.e. `ReadOnly`), not by the path
   checker. This guards against the new check breaking normal usage of `cat`
   with a free-form single argument that has no path shape. The argument
   must contain no `/`, no leading drive letter, no leading `~`, no
   `./`/`../`. The function's existing read-only allowlist lets `cat` run
   on a single positional regardless, so `ReadOnly` is the expected result.

4. **`classify_shell_requires_approval_for_flag_with_spaced_value`** — a flag
   argument whose value is a path with spaces, e.g.
   `--output=/etc/some spaced dir/x`, is classified as `Mutating`. Same
   shape as test 1 but exercised via the flag value extractor.

Use `tempfile::tempdir()` to construct real on-disk paths for tests 1 and 2.
For test 2, pass that tempdir as the policy workspace root so the spaced path
is actually in-workspace. Keep the external-path tests rooted at the normal
workspace helper.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml -- shell_tools::tests::` exits 0 and the four new tests appear in the output.

### Step 4: Update existing tests if they incidentally relied on whitespace bypass

Search the test module for any test that passes a string with spaces to
`classify_shell` and asserts `ReadOnly` or `Mutating`. The likely candidates
are the existing `classify_shell_allows_read_only_programs` (line 1506) and
`classify_shell_requires_approval_for_flag_style_path_escape` (line 1660).
Verify those assertions are unaffected. If any test expected a free-form
spaced argument to be ignored (test 3 above confirms this is the desired
behavior), do not change it.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml` exits 0 with all
pre-existing tests still passing.

### Step 5: Run lint and full test suite

Run the broader checks called out in the repo's standard pipeline to confirm
no regressions.

- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — exit 0.
- `cargo test --manifest-path src-tauri/Cargo.toml` — exit 0, all tests pass.

**Verify**: Both commands exit 0.

## Test plan

- New tests (in `src-tauri/src/shell_tools.rs`):
  - `classify_shell_requires_approval_for_external_spaced_path`
  - `classify_shell_permits_safe_in_workspace_spaced_path`
  - `classify_shell_ignores_freeform_text_with_spaces`
  - `classify_shell_requires_approval_for_flag_with_spaced_value`
- Existing tests that must continue to pass: the entire `shell_tools::tests`
  module (~30 tests).
- Verification: `cargo test --manifest-path src-tauri/Cargo.toml` exits 0 with
  4 more tests than before.

## Done criteria

- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` exits 0
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0; 4 new tests
      exist and pass
- [ ] No files outside `src-tauri/src/shell_tools.rs` are modified
- [ ] `plans/README.md` status row for plan 006 updated to `DONE`

## STOP conditions

- The drift check shows non-empty output for `src-tauri/src/shell_tools.rs`
  between `8c6e3ad` and `HEAD`. Re-open the file, reconcile the "Current
  state" excerpts, and treat the mismatch as a planning defect.
- The new helper `is_path_like_extended` rejects non-whitespace paths the
  existing `is_path_like` accepted in production code. It should only add
  recognition for whitespace-bearing values with a valid path shape; control
  characters remain rejected. If any other test regresses, STOP.
- The pre-existing `classify_shell_requires_approval_for_external_path`
  test starts failing. This indicates the new path is not flowing through
  `has_path_escape`; STOP and re-read Step 1.
- The drift check shows the file has been refactored such that line numbers
  in "Current state" no longer match. Update the plan rather than guessing.

## Maintenance notes

- The escape detection works on the value as passed by the agent. A future
  improvement could canonicalize-and-prefix-match before classification, but
  this plan stays within the existing `candidate_escapes_workspace` design
  to keep the change minimal.
- If a future plan introduces a wider argument parser (e.g. one that
  handles quoted-string splitting), the new helper should be re-evaluated
  in that context. The current helper is conservative by design — it only
  treats whitespace-bearing values as paths when the value also has a path
  separator or path-root marker.
- A reviewer should look for accidental expansion of the read-only
  allowlist; this plan does not change it, and any future change here
  should preserve the order: `common_shell_safety` → `has_path_escape` →
  read-only allowlist.
