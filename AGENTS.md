## Package manager

This project uses [Bun](https://bun.sh/) as its package manager (`bun.lock` is present at the repo root). Prefer `bun` over `npm` for installing dependencies, running scripts, and executing project commands.

## Agent skills

### Issue tracker

Issues live in GitHub Issues at `github.com/ChristianPerez34/gospel`. See `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical triage roles mapped to GitHub labels (default names). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout — one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Skill system

Gospel discovers user-authored skills from the workspace and global data directory. See `docs/agents/skills.md` for the system overview, matcher spec, and slash command semantics. See `docs/agents/skills-frontmatter.md` for the SKILL.md schema and parser rules. See `docs/agents/skills-scripts.md` for script execution rules.

## Shell, git, and GitHub CLI tools

Gospel exposes three agent-facing tools for workspace command execution:

- `run_shell_command`: Run a non-shell program with arguments in the active workspace.
- `run_git_command`: Run a git command in the active workspace.
- `run_github_cli_command`: Run a `gh` command in the active workspace.

### Safety model

- Read-only commands run directly.
- Mutating or destructive commands require one-time user approval via a native Tauri dialog.
- Hard-blocked commands always fail without approval:
  - `rm -rf /` or `rm -rf /*`
  - Commands containing shell metacharacters (`;`, `|`, `&`, `$`, `` ` ``, `<`, `>`, newline, carriage return, NUL)
  - `git push --force`, `git reset --hard`, `git clean`
  - `gh repo delete`
  - Shell interpreters (`sh`, `bash`, `zsh`, `powershell`, etc.) invoked through `run_shell_command`
- Workspace-escaping paths require approval.

### Implementation notes

- Core logic lives in `src-tauri/src/shell_tools.rs`.
- Approval is provided by the `CommandApproval` trait; the Tauri implementation uses `tauri_plugin_dialog`.
- Tools are registered in `src-tauri/src/llm.rs` and gated by the classifier in `CommandPolicy`.
- Policy defaults are hard-coded for Phase 1; future phases may load per-project overrides from `.gospel/shell-policy.json`.
