# Plan 010: Bound Subprocess Output Capture

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 8c6e3ad..HEAD -- src-tauri/src/shell_tools.rs src-tauri/src/skills.rs src-tauri/src/review/mod.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: plans/006-close-whitespace-shell-path-bypass.md,
  plans/007-harden-skill-script-directory-sandbox.md
- **Category**: performance
- **Planned at**: commit `8c6e3ad`, 2026-07-17

## Why this matters

Three subprocess execution sites all use `tokio::process::Child::wait_with_output`,
which blocks until the child exits and only then returns the full stdout and
stderr as `Vec<u8>`. Until the child exits the runtime reads both pipes into
in-process kernel pipe buffers, and once those buffers fill the child blocks
on its next write. A noisy or runaway subprocess can:

- Hang the agent turn indefinitely if it produces output faster than
  the kernel pipe drains and never exits on its own;
- Consume unbounded memory when the child does exit and we read
  `Vec<u8>`s the runtime kept around.

This plan replaces the three `wait_with_output` call sites with a shared
helper that drains both pipes into bounded buffers concurrently, so noisy
output cannot deadlock a turn or grow memory without bound. It also
preserves the existing `truncate_bytes_to_string` behavior, exit-code
reporting, success/failure flag, and timeout semantics.

## Current state

The three call sites:

### `src-tauri/src/shell_tools.rs:1002-1043` (`CommandExecutor::execute`)

```rust
let mut command = tokio::process::Command::new(program);
command
    .args(args)
    .current_dir(&self.workspace_root)
    .kill_on_drop(true)
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());

let label = command_label(program, args);

let child = command.spawn().map_err(|e| {
    ShellToolError::Execution(format!("failed to spawn `{}`: {}", label, e))
})?;

let result = tokio::time::timeout(timeout, child.wait_with_output()).await;

let output = match result {
    Ok(Ok(output)) => output,
    Ok(Err(e)) => {
        return Err(ShellToolError::Execution(format!(
            "failed to run `{}`: {}",
            label, e
        )));
    }
    Err(_) => {
        return Ok(CommandOutput {
            success: false,
            exit_code: -1,
            stdout: String::new(),
            stderr: format!("Command `{}` timed out after {:?}", label, timeout),
            truncated: false,
            // ...
        });
    }
};

let (stdout, stdout_truncated) =
    truncate_bytes_to_string(&output.stdout, COMMAND_OUTPUT_CAP);
let (stderr, stderr_truncated) =
    truncate_bytes_to_string(&output.stderr, COMMAND_OUTPUT_CAP);
```

`COMMAND_OUTPUT_CAP = 32 * 1024` (line 14).

### `src-tauri/src/skills.rs:771-807` (`run_skill_script`)

```rust
let mut cmd = tokio::process::Command::new(interpreter_parts[0]);
for arg in &interpreter_parts[1..] {
    cmd.arg(arg);
}
cmd.kill_on_drop(true);
cmd.arg(&canonical_script);

if let Some(ws) = workspace_path {
    cmd.current_dir(ws);
}

cmd.stdout(std::process::Stdio::piped());
cmd.stderr(std::process::Stdio::piped());

let child = cmd
    .spawn()
    .map_err(|e| format!("Failed to spawn script '{}': {}", script_name, e))?;

let result = tokio::time::timeout(
    std::time::Duration::from_secs(timeout_secs),
    child.wait_with_output(),
)
.await;

let output = match result {
    Ok(Ok(output)) => output,
    Ok(Err(e)) => return Err(format!("Script execution failed: {}", e)),
    Err(_) => {
        return Err(format!(
            "Script '{}' timed out after {} seconds",
            script_name, timeout_secs
        ));
    }
};

let (stdout, stdout_truncated) = truncate_bytes_to_string(&output.stdout, SCRIPT_OUTPUT_CAP);
let (stderr, stderr_truncated) = truncate_bytes_to_string(&output.stderr, SCRIPT_OUTPUT_CAP);
```

`SCRIPT_OUTPUT_CAP = 16 * 1024` (line 670).

### `src-tauri/src/review/mod.rs:1951-1978` (`run_command_output`)

```rust
async fn run_command_output(
    workspace_path: &Path,
    program: &str,
    args: &[&str],
) -> Result<std::process::Output, CommandRunError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(workspace_path)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let command_label = command_label(program, args);

    let child = command.spawn().map_err(|error| CommandRunError::Io {
        program: program.to_string(),
        error,
    })?;

    match timeout(COMMAND_TIMEOUT, child.wait_with_output()).await {
        Ok(result) => result.map_err(|error| CommandRunError::Io {
            program: program.to_string(),
            error,
        }),
        Err(_) => Err(CommandRunError::Timeout {
            command: command_label,
        }),
    }
}
```

The pre-existing test `run_command_output_drains_large_stdout_while_waiting`
at line 2519-2535 of the same file (which uses `tokio::time::timeout` with a
2-second wall clock and `yes x | head -c 131072`) demonstrates that
`wait_with_output` happens to drain enough today — it does so because
Tokio's runtime reads both pipes into separate `Vec<u8>`s while waiting,
and at 128 KB the kernel pipe (typically 64 KB) plus the runtime
back-pressure leaves enough room for the head-truncated process to exit.
The new helper should keep that test green.

## Commands you will need

| Purpose         | Command                                              | Expected on success               |
|-----------------|------------------------------------------------------|-----------------------------------|
| Compile check   | `cargo check --manifest-path src-tauri/Cargo.toml`   | exit 0, no errors                 |
| Lint            | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | exit 0       |
| Run tests       | `cargo test --manifest-path src-tauri/Cargo.toml`    | exit 0, all pass (incl. new tests)|

## Scope

**In scope**:
- `src-tauri/src/shell_tools.rs` — add a shared subprocess-output helper
  (or call into the new one) and update `CommandExecutor::execute`.
- `src-tauri/src/skills.rs` — update `run_skill_script` to use the helper.
- `src-tauri/src/review/mod.rs` — update `run_command_output` to use the
  helper. Preserve its `std::process::Output`-shaped return value.

**Out of scope**:
- Changes to command classification or approval semantics.
- Changes to `CommandOutput`, `ScriptResult`, or `CommandRunError` field
  semantics beyond the contract described in Step 1.
- Changes to `kill_on_drop` policy, timeout values, or other call sites
  that already use the helper.
- Changes to `truncate_text_bytes` or the existing `truncate_bytes_to_string`
  helpers (both modules already have private versions of that helper;
  leave them alone — the new shared helper reuses the same logic).

## Git workflow

- Branch: `advisor/010-bound-subprocess-output-capture`
- Commit message style: `perf: cap subprocess output capture to avoid deadlock and unbounded memory`
- Land 006 and 007 first; this plan depends on them only to avoid merge
  friction in the same files. The dependency does not affect runtime
  behavior — 010 will work whether or not 006/007 have shipped.

## Steps

### Step 1: Add a shared subprocess-output helper

Create a new module file `src-tauri/src/subprocess_output.rs` with a single
public function `run_with_bounded_output` that takes a `tokio::process::Command`,
a `Duration` timeout, and per-stream output caps, and returns a small
result struct. The helper:

- Spawns the child.
- Captures the stdout and stderr `ChildStdout` / `ChildStderr` handles.
- Spawns two concurrent reader tasks that each loop on
  `AsyncReadExt::read(&mut buf)` and append up to `max_bytes` into a
  `Vec<u8>`. When the cap is reached, the reader continues to drain
  the pipe into a discard buffer (so the child does not block on
  write) but no longer grows the kept `Vec<u8>`. Mark the
  corresponding `truncated` flag when the cap was hit.
- `await`s the child, applying the timeout.
- Returns the captured bytes, the truncated flags, the exit code, and
  the timeout indicator.

Sketch:

```rust
use std::io;
use std::process::ExitStatus;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

pub const DEFAULT_READ_CHUNK: usize = 8 * 1024;
const DISCARD_CHUNK: usize = 8 * 1024;

#[derive(Debug, Error)]
pub enum SubprocessError {
    #[error("failed to spawn `{label}`: {source}")]
    Spawn { label: String, source: io::Error },
    #[error("failed waiting on `{label}`: {source}")]
    Wait { label: String, source: io::Error },
}

#[derive(Debug)]
pub struct BoundedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub timed_out: bool,
}

pub async fn run_with_bounded_output(
    label: &str,
    mut command: Command,
    timeout: Duration,
    stdout_cap: usize,
    stderr_cap: usize,
) -> Result<BoundedOutput, SubprocessError> {
    command
        .stdio()
        .stdin(std::process::Stdio::null());

    let mut child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|source| SubprocessError::Spawn {
            label: label.to_string(),
            source,
        })?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdout_task = tokio::spawn(read_bounded(stdout, stdout_cap));
    let stderr_task = tokio::spawn(read_bounded(stderr, stderr_cap));

    let wait_result = tokio::time::timeout(timeout, child.wait()).await;
    let timed_out = wait_result.is_err();

    if timed_out {
        // On Unix, create the command in its own process group and signal the
        // group so descendants cannot retain inherited stdout/stderr pipes.
        terminate_child_group(&mut child).await?;
        child.wait().await?;
    }

    // Descendants are gone after group termination, but keep a bounded drain
    // as a final guard before aborting the reader tasks.
    let (stdout_res, stderr_res) = if timed_out {
        tokio::time::timeout(POST_KILL_DRAIN_TIMEOUT, async {
            tokio::join!(stdout_task, stderr_task)
        })
        .await
        .map_err(|_| SubprocessError::Wait {
            label: label.to_string(),
            source: io::Error::new(io::ErrorKind::TimedOut, "subprocess pipes remained open"),
        })?
    } else {
        tokio::join!(stdout_task, stderr_task)
    };
    let (stdout_bytes, stdout_truncated) = stdout_res
        .map_err(|e| SubprocessError::Wait { label: label.to_string(), source: io::Error::new(io::ErrorKind::Other, e) })?
        .unwrap_or_default();
    let (stderr_bytes, stderr_truncated) = stderr_res
        .map_err(|e| SubprocessError::Wait { label: label.to_string(), source: io::Error::new(io::ErrorKind::Other, e) })?
        .unwrap_or_default();

    let status = match wait_result {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            return Err(SubprocessError::Wait {
                label: label.to_string(),
                source: e,
            });
        }
        Err(_) => {
            // Synthesize a failed exit status so callers can produce a
            // structured timeout error.
            return Ok(BoundedOutput {
                status: fake_failure_status(),
                stdout: stdout_bytes,
                stderr: stderr_bytes,
                stdout_truncated,
                stderr_truncated,
                timed_out: true,
            });
        }
    };

    Ok(BoundedOutput {
        status,
        stdout: stdout_bytes,
        stderr: stderr_bytes,
        stdout_truncated,
        stderr_truncated,
        timed_out: false,
    })
}

async fn read_bounded<R>(
    pipe: Option<R>,
    cap: usize,
) -> Option<(Vec<u8>, bool)>
where
    R: AsyncReadExt + Unpin + Send + 'static,
{
    let mut pipe = pipe?;
    let mut kept = Vec::with_capacity(cap.min(DEFAULT_READ_CHUNK));
    let mut truncated = false;
    let mut buf = [0u8; DEFAULT_READ_CHUNK];
    loop {
        match pipe.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                if kept.len() < cap {
                    let remaining = cap - kept.len();
                    let take = remaining.min(n);
                    kept.extend_from_slice(&buf[..take]);
                    if take < n {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Err(_) => break,
        }
    }
    Some((kept, truncated))
}

#[cfg(unix)]
fn fake_failure_status() -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(0xff)
}

#[cfg(not(unix))]
fn fake_failure_status() -> ExitStatus {
    // On Windows we cannot synthesize a raw status cheaply; the timeout
    // path will still surface a structured error to the caller via
    // SubprocessError::Wait (see alternative design below).
    todo!("synthesize exit status on non-unix")
}
```

Register the module in `src-tauri/src/lib.rs` near the other
top-level modules (search for `pub mod` in `lib.rs` to find the right
spot). If the timeout path needs a real `ExitStatus` on Windows, branch
the function to return `Result<BoundedOutput, SubprocessError>` and let
the caller treat the timeout as `Err` — that is closer to the existing
`CommandRunError::Timeout` and `CommandOutput { success: false, reason: Some("timeout"), .. }`
shapes. **Recommended**: branch on `cfg!(unix)` at the *call* sites
(see Step 2), not in the helper, to keep the helper portable and to
mirror the existing `truncate_bytes_to_string` pattern in
`shell_tools.rs` (which uses `String::from_utf8_lossy` and so has no
platform-specificity).

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` exits 0.

### Step 2: Wire the helper into `CommandExecutor::execute`

In `src-tauri/src/shell_tools.rs`, replace the body of
`CommandExecutor::execute` (lines 989-1080) with code that:

1. Calls `subprocess_output::run_with_bounded_output` with
   `label = command_label(program, args)`, the constructed `Command`,
   the timeout, and `stdout_cap = stderr_cap = COMMAND_OUTPUT_CAP`.
2. Branches on `BoundedOutput.timed_out` to return the same
   `CommandOutput { success: false, exit_code: -1, ..., reason: Some("timeout"), .. }`
   shape as today (the `Err(_)` branch at line 1026-1037).
3. Builds the `CommandOutput` from `BoundedOutput.{stdout, stderr,
   stdout_truncated, stderr_truncated, status}`, reusing the existing
   `truncate_bytes_to_string` (still defined locally at line 1123) to
   apply the `"\n\n[truncated]"` suffix. The new helper's
   `stdout_truncated` is `true` when the cap was hit; passing that
   byte slice (already at most `COMMAND_OUTPUT_CAP` long) through
   `truncate_bytes_to_string(_, COMMAND_OUTPUT_CAP)` returns
   `truncated = true` because `truncate_text_bytes` reserves space
   for the suffix. That matches the existing visual: any output
   that hit the cap is marked truncated.

Sketch (the `execute` function body only — leave the function signature
unchanged):

```rust
let mut command = tokio::process::Command::new(program);
command
    .args(args)
    .current_dir(&self.workspace_root)
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());

let label = command_label(program, args);

let output = match crate::subprocess_output::run_with_bounded_output(
    &label,
    command,
    timeout,
    COMMAND_OUTPUT_CAP,
    COMMAND_OUTPUT_CAP,
)
.await
{
    Ok(o) => o,
    Err(crate::subprocess_output::SubprocessError::Spawn { source, .. }) => {
        return Err(ShellToolError::Execution(format!(
            "failed to spawn `{}`: {}",
            label, source
        )));
    }
    Err(crate::subprocess_output::SubprocessError::Wait { source, .. }) => {
        return Err(ShellToolError::Execution(format!(
            "failed to run `{}`: {}",
            label, source
        )));
    }
};

if output.timed_out {
    return Ok(CommandOutput {
        success: false,
        exit_code: -1,
        stdout: String::new(),
        stderr: format!("Command `{}` timed out after {:?}", label, timeout),
        truncated: false,
        needs_approval: None,
        reason: Some("timeout".to_string()),
        message: format!("Command `{}` timed out after {:?}", label, timeout),
    });
}

let (stdout, stdout_truncated) =
    truncate_bytes_to_string(&output.stdout, COMMAND_OUTPUT_CAP);
let (stderr, stderr_truncated) =
    truncate_bytes_to_string(&output.stderr, COMMAND_OUTPUT_CAP);
let truncated = stdout_truncated
    || stderr_truncated
    || output.stdout_truncated
    || output.stderr_truncated;
let exit_code = output.status.code().unwrap_or(-1);
let success = output.status.success();
// ... rest of the function unchanged: build message, return CommandOutput ...
```

Remove the now-unused `tokio::time::timeout` import if this was its only
use in the file (search the file before removing). Keep
`std::time::Duration` because it is used elsewhere.

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` exits 0.

### Step 3: Wire the helper into `run_skill_script`

In `src-tauri/src/skills.rs`, replace the spawn / wait block in
`run_skill_script` (lines 785-815) with a call to the same helper:

```rust
let output = match crate::subprocess_output::run_with_bounded_output(
    &script_name,
    cmd,
    std::time::Duration::from_secs(timeout_secs),
    SCRIPT_OUTPUT_CAP,
    SCRIPT_OUTPUT_CAP,
)
.await
{
    Ok(o) => o,
    Err(crate::subprocess_output::SubprocessError::Spawn { source, .. }) => {
        return Err(format!("Failed to spawn script '{}': {}", script_name, source));
    }
    Err(crate::subprocess_output::SubprocessError::Wait { source, .. }) => {
        return Err(format!("Script execution failed: {}", source));
    }
};

if output.timed_out {
    return Err(format!(
        "Script '{}' timed out after {} seconds",
        script_name, timeout_secs
    ));
}

let (stdout, stdout_truncated) = truncate_bytes_to_string(&output.stdout, SCRIPT_OUTPUT_CAP);
let (stderr, stderr_truncated) = truncate_bytes_to_string(&output.stderr, SCRIPT_OUTPUT_CAP);

Ok(ScriptResult {
    stdout,
    stderr,
    exit_code: output.status.code().unwrap_or(-1),
    truncated: stdout_truncated
        || stderr_truncated
        || output.stdout_truncated
        || output.stderr_truncated,
})
```

The local `truncate_bytes_to_string` helper at line 817 stays — it is the
project-wide convention for applying the `"\n\n[truncated]"` suffix and
matches the helper in `shell_tools.rs`.

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` exits 0.

### Step 4: Wire the helper into `run_command_output`

In `src-tauri/src/review/mod.rs`, change the return type of
`run_command_output` to a new local struct that mirrors `BoundedOutput` (or
expose `BoundedOutput` directly). The function's three callers (lines 1880,
1897, 1933) read `output.status`, `output.stdout`, and `output.stderr`. The
simplest move: keep the function returning something with the same
`status` / `stdout` / `stderr` shape, but capture them from
`BoundedOutput`. To minimize blast radius, define a small re-export:

```rust
struct CapturedOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
    timed_out: bool,
}

const REVIEW_COMMAND_OUTPUT_CAP: usize = 4 * 1024 * 1024;

async fn run_command_output(
    workspace_path: &Path,
    program: &str,
    args: &[&str],
) -> Result<CapturedOutput, CommandRunError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(workspace_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let command_label = command_label(program, args);

    let result = crate::subprocess_output::run_with_bounded_output(
        &command_label,
        command,
        COMMAND_TIMEOUT,
        REVIEW_COMMAND_OUTPUT_CAP,
        REVIEW_COMMAND_OUTPUT_CAP,
    )
    .await;

    match result {
        Ok(o) => Ok(CapturedOutput {
            status: o.status,
            stdout: o.stdout,
            stderr: o.stderr,
            stdout_truncated: o.stdout_truncated,
            stderr_truncated: o.stderr_truncated,
            timed_out: o.timed_out,
        }),
        Err(crate::subprocess_output::SubprocessError::Spawn { source, .. }) => {
            Err(CommandRunError::Io { program: program.to_string(), error: source })
        }
        Err(crate::subprocess_output::SubprocessError::Wait { source, .. }) => {
            Err(CommandRunError::Io { program: program.to_string(), error: source })
        }
    }
}
```

Then update the three callers (lines 1880, 1897, 1933) to use
`CapturedOutput` instead of `std::process::Output`. The only fields they
read are `status`, `stdout`, and `stderr`; the rename is mechanical.

For the timeout case: `run_command_output`'s old behavior returned
`CommandRunError::Timeout`. Preserve that by branching on
`captured.timed_out` in the callers, or by adding a small wrapper in
`run_command_output` that returns the existing `CommandRunError::Timeout`
when `timed_out` is true. **Recommended**: add a thin wrapper inside
`run_command_output` itself, returning `CommandRunError::Timeout` if
`timed_out` is set, otherwise the `CapturedOutput`:

```rust
match result {
    Ok(o) if o.timed_out => Err(CommandRunError::Timeout { command: command_label }),
    Ok(o) => Ok(CapturedOutput { ... }),
    Err(crate::subprocess_output::SubprocessError::Spawn { source, .. }) => {
        Err(CommandRunError::Io { program: program.to_string(), error: source })
    }
    Err(crate::subprocess_output::SubprocessError::Wait { source, .. }) => {
        Err(CommandRunError::Io { program: program.to_string(), error: source })
    }
}
```

This keeps the callers unchanged for the timeout path.

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` exits 0.

### Step 5: Add tests for bounded output

In the appropriate `tests` module of each modified file, add unix-guarded
tests modeled on the existing `run_command_output_drains_large_stdout_while_waiting`
at `src-tauri/src/review/mod.rs:2519-2535`. New tests:

#### `subprocess_output` tests (in the new module's own `#[cfg(test)] mod tests`)

```rust
#[cfg(unix)]
#[tokio::test]
async fn bounded_helper_caps_stdout_and_does_not_deadlock() {
    use std::time::Duration;
    let mut command = tokio::process::Command::new("sh");
    command.arg("-c").arg("yes x | head -c 131072");

    let out = run_with_bounded_output(
        "yes-head",
        command,
        Duration::from_secs(2),
        32 * 1024,
        32 * 1024,
    )
    .await
    .expect("run succeeds");

    assert!(out.stdout_truncated, "stdout should be marked truncated");
    assert!(out.stdout.len() <= 32 * 1024, "kept bytes within cap");
    assert!(out.status.success());
}

#[cfg(unix)]
#[tokio::test]
async fn bounded_helper_drains_simultaneous_stdout_and_stderr() {
    use std::time::Duration;
    let mut command = tokio::process::Command::new("sh");
    command.arg("-c").arg("yes x | head -c 65536 & yes y 1>&2 | head -c 65536; wait");

    let out = run_with_bounded_output(
        "both",
        command,
        Duration::from_secs(2),
        16 * 1024,
        16 * 1024,
    )
    .await
    .expect("run succeeds");

    assert!(out.stdout_truncated);
    assert!(out.stderr_truncated);
    assert!(out.stdout.len() <= 16 * 1024);
    assert!(out.stderr.len() <= 16 * 1024);
}

#[cfg(unix)]
#[tokio::test]
async fn bounded_helper_reports_timeout() {
    use std::time::Duration;
    let mut command = tokio::process::Command::new("sh");
    command.arg("-c").arg("sleep 5");

    let out = run_with_bounded_output(
        "sleep",
        command,
        Duration::from_millis(200),
        1024,
        1024,
    )
    .await
    .expect("run resolves with timed_out=true");

    assert!(out.timed_out);
}
```

#### `shell_tools.rs` test

Add `bounded_capture_truncates_large_stdout_without_deadlock` modeled on
the existing test at `src-tauri/src/review/mod.rs:2519`. Use
`CommandExecutor::execute` directly (or `run_shell` on a `CommandExecutor`
configured with a tempdir workspace) to invoke `sh -c "yes x | head -c
131072"`. Verify `output.truncated` is `true` and `output.stdout.len() <=
COMMAND_OUTPUT_CAP + 100` (matching the existing skills test's tolerance
at line 1380).

#### `skills.rs` test

`run_skill_script_caps_output` covering the same shape for a script that
produces 64 KB of output and a `SCRIPT_OUTPUT_CAP` of 16 KB. Reuse the
`hello` script pattern from `run_skill_script_executes_bash_script` at
line 1384.

#### `review/mod.rs` test

`run_command_output_drains_large_stdout_while_waiting` at line 2519 must
continue to pass unchanged. The new helper's `usize::MAX` cap in Step 4
preserves the existing 128 KB output. **No new test required for review**,
but rerun the existing one as part of the verification.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml -- subprocess_output shell_tools:: tests::run_skill_script review::` exits 0 and the new tests pass.

### Step 6: Run lint and full test suite

- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — exit 0.
- `cargo test --manifest-path src-tauri/Cargo.toml` — exit 0, all tests
  pass (the prior baseline reported 334 Rust unit tests plus 1 keychain
  integration test; the new tests add a handful more).

**Verify**: Both commands exit 0.

## Test plan

- New tests:
  - `subprocess_output::tests::bounded_helper_caps_stdout_and_does_not_deadlock` — `#[cfg(unix)]`
  - `subprocess_output::tests::bounded_helper_drains_simultaneous_stdout_and_stderr` — `#[cfg(unix)]`
  - `subprocess_output::tests::bounded_helper_reports_timeout` — `#[cfg(unix)]`
  - `shell_tools::tests::bounded_capture_truncates_large_stdout_without_deadlock` — `#[cfg(unix)]`
  - `skills::tests::run_skill_script_caps_output` — `#[cfg(unix)]`
- Pre-existing tests that must continue to pass:
  `run_command_output_drains_large_stdout_while_waiting`,
  `run_skill_script_executes_bash_script`, the full `shell_tools::tests`
  module, and the rest of the `skills` and `review` test modules.
- Verification: `cargo test --manifest-path src-tauri/Cargo.toml` exits 0
  with 5 more tests than before.

## Done criteria

- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` exits 0
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0; the 5 new
      tests exist and pass; `run_command_output_drains_large_stdout_while_waiting`
      still passes
- [ ] No files outside the in-scope list are modified (the new
      `subprocess_output.rs` is in scope; do not create additional files)
- [ ] `plans/README.md` status row for plan 010 updated to `DONE`

## STOP conditions

- The drift check shows non-empty output for any in-scope file. Re-open,
  reconcile, treat as a planning defect.
- A pre-existing test starts failing because the helper's `BoundedOutput`
  shape differs from what a caller expected. STOP and reconcile the
  field names; do not change the existing call sites' contracts.
- The pre-existing `run_command_output_drains_large_stdout_while_waiting`
  test fails. STOP — that test is the regression marker for the
  large-output no-deadlock case. Re-read Step 4; the most likely cause
  is that the helper closed the pipes before the child had a chance to
  write all 128 KB.
- A reviewer wants the new helper to also handle stdin. STOP — this plan
  intentionally does not, and a future plan can add a `Stdio::piped()`
  stdin path with backpressure.
- The `truncate_bytes_to_string` helper in either `shell_tools.rs` or
  `skills.rs` is removed by the change. STOP and re-add it; the helper
  is still used to apply the visual `"\n\n[truncated]"` suffix.

## Maintenance notes

- The shared helper sets a hard `stdin` to `Stdio::null()` so a child
  cannot read from the agent's stdin. This is a defensive default; if a
  future plan needs interactive commands, the helper should accept an
  optional `Option<Stdio>` parameter.
- The new helper is intentionally minimal — no streaming output to the
  agent, no event hooks, no progress. A future plan that streams
  intermediate output to the UI should layer on top, not replace, this
  helper.
- A reviewer should check that the `kill_on_drop(true)` policy is
  preserved. The helper sets it; the old call sites also set it; the
  union is the same single `true` on the spawned `Command`. Behavior is
  preserved.
- The `usize::MAX` cap in `review/mod.rs` keeps the existing review
  behavior of capturing the full output. If the review pipeline ever
  needs bounded output, lower that to e.g. `256 * 1024` in a follow-up
  plan rather than changing this plan.
