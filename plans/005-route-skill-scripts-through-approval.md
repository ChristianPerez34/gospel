# Plan 005: Route Skill Script Execution Through Approval Broker

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 2e5bd36..HEAD -- src-tauri/src/skills.rs src-tauri/src/session_turn.rs src-tauri/src/lib.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: plans/004-close-shell-flag-path-escape.md
- **Category**: security
- **Planned at**: commit `2e5bd36`, 2026-07-11

## Why this matters

The codebase allows agents to execute skill-bundled scripts on the host system using the `RunSkillScriptTool` tool. Currently, these scripts are executed directly without any user authorization or prompts. Because executing arbitrary scripts carries a high risk of malicious code execution, this execution must be governed by the user safety model.
This plan integrates `RunSkillScriptTool` with the `ApprovalBroker` to prompt the user before any script is executed, conforming to the same approval gates used for mutating CLI shell commands.

## Current state

- In `src-tauri/src/skills.rs` line 80:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSkillScriptTool {
    pub available_skills: Vec<Skill>,
    pub workspace_path: Option<PathBuf>,
}
```
- In `src-tauri/src/skills.rs` line 188 (inside `RunSkillScriptTool::call`):
```rust
        match run_skill_script(skill, &args.script, self.workspace_path.as_deref()).await {
```
- In `src-tauri/src/session_turn.rs` line 437:
```rust
        Some(RunSkillScriptTool {
            available_skills: scriptable,
            workspace_path,
        })
```
- In `src-tauri/src/lib.rs` line 1123, the `command_approval` broker is constructed but never injected into `RunSkillScriptTool`.

## Commands you will need

| Purpose     | Command                                            | Expected on success |
|-------------|----------------------------------------------------|---------------------|
| Run tests   | `cargo test --manifest-path src-tauri/Cargo.toml`  | exit 0, all pass    |

## Scope

**In scope**:
- `src-tauri/src/skills.rs` - add approval field, request approval in `call`, update tests.
- `src-tauri/src/session_turn.rs` - update `RunSkillScriptTool` instantiation.
- `src-tauri/src/lib.rs` - inject `command_approval` broker into `RunSkillScriptTool` during completion.

**Out of scope**:
- Changing approval UI elements or Tauri plugin dialogs.
- Modifying standard shell command approval policy.

## Git workflow

- Branch: `advisor/005-route-skill-scripts-through-approval`
- Commit message style: `security: route skill script execution through approval broker`

## Steps

### Step 1: Update RunSkillScriptTool struct definition
In `src-tauri/src/skills.rs`, import `CommandApproval`, `CommandApprovalRequest`, and `CommandRisk` from `crate::shell_tools`.
Update the `RunSkillScriptTool` struct to hold an optional `Arc<dyn CommandApproval>`. Mark it with `#[serde(skip)]` so that it doesn't interfere with serialization/deserialization.

```rust
use crate::shell_tools::{CommandApproval, CommandApprovalRequest, CommandRisk};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSkillScriptTool {
    pub available_skills: Vec<Skill>,
    pub workspace_path: Option<PathBuf>,
    #[serde(skip)]
    pub command_approval: Option<Arc<dyn CommandApproval>>,
}
```

**Verify**: Run `cargo check --manifest-path src-tauri/Cargo.toml` to check imports.

### Step 2: Request user approval in RunSkillScriptTool::call
Modify `RunSkillScriptTool::call` in `src-tauri/src/skills.rs` to request approval prior to running the script. If the user denies approval, return a successful output containing a permission error.

```rust
        if let Some(approval) = &self.command_approval {
            let script_path = format!("{}/scripts/{}", args.skill, args.script);
            let approved = approval
                .request_approval(CommandApprovalRequest {
                    tool_name: "run_skill_script",
                    command_label: format!(
                        "Execute skill script '{}' for skill '{}'",
                        args.script, args.skill
                    ),
                    reason: format!("Run script at {}", script_path),
                    risk: CommandRisk::Mutating,
                })
                .await;

            if !approved {
                return Ok(RunSkillScriptOutput {
                    success: false,
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: -1,
                    truncated: false,
                    error: Some("Execution denied by user".to_string()),
                });
            }
        }
```

**Verify**: Compiles successfully.

### Step 3: Update RunSkillScriptTool instantiations
1. In `src-tauri/src/session_turn.rs` line 437, add `command_approval: None`:
```rust
        Some(RunSkillScriptTool {
            available_skills: scriptable,
            workspace_path,
            command_approval: None,
        })
```
2. In `src-tauri/src/skills.rs` tests (lines 1405 and 1424), add `command_approval: None` to the test instantiations.

**Verify**: Run `cargo test --manifest-path src-tauri/Cargo.toml` to ensure all existing tests compile and pass.

### Step 4: Inject approval broker in stream_completion
In `src-tauri/src/lib.rs` line 1142, extract `request.skill_script_tool`, inject the cloned `command_approval` broker into it, and pass it to `llm::stream_completion`.

```rust
            let mut skill_script_tool = request.skill_script_tool;
            if let Some(ref mut tool) = skill_script_tool {
                tool.command_approval = Some(command_approval.clone());
            }

            llm::stream_completion(
                ...
                request.chat_history,
                request.matched_skills_section,
                request.invoked_skill_section,
                skill_script_tool,
                ...
```

**Verify**: Check that compiling passes.

### Step 5: Add tests for skill script approval
Add unit tests in `src-tauri/src/skills.rs` verifying:
1. When `command_approval` is configured and returns `true`, script execution completes successfully.
2. When `command_approval` is configured and returns `false`, script execution is blocked and the returned output indicates it was denied.

**Verify**: Run `cargo test --manifest-path src-tauri/Cargo.toml` and confirm all 307+ tests pass.

## Done criteria

- [x] `cargo test` runs and passes all tests.
- [x] Script execution requests are successfully routed through `command_approval` when populated.
- [x] No files outside the in-scope list are modified, except the required plan status files.

## STOP conditions

- If compiling fails due to serialization mismatches on the tool.
- If modifying `lib.rs` causes lifetime issues with the `on_event` closure or the `command_approval` reference.
