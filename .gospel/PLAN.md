# Plan

## Goal

Implement Phase 1 of the Gospel shell/git/GitHub CLI agent tools: a safe command executor, a classification engine, and three rig tools (`run_shell_command`, `run_git_command`, `run_github_cli_command`) gated by one-time user approval for mutating/destructive commands.

## Steps

- [x] Read the handoff and explore the codebase for integration points.
- [x] Create/update `.gospel/PLAN.md` with the shell tools Phase 1 plan.
- [x] Implement `src-tauri/src/shell_tools.rs` with `CommandSafety`, `CommandPolicy`, `CommandExecutor`, `CommandApproval`, and the three tool structs.
- [x] Add unit tests for the command classifier and safety checks.
- [x] Wire the tools into `llm.rs` system preamble and `stream_completion` tool builder.
- [x] Wire `command_approval` through the `lib.rs` Tauri adapter (mirrors the existing `ExternalPathApproval` pattern; `session_turn.rs` does not need changes because approval is provided by the Tauri-side `SessionTurnLlm` adapter).
- [x] Implement `TauriCommandApproval` using `tauri_plugin_dialog`.
- [x] Update `AGENTS.md` to document the new tools and policy.
- [x] Verify with `cargo test --manifest-path src-tauri/Cargo.toml` and `bun run build`.

## Evidence / Verification

- `cargo test --manifest-path src-tauri/Cargo.toml shell_tools` — 21 tests passed.
- `cargo test --manifest-path src-tauri/Cargo.toml llm::tests` — 14 tests passed.
- `cargo test --manifest-path src-tauri/Cargo.toml session_turn::tests` — 21 tests passed.
- `cargo test --manifest-path src-tauri/Cargo.toml` — 273 library tests + 1 keychain integration test + 1 ignored doctest passed.
- `cargo check --manifest-path src-tauri/Cargo.toml` — clean, no warnings.
- `bun run build` — Vite frontend build succeeded.

## Open Questions / Risks

- Tauri dialog is async via callback; approval timeout set to 60 seconds.
- Classification engine is conservative: read-only allowlists are small, mutating/destructive commands require explicit approval.
- Workspace path escaping detection relies on canonicalization of absolute paths and treats any `..` in arguments as requiring approval; legitimate external paths still require approval rather than being hard-blocked.
- `session_turn.rs` was not modified because the `CommandApproval` adapter is supplied by the Tauri-side `SessionTurnLlm` implementation, matching the existing `ExternalPathApproval` pattern.

## Next Action

Ready for review. Future phases can add `.gospel/shell-policy.json` overrides, CI-wait polling, and additional command allowlists/denylists.
