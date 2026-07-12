# Plan 003: Fix Loop Detector False Positive on Successful No-Op Results

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 2e5bd36..HEAD -- src-tauri/src/llm.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: plans/002-add-lint-format-typecheck.md
- **Category**: bug
- **Planned at**: commit `2e5bd36`, 2026-07-11

## Why this matters

The loop detector tracks consecutive tool calls and repeated deterministic failures to prevent an agent from getting stuck. Currently, when parsing tool result summaries, it extracts the `"reason"` key and records a failure if the reason matches one of the `DETERMINISTIC_FAILURE_REASONS` (e.g., `"no_match"`).
However, it does this without verifying whether the tool execution actually *failed*. A successful execution (e.g. `grep_search` finding no matches, returning `"success": true, "reason": "no_match"`) is falsely recorded as a deterministic failure, accumulating towards the failure streak and causing the agent to abort prematurely on completely successful operations.
This plan fixes the check to only record a failure if the tool output explicitly signals a failure (`"success": false`).

## Current state

- In `src-tauri/src/llm.rs` lines 1043-1074:
```rust
                            // Check for repeated deterministic failures
                            match serde_json::from_str::<serde_json::Value>(&result_summary) {
                                Ok(parsed) => {
                                    if let Some(reason) = parsed.get("reason").and_then(|r| r.as_str()) {
                                        if let Some(status) = loop_detector.record_failure(reason) {
                                            match status {
                                                LoopStatus::Ok => {}
                                                LoopStatus::Warning(count) => {
                                                    on_event(StreamEvent::LoopWarning {
                                                        count,
                                                        tool_name: tool_name.clone(),
                                                    });
                                                }
                                                LoopStatus::Stop => {
                                                    let msg = format!(
                                                        "Agent stopped: repeated deterministic failure '{}' detected {} times. The agent appears stuck trying the same failing approach.",
                                                        reason, loop_detector.failure_streak
                                                    );
                                                    on_event(StreamEvent::LoopStopped {
                                                        count: loop_detector.failure_streak,
                                                        tool_name: tool_name.clone(),
                                                        message: msg.clone(),
                                                    });
                                                    return Err(LlmError::ControlledStop(msg));
                                                }
                                            }
                                        }
                                    } else {
                                        loop_detector.reset_failure_streak();
                                    }
                                }
                                Err(_) => loop_detector.reset_failure_streak(),
                            }
```

## Commands you will need

| Purpose     | Command                                            | Expected on success |
|-------------|----------------------------------------------------|---------------------|
| Run tests   | `cargo test --manifest-path src-tauri/Cargo.toml`  | exit 0, all pass    |

## Scope

**In scope**:
- `src-tauri/src/llm.rs` - update loop detector evaluation logic and add unit tests.

**Out of scope**:
- Changing any other files.
- Changing `LoopDetector`'s internal hashing and hash matching.

## Git workflow

- Branch: `advisor/003-fix-loop-detector-false-positive`
- Commit message style: `fix: check success flag before recording deterministic failures in loop detector`

## Steps

### Step 1: Update the failure check in stream_completion
Modify the JSON check in `src-tauri/src/llm.rs` to inspect the `"success"` boolean field first. If it is `true` or missing, we must reset the failure streak and bypass `record_failure`.

```rust
                            // Check for repeated deterministic failures
                            match serde_json::from_str::<serde_json::Value>(&result_summary) {
                                Ok(parsed) => {
                                    let success = parsed.get("success").and_then(|s| s.as_bool()).unwrap_or(true);
                                    if !success {
                                        if let Some(reason) = parsed.get("reason").and_then(|r| r.as_str()) {
                                            if let Some(status) = loop_detector.record_failure(reason) {
                                                match status {
                                                    LoopStatus::Ok => {}
                                                    LoopStatus::Warning(count) => {
                                                        on_event(StreamEvent::LoopWarning {
                                                            count,
                                                            tool_name: tool_name.clone(),
                                                        });
                                                    }
                                                    LoopStatus::Stop => {
                                                        let msg = format!(
                                                            "Agent stopped: repeated deterministic failure '{}' detected {} times. The agent appears stuck trying the same failing approach.",
                                                            reason, loop_detector.failure_streak
                                                        );
                                                        on_event(StreamEvent::LoopStopped {
                                                            count: loop_detector.failure_streak,
                                                            tool_name: tool_name.clone(),
                                                            message: msg.clone(),
                                                        });
                                                        return Err(LlmError::ControlledStop(msg));
                                                    }
                                                }
                                            }
                                        }
                                    } else {
                                        loop_detector.reset_failure_streak();
                                    }
                                }
                                Err(_) => loop_detector.reset_failure_streak(),
                            }
```

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` compiles successfully.

### Step 2: Add unit tests for LoopDetector
Add a unit test suite to `src-tauri/src/llm.rs` tests module verifying:
1. `record_failure` only tracks failure reasons when success is false.
2. A sequence of results with `success: true` and `reason: "no_match"` resets/does not increment the failure streak.
3. A sequence of results with `success: false` and `reason: "blocked"` correctly warning-triggers and stop-triggers.

**Verify**: Run `cargo test --manifest-path src-tauri/Cargo.toml` and confirm all tests (including the new ones) pass.

## Done criteria

- [x] `cargo test` runs and passes all Rust tests.
- [x] New unit tests verifying LoopDetector success/failure behavior exist and pass.
- [x] No files outside the in-scope list are modified, except the required plan status files.

## STOP conditions

- If code changes introduce compilation errors that cannot be easily resolved.
- If modifying `llm.rs` breaks other dependencies or structs.
