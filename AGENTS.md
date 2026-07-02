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
