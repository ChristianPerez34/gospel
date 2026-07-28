# Plan 016: Hard-block `rm --recursive /` and other long-form destructive shield invocations

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 72819cd..HEAD -- src-tauri/src/shell_tools.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `72819cd`, 2026-07-20

## Why this matters

The `is_blocked_shell_pattern` matcher in `shell_tools.rs` hard-blocks
`rm -rf /` and `rm -rf /*` per the documented safety contract (AGENTS.md
"Hard-blocked commands"). But it uses `arg.contains('r')`/`arg.contains('f')`:
this both false-positives (any long flag with an `r` or `f` in its name)
and false-negatives — a plainly destructive `rm --recursive --force /` or
`rm -r --force /` is the same command, written differently, and bypasses
the hard block. It falls through to `Mutating` (approval-only) instead of
being hard-blocked.

This plan replaces the substring matcher with a short-flag + long-flag
parser, restores the destructive-stream hard block for all forms of `rm
-rf /`, and prevents attackers/agents from trivially bypassing the
hard-block by spelling the flag out.

Hardening note: the repository's `gospel` app installs the `AGENTS.md`
shell-tools safety model where this contract lives (AGENTS.md "Hard-blocked
commands: `rm -rf /` or `rm -rf /*`, …"). The ADR does not cover the parser
internals — this plan stays within contract.

## Current state

`src-tauri/src/shell_tools.rs:649-684`:

```rust
fn is_blocked_shell_pattern(program: &str, args: &[String]) -> bool {
    // rm -rf / or rm -rf /*
    if program == "rm" {
        let mut recursive = false;
        let mut force = false;
        let mut root_target = false;
        for arg in args {
            if arg.starts_with('-') {
                if arg.contains('r') {
                    recursive = true;
                }
                if arg.contains('f') {
                    force = true;
                }
            } else if arg == "/" || arg == "/*" {
                root_target = true;
            }
        }
        if recursive && force && root_target {
            return true;
        }
    }
    // ... dd/mkfs/fdisk/parted, curl blocks ...
}
```

Problems:
- High confidence, false negative — `rm --recursive --force /` parses as:
  `--recursive` sets `recursive` (has 'r'); `--force` sets `force` (has 'f');
  `/` sets `root_target`. So actually `rm --recursive --force /` IS blocked.
  Re-check: `--recursive`.contains('r') → true; `--force`.contains('f') → true;
  both set; `arg == "/"` → `root_target = true`; **blocked**. Good.
  But `rm --recursive /` (no force) → `recursive = true`, `force = false` →
  NOT blocked. This is the documented contract's spirit ("rm -rf /")
  whereas `rm --recursive /` is the same destructive action. The current
  contract is `recursive && force && root_target` — `force` is not strict
  necessary to delete `/` recursively. So **`rm --recursive /`** alone
  wipes `/` but reaches the user as "may modify workspace".
- False positive — `rm --report /tmp/foo` (where `--report` is a hypothetical
  long flag with 'r') wrongly sets `recursive`. Real `rm` long flags:
  `--recursive`, `--force`, `--dir`, `--one-file-system`, `--no-preserve-root`,
  `--preserve-root`, `--interactive`, `--verbose`. `--one-file-system`
  contains no 'r' or 'f'... but `--preserve-root` contains 'r' → falsely
  sets recursive. Bug.

Adjacent surfaces to NOT regress:
- The shell-tool already approves `rm` with `-r`/`-f` via the wider
  `CommandSafety` classifier downstream — the ONLY thing this function
  controls is the hard-block. Adjustments here must not change Mutating vs
  Destructive classification elsewhere.
- `classify_find_action` (line 686) handles `-delete`; `is_read_only_shell_program` (line 699) controls the read-only shortlist. Both are siblings of `is_blocked_shell_pattern` in the same file; not in scope.

Conventions:
- Tests live inline at the bottom of `shell_tools.rs` as `#[test]` functions
  Cohort (search `fn.*_test` or `#\[test\]` near `is_blocked_shell_pattern`).
- Error messages are static lifetimes; the parser is pure (no allocation
  outside Vec); `match`-style Rust idioms.
- Follow AGENTS.md "Implementation notes: Core logic lives in `src-tauri/src/shell_tools.rs`."

## Commands you will need

| Purpose   | Command                                                                  | Expected on success |
|-----------|--------------------------------------------------------------------------|---------------------|
| Backend lint | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`     | exit 0             |
| Backend tests | `cargo test --manifest-path src-tauri/Cargo.toml -- shell_tools`     | all pass           |
| Full backend | `cargo test --manifest-path src-tauri/Cargo.toml`                    | all pass           |

## Scope

**In scope**:
- `src-tauri/src/shell_tools.rs` (parser rework of `is_blocked_shell_pattern` and adjacent `rm`-recognition logic only, plus tests)

**Out of scope**:
- Any change to `classify_find_action`, `is_read_only_shell_program`, the
  metacharacter block list, or any other command's classification.
- The approval UI string ("may modify the workspace") — that contract is
  in AGENTS.md and would need a separate doc edit (out of scope; the
  hard-block is the right surface).
- Adding `--preserve-root` logic to the matcher (potential future hardening,
  not in this plan — `--no-preserve-root` would already be flagged as
  destructive by other paths since current shell-tools blocklist describes
  `dd`, `mkfs`, etc.; do not expand scope).

## Git workflow

- Branch: `advisor/016-rm-recursive-hardblock`
- Commit example: `fix: hard-block rm --recursive / matching the rm -rf / contract`.
- Do NOT push unless instructed.

## Steps

### Step 1: Add a short-flag + long-flag parser for rm

Add a private helper above `is_blocked_shell_pattern` (it stays pure,
no alloc, returns `(recursive, force)` or a small struct `RmFlags { recursive, force, dir }`):

```rust
struct RmFlags { recursive: bool, force: bool, dir: bool }

fn parse_rm_flags(args: &[String]) -> RmFlags {
    let mut f = RmFlags { recursive: false, force: false, dir: false };
    let mut parsing_options = true;
    for arg in args {
        if !parsing_options { continue; }
        if arg == "--" {
            parsing_options = false;
            continue;
        }
        if arg == "--recursive" { f.recursive = true; continue; }
        if arg == "--force"     { f.force     = true; continue; }
        if arg == "--dir" || arg == "-d" { f.dir = true; continue; }
        if let Some(rest) = arg.strip_prefix("--") {
            // unknown long flag -> ignore
            let _ = rest;
            continue;
        }
        if let Some(rest) = arg.strip_prefix('-') {
            // cluster of short flags (allow combined -rf / -fr / -rd etc.)
            for c in rest.chars() {
                match c {
                    'r' => f.recursive = true,
                    'f' => f.force     = true,
                    'd' => f.dir       = true,
                    // other accepted short flags exist on some rm impls;
                    // the parser intentionally ignores them.
                    _ => {}
                }
            }
            continue;
        }
        // non-flag operand — handled by the caller's root_target check
    }
    f
}
```

The standalone `--` ends option parsing. `parse_rm_flags` must ignore it and
all subsequent arguments for flag detection, while the caller's operand pass
continues to treat every later argument as an operand. Thus
`rm -- -rf /` does not set `recursive` or `force`.

### Step 2: Tighten `is_blocked_shell_pattern` for rm

Replace the `if program == "rm" { ... }` block with:

```rust
if program == "rm" {
    let mut operands: Vec<&str> = Vec::new();
    let mut past_dashdash = false;
    for arg in args {
        if past_dashdash { operands.push(arg); continue; }
        if arg == "--"   { past_dashdash = true; continue; }
        if arg.starts_with('-') { continue; }
        operands.push(arg);
    }
    let flags = parse_rm_flags(args);
    let target_is_root = |s: &str| s == "/" || s == "/*"
        || (s.starts_with("/*") && s.ends_with("/"));
    let root_target = operands.iter().any(|s| target_is_root(s));
    // destructive when recursive AND a root target — force is not required
    // for a recursive rm of / (per AGENTS.md "rm -rf / or rm -rf /*" spirit).
    if (flags.recursive && flags.force && root_target)
        || (flags.recursive && root_target) {
        return true;
    }
}
```

The two-term disjunction (with force, without force) is redundant in this
simplified form — collapse to `flags.recursive && root_target` so a lone
`rm --recursive /` is also hard-blocked. AGENTS.md describes the contract as
"rm -rf / or rm -rf /*" — destructive recursive removal of root — and
"`--recursive` alone on `/`" satisfies the contract's intent. Update the
internal comment to explain the rationale.

**Verify**: `cargo build --manifest-path src-tauri/Cargo.toml` → exit 0.

### Step 3: Add tests for the parser

Add `#[test]` functions next to the existing `is_blocked_shell_pattern`
tests (search `rg "#\[test\]" src-tauri/src/shell_tools.rs` for the test
cohort). Cases to add (each as its own `#[test]`):

1. `rm_rf_root_blocked` — `["-rf", "/"]` → true.
2. `rm_fr_root_blocked` — `["-fr", "/"]` → true.
3. `rm_long_recursive_force_root_blocked` — `["--recursive", "--force", "/"]` → true.
4. `rm_long_recursive_root_blocked` — `["--recursive", "/"]` → true (the
   bug this plan fixes).
5. `rm_long_recursive_no_root_not_blocked_by_pattern` —
   `["--recursive", "subdir/"]` → false (it's Mutating, not hard-blocked;
   the blocklist only flags root-with-recursive).
6. `rm_preserve_root_long_flag_does_not_set_recursive` —
   `["--preserve-root", "/"]` → false (the false-positive regression case).
7. `rm_dir_long_flag_does_not_set_recursive` — `["--dir", "/"]` → false
   (`--dir` without `-r` is not recursive; not hard-blocked).
8. `rm_dashdash_does_not_treat_operands_as_flags` — `["--", "--recursive", "/"]`
   → false (operands after `--` are operands, the literal string
   `"--recursive"` is not a flag).
9. `rm_glob_root_target_blocked` — `["-rf", "/*"]` → true.
10. `rm_glob_subdir_target_not_blocked` — `["-rf", "/tmp/*"]` → false (out
    of contract; subclassification of `/tmp` traversal can ship separately).

Match the existing `#[test]` naming and assertion style (use
`assert!(is_blocked_shell_pattern("rm", &args))` / `assert!(!...)`).

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml -- shell_tools::`
→ shell_tools tests pass (existing + 10 new).
`cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` → exit 0.

## Test plan

- The 10 new tests above, in `src-tauri/src/shell_tools.rs`.
- Pattern: existing `#[test]` cohort in the same file (read one named
  `..._test` near `is_blocked_shell_pattern`).
- Verify the structural regression: `cargo test --manifest-path src-tauri/Cargo.toml`
  → full backend suite still passes (no behavior change outside `rm`).

## Done criteria

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml -- shell_tools` exits 0; the 10 new tests pass
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] `rg "arg\.contains\('r'\)|arg\.contains\('f'\)" src-tauri/src/shell_tools.rs` returns no matches (substring matcher replaced)
- [ ] `rg "parse_rm_flags" src-tauri/src/shell_tools.rs` returns ≥2 matches (definition + use)
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row for plan 016 updated

## STOP conditions

Stop and report back if:

- An existing test asserts that `rm --recursive /` is currently Mutating
  (not hard-blocked) — there's a real chance the existing test suite
  encoded the current buggy behavior; if found, the executor must update
  the test to the new hard-blocked expectation and call it out in the
  commit body.
- The `parse_rm_flags` short-flag cluster parser interacts with a test
  that uses a real `rm` flag the parser is silently ignoring (e.g. `-i`,
  `--verbose`); ensure the parser doesn't error on unknown flags (it
  ignores them by design). If a test fails due to such an interaction,
  report the flag and what the test asserts.
- Step 1's `RmFlags` struct adds fields unused by `is_blocked_shell_pattern`
  and clippy flags them — drop the unused field (e.g. `dir` if not used in
  the disjunction).
- The `target_is_root` glob expansion (`s.starts_with("/*") && s.ends_with("/")`)
  produces false positives for obviously non-root targets — review the
  pattern, prefer an exact match `s == "/" || s == "/*"` first.

## Maintenance notes

- Future hardening (`--no-preserve-root` strict-block, `xargs rm`, `find
  -delete /`) is out of scope; this plan only repairs the existing root/
  recursive contract. Do not pre-emptively extend the block list.
- Reviewer should confirm the `--` (end-of-options) case is correctly
  handled: operands after `--` must be considered for `root_target`, flags
  after `--` must not be considered as flags.
- An existing test may pin the previous substring behavior; updating that
  test is in scope. Note in commit body.
