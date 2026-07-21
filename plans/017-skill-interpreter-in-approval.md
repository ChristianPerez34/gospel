# Plan 017: Surface resolved skill-script interpreter in approval request

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 72819cd..HEAD -- src-tauri/src/skills.rs src-tauri/src/approval_broker.rs`
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

Skill scripts (`<skill>/scripts/<name>`) are user-authored content running with
the user's privileges. The approval dialog currently labels them as
`"Execute skill script '<script>' for skill '<skill>'"` with no mention of
the resolved interpreter. Because `detect_interpreter_from_content` reads the
shebang and accepts arbitrary interpreters (including
`/usr/bin/env bash -c '…'` or `python3 -u …`), a workspace skill can hide an
arbitrary shell interpreter behind a benign filename. The user gets the same
"Run script 'hello' for skill 'X'" prompt whether the interpreter is `node`
or `/bin/bash` with extra args. Surfacting the interpreter in the approval
label closes the gap — purely additive, no behavior change for legitimate
skills.

## Current state

- `src-tauri/src/skills.rs:217-228` — constructs the `CommandApprovalRequest`
  for the skill script run. The label and reason omit the interpreter:
  ```rust
  // (paraphrased — read the actual line numbers in the live file)
  CommandApprovalRequest {
      command_label: format!("Execute skill script '<{}>' for skill '<{}>'", script, skill),
      reason: format!("Run script at {}/scripts/{}", skill, script),
      // ...
  }
  ```
- `src-tauri/src/skills.rs:687-720` — `detect_interpreter_from_content`
  parses shebangs and returns an interpreter token list (e.g.
  `["/usr/bin/env", "bash"]` or `["/usr/bin/python3", "-u"]`); it accepts
  arbitrary interpreter paths and an arbitrary number of trailing args.
- `src-tauri/src/skills.rs:816-830` — `run_skill_script` builds
  `tokio::process::Command::new(interpreter_parts[0])` from the parsed
  shebang then chains `.args(&interpreter_parts[1..])` (read the live code;
  the executor must match it).
- `src-tauri/src/approval_broker.rs` — defines `CommandApprovalRequest` and
  the public interface the Tauri `CommandApproval` trait implementation uses.
  The `command_label` and `reason` are plain strings surfaced in the native
  Tauri dialog (per AGENTS.md: "Approval is provided by the `CommandApproval`
  trait; the Tauri implementation uses `tauri_plugin_dialog`").

Conventions:
- Tests live inline `#[test]` in `skills.rs` (search `rg "#\[test\]" src-tauri/src/skills.rs`).
- The repo has 29 skill tests; new ones must follow the same shape
  (small fixture strings, assertions via `assert_eq!`).
- AGENTS.md "Safety model": "Hard-blocked commands always fail without
  approval" and "Mutating or destructive commands require one-time user
  approval via a native Tauri dialog." Skill script execution is classified
  mutating → goes through approval; the dialog is the user's only signal.

## Commands you will need

| Purpose   | Command                                                                  | Expected on success |
|-----------|--------------------------------------------------------------------------|---------------------|
| Backend tests | `cargo test --manifest-path src-tauri/Cargo.toml -- skills::`         | all pass           |
| Backend lint | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`     | exit 0             |

## Scope

**In scope**:
- `src-tauri/src/skills.rs` (resolve interpreter before constructing
  `CommandApprovalRequest`; include in label/reason; add tests)

**Out of scope**:
- Rejecting interpreters outright (denylist) — out of scope; this plan only
  surfaces the interpreter, it does not constrain it. A separate hardening
  pass could add a stricter skill-type-tag policy.
- The Tauri dialog UI itself (the `command_label`/`reason` strings flow
  through `approval_broker` → tauri_plugin_dialog unchanged).
- Detecting the interpreter from the script content vs. the file's actual
  shebang: keep using `detect_interpreter_from_content` — the existing
  inferer is the contract.

## Git workflow

- Branch: `advisor/017-skill-interpreter-in-approval`
- Commit example: `fix: include resolved skill-script interpreter in approval dialog label`.
- Do NOT push unless instructed.

## Steps

### Step 1: Reorder so interpreter resolution precedes approval request construction

Read `src-tauri/src/skills.rs` around lines 200–230 (the approval request
construction) and the function that calls it (search
`run_skill_script` near lines 780–830). Today the order is approximately:
1. Build `CommandApprovalRequest` with a fixed label/reason.
2. Request approval via the broker.
3. On approval: detect the interpreter, then spawn.

Swap so interpreter detection happens BEFORE the approval request is built;
the detection's output feeds the label/reason. Keep the failure path the
same (if no shebang is detected, the existing default interpreter is used —
do not change `detect_interpreter_from_content`'s return contract).

**Verify**: `cargo build --manifest-path src-tauri/Cargo.toml` → exit 0.

### Step 2: Include interpreter tokens in label + reason

In the `CommandApprovalRequest` construction, interpolate the interpreter
token list. Examples (use the live format conventions of the original
strings — single quotes inside are surrounding):

```rust
let interpreter = interpreter_parts.join(" ");
let command_label = format!(
    "Execute skill script '{script}' ({interpreter}) for skill '{skill}'",
);
let reason = format!(
    "Run script at {skill}/scripts/{script} via `{interpreter}`",
);
```

If `interpreter_parts` is empty (no shebang, default path used), fall back
to the existing strings — keep the label/reason unchanged in that branch so
the regression test for default-interpreter skills still matches.

Format the interpreter list as a shell-quoted string if any token contains
a space; use the existing repo quoting helper if one exists (search `rg "fn
quote|shell_quote" src-tauri/src`); if none exists, the simplest safe
rendering is to join with spaces and let the user read it as-is (the label
is informative, not a command to be re-execed). Do NOT construct runnable CLI
strings in the label — only a human-readable summary.

**Verify**: `cargo build --manifest-path src-tauri/Cargo.toml` → exit 0.

### Step 3: Add tests

Add `#[test]` functions next to the existing skills tests (search
`rg "#\[test\]" src-tauri/src/skills.rs` for placement):

1. `approval_request_includes_interpreter_for_bash_shebang` — feed a script
   whose shebang is `#!/usr/bin/env bash`; assert the constructed label
   contains `"bash"` (or the full `/usr/bin/env bash`).
2. `approval_request_includes_interpreter_for_python_with_args` — shebang
   `#!/usr/bin/python3 -u`; assert the label contains `"-u"`.
3. `approval_request_default_interpreter_when_no_shebang` — script with no
   shebang; assert the label has the original form unchanged (or contains
   the default interpreter name if there is one — match the existing
   default interpreter path).
4. `approval_request_reproduces_script_name` — confirm the script name is
   still in the label (regression for the existing assertion form).

To drive the approval request construction in a test without going through
the full Tauri dialog, extract the label/reason builder into a small pure
function (`fn build_script_approval_label(skill, script, interpreter_parts) ->
(String, String)`) at the top of the relevant module; this makes it unit-
testable without `CommandApprovalRequest` plumbing (which has Tauri
dependencies). Mirror the existing inline-test pattern; assert on the
returned strings.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml -- skills::`
→ all skills tests pass, including the 4 new ones.
`cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` → exit 0.

## Test plan

- 4 new `#[test]` functions in `src-tauri/src/skills.rs` (Step 3).
- Pattern: existing `#[test]` cohort in `skills.rs` (read any one test near
  `detect_interpreter_from_content` for the fixture style).
- The existing skills test suite (29 tests) must remain green — no behavior
  change except the label string, which the new tests pin.

## Done criteria

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml -- skills::` exits 0; 4 new tests pass
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0 (full backend suite)
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] The approval request construction label mentions the interpreter
  (search for the `format!` newly added in `skills.rs`).
- [ ] `rg "command_label: format!" src-tauri/src/skills.rs` shows the
  interpreter is interpolated into the label.
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row for plan 017 updated

## STOP conditions

Stop and report back if:

- `skills.rs:217-228` doesn't match the approval-request construction
  shape (drift since plan was written; re-read and reconsider).
- `skills.rs:816-830` doesn't build the command from
  `interpreter_parts[0]` … (executor should adapt the parser source to
  the live code; if the interpreter source is stored differently, report).
- Resolving the interpreter before approval changes an observable side
  effect (e.g. content is currently read lazily after approval; if so,
  confirm the read path is safe to take pre-approval — read
  `detect_interpreter_from_content` to make sure it's a pure read).
- The existing skills test fixtures assert the old-style label string
  verbatim — update these to the new label form; the executor must include
  the assertion updates in the same commit and call them out in the body.

## Maintenance notes

- A future plan may add interpreter denylisting (restrict to `bash`, `node`,
  `python3`, `sh`); the pure label-builder introduced in Step 3 is the
  seam for that future validation — extend it, don't duplicate.
- Reviewer: confirm the rendered label is human-readable and contains the
  interpreter (not just the script name); visually inspect on a real run
  using `bun run tauri dev` with a workspace that has a `.agents/skills/`
  entry containing a script.
- Follow-up deferred: a skill-type-tag clearinghouse that lets workspace
  skills declare allowed interpreters per-skill — out of scope; the surface
  here is purely informative.