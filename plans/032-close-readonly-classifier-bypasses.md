# Plan 032: Close Read-Only Classifier Bypasses (`rg --pre`, `git --output`, `gh api --input`)

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 2982ea5..HEAD -- src-tauri/src/shell_tools.rs`
> If this file changed since this plan was written, compare the "Current state"
> excerpts against the live code before proceeding; on a mismatch, treat it as
> a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `2982ea5`, 2026-10-10

## Why this matters

Gospel exposes three agent-facing command tools — `run_shell_command`, `run_git_command`, `run_github_cli_command` — governed by a `CommandPolicy` classifier in `src-tauri/src/shell_tools.rs`. Read-only commands execute with no approval; mutating/destructive commands require a one-time user approval via a native dialog; hard-blocked commands always fail.

Three argv shapes currently slip write/execute capability through the **read-only** classification:

1. `rg --pre <program>` makes ripgrep spawn `<program>` once per searched file. The agent can stage a script inside the workspace via `source_edit` or `write_harness_file` (`.gospel/` is agent-writable) and execute it — a complete bypass of both the approval gate and the shell-interpreter hard-block.
2. `git diff --output=<file>` and `git show --output=<file>` write generated diff content to a workspace file — a file-write primitive that skips `source_edit`'s exact-replacement safeguards entirely.
3. `gh api <endpoint> --input <file>` sends the file contents as the request body; `gh api` defaults to POST when a body is present, so it performs remote mutations (create/close issues, merge PRs, graphql deletions) classified read-only.

Each is a one-line-of-logic fix matching helpers that already exist in the same file.

## Current state

The only file involved is `src-tauri/src/shell_tools.rs` (~2,580 LOC), which owns the `CommandSafety` classifier, the program allowlists, and the executor.

`CommandSafety` enum at the top of the file:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSafety {
    ReadOnly,
    Mutating,
    Destructive,
    Blocked(String),
}
```

`is_read_only_shell_program` (~lines 751–783) holds the shell read-only allowlist plus per-program argument carve-outs — `find` already loses read-only status on `-delete`/`-exec`/`-execdir`/`-ok`/`-okdir` (lines ~739–744), and `sort`/`uniq` lose it on output-destination flags (lines ~772–779). `rg` is in the allowlist with **no** argument carve-out:

```rust
fn is_read_only_shell_program(program: &str, args: &[String]) -> bool {
    if !matches!(
        program,
        "ls" | "cat"
            | "head"
            | "tail"
            | "find"
            | "grep"
            | "rg"
            | "pwd"
            | "echo"
            | "test"
            ...
            | "which"
            | "wc"
            | "sort"
            | "uniq"
    ) {
        return false;
    }

    if program == "sort" && sort_has_output_destination(args) {
        return false;
    }

    if program == "uniq" && uniq_has_output_destination(args) {
        return false;
    }

    true
}
```

There is already a helper for exactly this bug shape — `sort_has_output_destination` at line ~799:

```rust
fn sort_has_output_destination(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "-o"
            || arg == "--output"
            || (arg.starts_with("-o") && arg.len() > 2)
            || arg.starts_with("--output=")
    })
}
```

The existing `--pre` handling at ~lines 918–926 is only for *path-escape checking* (it treats the `--pre` value as a path operand) — it does not gate execution:

```rust
        "rg" => arg
            .strip_prefix("--pre=")
            .or_else(|| matches!(previous.map(String::as_str), Some("--pre")).then_some(arg)),
```

`classify_git` (~lines 200–243) returns `ReadOnly` unconditionally for `diff`/`show`/`log` with no output-destination check:

```rust
        // Read-only subcommands.
        if matches!(
            subcommand.as_str(),
            "status"
                | "log"
                | "diff"
                | "show"
                | "blame"
                | "ls-files"
                | "ls-tree"
                | "rev-parse"
                | "describe"
        ) {
            return CommandSafety::ReadOnly;
        }
```

`gh api` classification (~lines 368–377) delegates to `gh_api_uses_mutating_method` (~lines 395–444), which inspects `-X`/`--method`, `-f`/`-F`/`--field`/`--raw-field` — but not `--input`:

```rust
fn gh_api_uses_mutating_method(args: &[String]) -> bool {
    let mut explicit_method: Option<String> = None;
    let mut has_field_params = false;
    let mut iter = args.iter().peekable();

    while let Some(arg) = iter.next() {
        ...
        if arg == "-f" || arg == "-F" || arg == "--field" || arg == "--raw-field" {
            has_field_params = true;
            let _ = iter.next();
            continue;
        }
        if (arg.starts_with("-f") && arg.len() > 2)
            || (arg.starts_with("-F") && arg.len() > 2)
            || arg.starts_with("--field=")
            || arg.starts_with("--raw-field=")
        {
            has_field_params = true;
        }
    }
    ...
}
```

**Test conventions**: classifier tests live in `#[cfg(test)] mod tests` at the bottom of the same file (~1,900+). They build `Vec<String>` arg lists and assert the `CommandSafety` variant — see `classify_git_allows_read_only_subcommands` (~line 2130), `classify_gh_treats_api_post_as_mutating` (~line 2399), and `classify_shell_requires_approval_for_flag_style_path_escape` (~line 2033) for the exact construction pattern. Match the existing naming (`classify_<tool>_<behavior>`).

## Commands you will need

| Purpose   | Command                                              | Expected on success |
|-----------|------------------------------------------------------|---------------------|
| Rust tests (this file) | `cargo test --manifest-path src-tauri/Cargo.toml shell_tools` | all tests pass |
| Full Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all pass |
| Clippy    | `bun run clippy`                                     | exit 0, no warnings |
| Lint      | `bunx biome check .`                                  | exit 0              |

## Scope

**In scope** (the only files you should modify):
- `src-tauri/src/shell_tools.rs` — three classifier checks + new unit tests in its test module

**Out of scope** (do NOT touch, even though they look related):
- `SHELL_TOOLS_SYSTEM_PROMPT` (top of the same file) — the prompt's safety list describes *hard-blocks*; these fixes reclassify to `Mutating` (approval-gated), not `Blocked`, so the prompt stays accurate. Do not add new hard-blocks.
- `src-tauri/src/approval_broker.rs`, `src-tauri/src/harness_profile.rs`, `src-tauri/src/lib.rs` — the approval plumbing is correct; only the classification is wrong.
- Any other program's classification (e.g., `find` `-exec` handling, `gh api` `-X` handling) — already correct.
- Do NOT hard-block `--pre`/`--output`/`--input`: they have legitimate uses; `Mutating` (user approval) is the right classification.

## Git workflow

- Branch: `advisor/032-close-readonly-classifier-bypasses`
- Commit style: conventional commits, e.g. `fix(shell): classify rg --pre as mutating` (see `git log` — `fix(providers): ...`, `feat(sessions): ...`)
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: `rg --pre` / `--pre-glob` lose read-only status

Add a helper next to `sort_has_output_destination`:

```rust
fn rg_uses_preprocessor(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "--pre"
            || arg == "--pre-glob"
            || arg.starts_with("--pre=")
            || arg.starts_with("--pre-glob=")
    })
}
```

In `is_read_only_shell_program`, beside the `sort`/`uniq` carve-outs add:

```rust
    if program == "rg" && rg_uses_preprocessor(args) {
        return false;
    }
```

Rationale: `--pre <prog>` and `--pre=<prog>` spawn a preprocessor per file; `--pre-glob` selects which files get the preprocessor (still enables it). Returning `false` drops the invocation to the default `Mutating` classification → one-time approval, consistent with the `find -exec` carve-out.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml shell_tools` → existing tests still pass (behavior additive).

### Step 2: `git diff`/`show`/`log --output` classified mutating

Add a helper:

```rust
fn git_subcommand_writes_output(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--output" || arg.starts_with("--output="))
}
```

In `classify_git`, immediately before the `// Read-only subcommands.` block, add:

```rust
        if matches!(subcommand.as_str(), "diff" | "show" | "log")
            && git_subcommand_writes_output(&args[1..])
        {
            return CommandSafety::Mutating;
        }
```

Correctness notes for the executor:
- Do NOT match bare `-o` or `arg.starts_with("-o")` — git `diff`/`show`/`log` have no `-o` short flag, and a prefix match would false-positive `--oneline`.
- `--output-indicator-context`, `--output-indicator-new`, `--output-indicator-old` are unrelated diff flags; the `--output=` prefix match does not catch them (`--output-i` ≠ `--output=`). Do not add broader matching.
- `git format-patch` already writes files but is not on the read-only list — leave it.
- `status`, `blame`, `ls-files`, `ls-tree`, `rev-parse`, `describe` have no output-redirect flag — leave them.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml shell_tools` → pass.

### Step 3: `gh api --input` classified mutating

In `gh_api_uses_mutating_method`, add two branches inside the `while let` loop, next to the field-param handling:

```rust
        if arg == "--input" {
            has_field_params = true;
            let _ = iter.next();
            continue;
        }
        if arg.starts_with("--input=") {
            has_field_params = true;
        }
```

Rationale: `--input <file>` supplies a request body; `gh api` sends POST by default when a body is present, which is exactly the mutating-path contract `-f`/`--field` already enforces.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml shell_tools` → pass.

### Step 4: Unit tests

Add to the existing `#[cfg(test)] mod tests` at the bottom of `shell_tools.rs`, modeled on the existing classifier tests:

- `classify_shell_rg_pre_is_mutating` — `rg --pre ./script.sh foo` and `rg --pre=./script.sh foo` → `CommandSafety::Mutating`; `rg --pre-glob '*.bin' --pre ./s foo` → `Mutating`
- `classify_shell_rg_plain_stays_read_only` — `rg foo src/` → `ReadOnly`
- `classify_git_output_destination_is_mutating` — `git diff --output=x.patch`, `git show --output=x`, `git log --output x` → `Mutating`; `git diff --oneline` and `git diff --output-indicator-context=all` → `ReadOnly` (false-positive guards)
- `classify_gh_api_input_is_mutating` — `gh api /x --input body.json` and `gh api /x --input=body.json` → `Mutating`; `gh api /x` → `ReadOnly`
- `classify_gh_api_input_with_explicit_get` — decide the contract explicitly: `gh api /x --input f -X GET` → assert `Mutating` (a body on GET is still non-standard; matching `-f` behavior which also forces Mutating unless... check the existing tail of `gh_api_uses_mutating_method` — if an explicit read-only method overrides `has_field_params`, mirror that behavior and assert whatever the code does; do not invent new semantics).

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml shell_tools` → all pass including ≥5 new tests. `bun run clippy` → exit 0.

## Test plan

Covered by Step 4 — classifier unit tests in the file's existing test module, modeled on `classify_git_allows_read_only_subcommands` (~line 2130) and `classify_gh_treats_api_post_as_mutating` (~line 2399).

Verification: `cargo test --manifest-path src-tauri/Cargo.toml` → all pass.

## Done criteria

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0; ≥5 new classifier tests exist and pass
- [ ] `bun run clippy` exits 0
- [ ] `bunx biome check .` exits 0 (unchanged — Rust file only)
- [ ] `grep -n '"rg"' src-tauri/src/shell_tools.rs` shows an `rg_uses_preprocessor` carve-out in `is_read_only_shell_program`
- [ ] `git diff --output=x` classifies `Mutating` (asserted by test); `gh api --input` classifies `Mutating` (asserted by test)
- [ ] Only `src-tauri/src/shell_tools.rs` modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The code at the cited locations doesn't match the excerpts (file refactored since `2982ea5`).
- `is_read_only_shell_program`'s signature or call structure changed such that a per-program carve-out can't be expressed the way `sort`/`uniq` do it — describe the new structure instead of forcing the pattern.
- A test in Step 4 fails twice after a reasonable fix attempt.
- You find that `gh api --input` already has handling via a path this plan didn't see (e.g., a generic body-file detector) — then the finding is stale; report instead of double-fixing.
- Correct classification appears to require `Blocked` (permanent deny) rather than `Mutating` for one of these — the safety model deliberately keeps them approvable; report before changing that contract.

## Maintenance notes

- Future read-only allowlist additions must be reviewed for "flag makes the tool exec/write" the way `--pre`, `-exec`, and `--output` do — this is the recurring failure class (see also `find -exec`, `sort -o` precedents).
- A reviewer should scrutinize: the `--pre-glob` inclusion (it only *selects* files for the preprocessor — including it is conservative and correct), and the `git log` coverage (`git log --output` is obscure but real).
- Deferred out of scope: `git push` force-variant spellings (`-fu`, `--force-with-lease` currently classify `Mutating` instead of the documented hard-block) and `cancel_streaming`'s missing session-access guard — separate small security items recorded in `plans/README.md`'s rejected ledger.
