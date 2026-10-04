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
- Selecting from the palette inserts `/skill-name` followed by a space and closes the menu.
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

## Skill optimization

Gospel can harvest Skill Invocations from Display Transcripts and stage a bounded edit of one skill under `.gospel/skill-opt/<name>/`. Discovered skills change only through Skill Adoption (`adopt_staged_skill`), never by auto-writing `.agents/skills/`.

- **Harvest**: slash-invoked user turns for the named skill. Keeps the stripped prompt and subsequent tool *names*. Drops tool arguments, results, Model History, and Trace Log.
- **Split**: harvests the 50 most recently updated workspace sessions (oldest of those first). With two or more tasks, the last 20% (at least one) is the selection split, so gating uses the most recent Skill Tasks. A single task cannot gate.
- **Patches**: add/delete/replace/insert/append, default budget 4. Missing, ambiguous, empty, no-op, and slow-update-region edits are skipped. Frontmatter is not editable.
- **Selection Gate**: accept only when the candidate mean is strictly greater than the current mean. Ties reject. Rejected edits append to `.gospel/skill-opt/<name>/rejected.json`.
- **Protected region**: `<!-- gospel:slow-update -->` … `<!-- /gospel:slow-update -->` in the body cannot be overwritten by step-level edits.
- **Student injection**: candidate evaluation uses the full body (`## Invoked Skill`), not the description-only auto-match list. `prepare_student_skill_turn` builds that turn.
- **Scoring**: Verification Agent `pass` = 1.0, `concerns` = 0.5, `fail` = 0.0. `unavailable` is omitted from the Selection Gate mean.
- **Conclude**: `conclude_skill_optimization` stages a candidate on accept and appends the Rejected Edit Buffer on reject. It never writes `.agents/skills/`.
- **Replay scores**: `score_skill_replay` pairs current vs candidate Verification Agent outcomes, drops any pair with `unavailable`, then runs the Selection Gate.
- **Replay runner**: `replay_and_conclude` executes each Skill Task twice as a student turn (full body, no auto-match), scores both with a `SkillReplayVerifier`, then concludes. `LlmStudentExecutor` + `StreamCompletionStudentLlm` is the production student (Read-Only Session Mode, invoked skill body, empty history). `VerificationAgentVerifier` is the production scorer and calls the Verification Agent.
- **Adopt UI**: `list_staged_skills` feeds the debug panel (`?panel=skill-opt`). Adopt copies a Staged Skill into `.agents/skills/<name>/SKILL.md` and reloads discovery.
- **Opt-in replay**: `run_skill_selection_replay` harvests Skill Tasks, refuses to call the model when the selection split is empty, then replays only the hold-out with `LlmStudentExecutor` + `VerificationAgentVerifier` and concludes. It is not run automatically.
- **Opt-in optimizer**: `optimize_skill_from_sessions` asks a Read-Only optimizer model for a JSON edit array (budget 4, rejected-edit buffer in the prompt), applies patches, then runs selection replay. Empty hold-out skips the optimizer. Empty/unusable edits skip the student. Never auto-adopts. Open the panel from the command palette (**Optimize skill**) or `?panel=skill-opt`. It uses the current session model, shows gate scores plus a body diff, and surfaces errors. Adopt remains explicit.

See ADR-0011.
