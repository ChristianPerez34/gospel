## Package manager

This project uses [Bun](https://bun.sh/) as its package manager (`bun.lock` is present at the repo root). Prefer `bun` over `npm` for installing dependencies, running scripts, and executing project commands.

## Running tests

The frontend test suite uses [Vitest](https://vitest.dev). Always run it with:

```bash
bun run test
```

This executes `vitest run` (defined in `package.json`'s `test` script), which honors the Vitest-specific setup in `src/test/setup.ts` (`vi.mock`, `vi.mocked`, etc.).

Do **not** use `bun test` to run the frontend suite. `bun test` invokes Bun's native test runner, which does not understand Vitest APIs like `vi.mock`/`vi.mocked` and will fail the Vitest-based tests. (`bun test` is only appropriate for Bun-native test files if any are added in the future.)

## Agent skills

### Issue tracker

Issues live in GitHub Issues at `github.com/ChristianPerez34/gospel`. See `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical triage roles mapped to GitHub labels (default names). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout — one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Skill system

Gospel discovers user-authored skills from the workspace and global data directory. See `docs/agents/skills.md` for the system overview, matcher spec, slash command semantics, and skill optimization. See `docs/agents/skills-frontmatter.md` for the SKILL.md schema and parser rules. See `docs/agents/skills-scripts.md` for script execution rules. See ADR-0011 for staged optimization.

## Shell, git, and GitHub CLI tools

Gospel exposes three agent-facing tools for workspace command execution:

- `run_shell_command`: Run a program directly with arguments in the active workspace (without implicit shell parsing).
- `run_git_command`: Run a git command in the active workspace.
- `run_github_cli_command`: Run a `gh` command in the active workspace.

### Safety model

- Read-only commands run directly.
- Auto mode executes every command-tool request without approval, regardless of risk, workspace scope, or allow/reject rules. This intentionally includes destructive, remote, shell-interpreter, and otherwise restricted commands; selecting Auto grants that control to the agent.
- In Ask mode, mutating commands require one-time approval. Restricted commands—including recursive root deletion, shell interpreters/metacharacters, secret-like arguments, `gh auth token`, `git push --force`, `git reset --hard`, `git clean`, and `gh repo delete`—also require approval instead of being hard-blocked.
- Ask mode can always-allow eligible workspace-scoped commands. Workspace-escaping command calls still require per-request approval in Ask; external-path requests always require per-request approval, and MCP tools need explicit server/tool allow rules.
- Invalid empty command invocations remain blocked in either mode.

### Implementation notes

- Core logic lives in `src-tauri/src/shell_tools.rs`.
- Approval is provided by the `CommandApproval` trait; the Tauri implementation uses `tauri_plugin_dialog`.
- Tools are registered in `src-tauri/src/harness_profile.rs` and gated by the classifier in `CommandPolicy`.
- Policy defaults are hard-coded for Phase 1; future phases may load per-project overrides from `.gospel/shell-policy.json`.
