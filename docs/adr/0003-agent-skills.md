# ADR 0003: Agent Skills

## Status

Accepted

## Context

Gospel is an LLM coding assistant. Users want to steer the LLM's behaviour for specific tasks (TDD, debugging, design critique, etc.) without manually pasting instructions every turn. The existing `.agents/skills/` directory in the repo already contains 15 skill definitions used by other agent tooling. Gospel needs to discover, match, and inject these skills into the LLM context.

## Decision

### Workspace + Global Sourcing

Skills are discovered from two locations:

1. **Workspace skills**: `<workspace>/.agents/skills/<name>/SKILL.md` — project-specific, version-controlled with the repo.
2. **Global skills**: `<app_data_dir>/skills/<name>/SKILL.md` — user-installed, shared across all workspaces.

When a skill name exists in both locations, the workspace version wins. The discovery scan walks each location, parses each `SKILL.md`, and validates the frontmatter.

### Hybrid Execution

Skills influence the LLM in two ways:

1. **Body injection**: The full markdown body of a skill is injected into the system preamble when the user invokes it via slash command (`/<skill-name>`). This steers the LLM's behaviour for that turn.
2. **Script execution**: Skills may bundle executable scripts in a `scripts/` directory. The LLM can invoke these scripts via the `run_skill_script` tool. Scripts are executed in the workspace directory with a configurable timeout.

### Local Keyword Match

Skill matching uses a local token-overlap formula, not an LLM-driven selection. This is deterministic, fast, and free. The algorithm:

1. Tokenize the user prompt and each skill's `name` + `description` into lowercase alphanumeric tokens.
2. Filter out stopwords (loaded from `skills/stopwords.json`).
3. Score each skill: `matched_query_tokens / total_query_tokens` (recall-style).
4. Filter to skills scoring >= 0.1.
5. Sort by score descending, then workspace source before global.
6. Take the top 3.

Workspace skills win score ties over global skills.

## Consequences

- Skills are discoverable without network access or LLM calls.
- The matcher runs on every turn with negligible cost.
- Users can create project-specific skills by adding a `SKILL.md` to their repo.
- The slash command palette gives users explicit control over which skill is active.
- Script execution is sandboxed to the skill directory via `canonicalize()` path guards.
