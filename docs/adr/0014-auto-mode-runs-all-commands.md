# ADR 0014: Auto Mode Runs All Commands

## Status

Accepted.

## Context

Command tools originally had three outcomes: read-only commands ran, mutating and destructive commands asked for approval, and a fixed list of dangerous commands (shell interpreters, shell metacharacters, recursive root deletion, force pushes, hard resets, `git clean`, `gh repo delete`, and others) was hard-blocked. Auto mode only skipped prompts for an allowlist of routine workspace-scoped development commands.

That made Auto mode unpredictable. Agents could not run ordinary workflows that needed a shell pipeline or a remote operation, and the developer could not opt into handing the agent full command control.

## Decision

1. **Auto mode runs every command.** Shell, git, GitHub CLI, and skill-script requests run without approval regardless of risk, workspace scope, or the built-in reject patterns. Selecting Auto is the developer's consent.
2. **Former hard blocks become Restricted Commands.** Ask mode still blocks them without a prompt, so one click can never run them. Only Auto mode runs them.
3. **User Deny Rules still prompt in Auto.** A rule the developer saved with "Always deny" is an explicit instruction and outranks the mode.
4. **The mode stays global.** Switching to Auto approves every pending command request in every session except those matching a User Deny Rule. Switching to Ask denies pending Restricted Commands.
5. **Other approvals are unchanged.** External-path requests always prompt; MCP tools need explicit server/tool allow rules. Empty command invocations remain blocked.

## Consequences

- In Auto mode, the agent can delete data, rewrite remote history, and read secrets through command tools. The TopBar tooltip states this.
- Ask mode keeps the original hard-block safety contract.
- The command executor needs the current mode before it asks for approval; `CommandApproval::permits_restricted` provides it, and the broker auto-denies Restricted Commands outside Auto as a backstop against mode-switch races.
