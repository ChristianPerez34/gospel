# Skills Frontmatter

## Schema

The `SKILL.md` file must start with a YAML frontmatter block delimited by `---`:

```yaml
---
name: my-skill
description: Short description of what this skill does.
argument-hint: "[command] [target]"
user-invocable: true
disable-model-invocation: false
allowed-tools:
  - Bash(npx my-tool *)
timeout-seconds: 60
license: MIT
---
```

### Required Fields

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Must match the folder name exactly. |
| `description` | string | Short description shown in the slash palette and auto-match preamble. |

### Optional Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `argument-hint` | string | — | Hint shown in the palette for expected arguments. |
| `user-invocable` | bool | `true` | Whether the skill appears in the slash palette. |
| `disable-model-invocation` | bool | `false` | If `true`, excluded from auto-match scoring. |
| `allowed-tools` | string[] | `[]` | Tool permissions for the skill. |
| `timeout-seconds` | u64 | `30` | Per-skill script execution timeout override. |
| `license` | string | — | License identifier. |

## Parser Limitations

- The frontmatter must not contain `---` within a YAML value. The parser uses `splitn(2, "\n---\n")` after stripping the opening `---\n`, so embedded `---` in values will break parsing.
- CRLF line endings are normalized to LF before parsing.
- YAML parse errors cause the skill to be skipped with a `tracing::warn!`.

## Name-Must-Match-Folder Rule

The `name` field in the frontmatter must exactly match the directory name containing the `SKILL.md`. If they differ, the skill is rejected with a `tracing::warn!` and excluded from the result. This prevents ambiguity when referencing skills by name.

## Case E: Embedded `---` in Description

If the description string contains `---`, use YAML quoting:

```yaml
description: "Use when the user says --- or asks for separators."
```
