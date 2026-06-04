# Agent Skills

## Overview

Gospel discovers user-authored **skills** from the workspace and global data directory. A skill is a `SKILL.md` file with YAML frontmatter and a markdown body. Skills can steer the LLM's behaviour and bundle executable scripts.

## Discovery

Skills are scanned from two locations on every `list_skills` call (with caching):

1. `<workspace>/.agents/skills/<name>/SKILL.md`
2. `<app_data_dir>/skills/<name>/SKILL.md`

Workspace skills take precedence over global skills when names collide.

## Matcher Spec

The auto-matcher runs on every turn and produces a `## Active Skills` preamble section:

- **Formula**: Token-overlap recall — `matched_query_tokens / total_query_tokens`.
- **Tokenization**: Lowercase, split on non-alphanumeric, filter tokens <= 1 char, remove stopwords.
- **Stopwords**: Loaded from `src-tauri/src/skills/stopwords.json` (English common words + filler).
- **Threshold**: Score >= 0.1.
- **Cap**: Top 3 matches.
- **Tiebreak**: Workspace source wins over global at equal score.
- **Suppression**: The auto-match list is suppressed when a slash-invoked skill is active for that turn.

## Slash Command Semantics

Users can type `/<skill-name>` in the input bar to explicitly invoke a skill:

- The slash must lead the first non-whitespace line of the input.
- Multi-line args are captured: `/<skill-name> first line\n\nremaining text`.
- Selecting from the palette inserts `/skill-name ` and closes the menu.
- Sending `/skill-name args` strips the slash, captures `args`, and calls `complete_streaming` with `invokedSkill: { name, args }`.
- The invoked skill's full body is injected into the preamble; the auto-match list is suppressed.
- Unknown skill names show an inline warning. Pressing Esc sends the literal text unchanged.
- Prefix completion via Levenshtein distance shows "Did you mean: /x?" when the filter has zero matches. Tab accepts.

## Cache

The skill discovery cache is a `RwLock<HashMap<PathBuf, Vec<Skill>>>` keyed by canonical workspace path:

- Reads acquire a read lock; on a miss, discovery runs and the result is written.
- `set_active_workspace` drops the cache entry for the old and new paths.
- `reload_skills` Tauri command clears the active entry and re-scans.
- No TTL — explicit invalidation only.
