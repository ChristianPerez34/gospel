# Workspace Exploration Chain Design

## Status

Approved in conversation. Ready for implementation planning.

## Scope

This spec covers the issue chain:

- #75 Read workspace files from chat
- #76 Search workspace code from chat
- #77 Discover workspace files and directories from chat
- #78 Keep chat working when corpus is unavailable
- #79 Delegate broad investigations to an Exploration Agent

## Goal

Let Gospel's streaming Agent inspect the active workspace safely during chat without depending on the corpus as the only codebase knowledge source.

The design must:

- keep all live exploration scoped to the active Gospel workspace
- prevent path escapes, secret exposure, and high-noise traversal
- preserve streaming chat even when corpus build or load fails
- add a second Exploration Agent for broad multi-file investigations

## Current Context

Today, `src-tauri/src/llm.rs` registers only corpus-backed Rig tools. `src-tauri/src/lib.rs` resolves the active workspace and currently fails streaming chat if `ensure_workspace_corpus` fails. Corpus tools also still accept an optional workspace path and fall back to the process current directory when no explicit workspace is available.

That architecture conflicts with the issue chain in three ways:

1. Live file inspection is not available at all.
2. Corpus availability is a hard dependency for workspace-aware chat.
3. Workspace boundaries are too loose for safe chat-time exploration.

## Decision Summary

Use a shared backend workspace-tools module and implement all live exploration as first-class Rig tools.

This design is preferred over per-tool one-off implementations because the full chain needs one consistent answer for:

- workspace scoping
- symlink and path-escape blocking
- hidden-path and secret-file policy
- text and size caps
- deterministic traversal and truncation metadata
- tool labeling in the UI

## Architecture

Add a new backend module, expected at `src-tauri/src/workspace_tools.rs`, with shared infrastructure for all live exploration tools.

Primary responsibilities:

1. `WorkspaceAccess`
   - resolve the active workspace root
   - normalize relative and absolute user paths
   - convert successful outputs back to workspace-relative paths
   - block path escapes, including symlink escapes
   - support safe handling of paths that do not yet exist by validating the nearest existing ancestor

2. `SafetyPolicy`
   - centralize hidden-path allow/block logic
   - block secret-like filenames
   - detect binary files
   - enforce per-tool caps and truncation behavior
   - define broad-tool ignore rules for noisy and generated content

3. Live Rig tools
   - `read_file`
   - `search_code`
   - `find_files`
   - `list_directory`
   - `delegate_exploration`

4. Exploration Agent support
   - build a second backend-only agent with the same provider and model as the active chat
   - reuse the same workspace and safety policy
   - exclude recursive delegation

Corpus tools remain separate. They provide indexed structural knowledge, while live workspace tools provide source-of-truth reads and searches.

## Safety Policy

The agreed default policy is balanced.

### Workspace Scope

- All live tools operate only inside the active workspace selected in Gospel.
- Inputs may be relative or absolute.
- All successful outputs are returned as workspace-relative paths.
- Existing targets must canonicalize inside the canonical workspace root.
- Missing targets must validate against their nearest existing ancestor and still fail cleanly if the final target is not safe.
- Symlink escapes are blocked for direct reads, scoped searches, and directory traversal.

### Hidden Paths

Allow normal developer configuration and repo metadata that is routinely useful during coding:

- `.github/**`
- `.vscode/**`
- `.devcontainer/**`
- `.cargo/**`
- `.agents/**`
- `.opencode/**`
- `.gitignore`
- `.gitattributes`
- `.gitmodules`
- `.editorconfig`
- `.env.example`
- `.env.sample`
- `.env.template`
- `.nvmrc`
- `.tool-versions`

Block risky hidden areas and hidden paths outside the allowlist, including:

- `.git/**`
- `.svn/**`
- `.hg/**`
- `.ssh/**`
- `.aws/**`
- `.gnupg/**`
- `.direnv/**`
- `.npm/**`
- `.pki/**`
- `.DS_Store`

### Secret-Like Files

Block secret-like targets even when they are inside the workspace.

Block patterns include:

- `.env`
- `.env.*` except example, sample, and template variants
- `.npmrc`
- `*.pem`
- `*.key`
- `*.crt`
- `*.cer`
- `*.p12`
- `*.pfx`
- `*.der`
- `*.jks`
- `*.keystore`
- `id_rsa*`
- `id_ed25519*`
- `credentials*`
- `secrets*`

### Binary Detection

Use a deterministic sampled-prefix check.

- Treat the file as binary if the sampled prefix contains NUL bytes.
- Treat the file as binary if the sampled prefix is not valid UTF-8.

### Broad-Tool Ignore Rules

Broad traversal tools are stricter than explicit file reads.

`search_code`, `find_files`, and `list_directory` skip noisy or generated areas such as:

- `node_modules`
- `target`
- `dist`
- `build`
- `.next`
- `.nuxt`
- `coverage`
- `tmp`
- minified asset patterns

Direct `read_file` may still read an explicit safe target like `Cargo.lock` or `package-lock.json` if it is not otherwise blocked.

## Tool Contracts

All expected user-correctable failures return structured tool results instead of ending the turn.

### `read_file`

Arguments:

- `path`
- optional `start_line`
- optional `end_line`

Success payload:

- `success: true`
- `path`
- `size_bytes`
- `start_line`
- `end_line`
- `total_lines`
- `truncated`
- `content`

`content` is line-numbered and capped.

Recoverable failure payload:

- `success: false`
- `reason`: `invalid_range`, `not_found`, `blocked`, `binary`, `oversized`, or `io_error`
- `message`

Unsafe reads never expose content.

### `search_code`

Arguments:

- `pattern`
- optional `path`
- optional `include_glob`
- optional `max_results`

Success payload:

- `success: true`
- `matches`
- `truncated`
- `scanned_files`
- `skipped_files`

Each match includes:

- `path`
- `line`
- `text`

Invalid regex returns a recoverable payload with `success: false`, `reason: invalid_regex`, and a readable message.

### `find_files`

Arguments:

- `glob`
- optional `path`
- optional `max_results`

Result payload:

- `success: true`
- `files`
- `truncated`
- `scanned_entries`

Returns files only and always uses workspace-relative paths.

### `list_directory`

Arguments:

- optional `path`
- optional `depth`
- optional `max_entries`

Result payload:

- `success: true`
- `entries`
- `truncated`
- `visited_entries`

Each entry includes:

- `path`
- `name`
- `kind`
- `size_bytes` for files

Sort order is deterministic: directories first, then files, both alphabetically. Directory recursion does not follow symlinked directories.

### `delegate_exploration`

Arguments:

- required `task`
- optional `context`
- optional `expected_output`

Result envelope:

- `success: true`
- `truncated`
- `report`
- `tools_used`
- `message`

`report` is capped markdown with fixed sections:

- `Summary`
- `Key Files`
- `Findings`
- `Constraints`
- `Suggested Next Reads`
- `Tools Used`

The Exploration Agent cannot call `delegate_exploration` recursively.

## Caps

### `read_file`

- default line cap: `200`
- absolute line cap: `400`
- total response text cap: `64 KiB`
- per-line display cap: `500` characters
- block files larger than `1 MiB`

### `search_code`

- default match cap: `50`
- absolute match cap: `200`
- scan up to `500` safe files or `16 MiB` cumulative text
- skip individual files larger than `256 KiB`

### `find_files`

- default result cap: `100`
- absolute result cap: `500`
- visited-entry cap: `5,000`

### `list_directory`

- default depth: `2`
- absolute depth: `5`
- default entry cap: `200`
- absolute entry cap: `1,000`
- visited-entry cap: `5,000`

### `delegate_exploration`

- report text cap: `32 KiB`
- timeout: `90` seconds

## Runtime Flow

### Active Workspace Handling

`complete_streaming` resolves the active workspace once at turn start.

If there is no active workspace:

- chat still runs
- no live workspace tools are registered
- corpus tools are also unavailable
- there is no fallback to the process current directory

If there is an active workspace:

- live workspace tools are registered immediately
- corpus availability is checked separately
- if corpus exists, corpus tools are registered too
- if corpus auto-build fails, Gospel emits the existing `corpus-auto-build-complete` failure event and continues the turn with live tools only

### Corpus Integration Changes

- Corpus tools must stop letting the model choose arbitrary workspace roots.
- Corpus tools must use the active Gospel workspace selected by the app.
- Corpus tool resolution must stop falling back to `std::env::current_dir()`.
- Prompting must distinguish corpus knowledge from live source-of-truth reads.

### Prompting Split

Keep a short live-tools prompt alongside the corpus prompt.

The live-tools prompt should tell the model:

- use corpus for fast structural orientation when available
- use live workspace tools for source-of-truth reads and verification
- prefer `find_files` or `search_code` before broad claims
- use `delegate_exploration` only for broad multi-file or architecture-oriented investigations

## UI Activity Labels

Replace the current generic `Searching X...` label construction with a fixed mapping:

- `read_file` -> `Reading file...`
- `search_code` -> `Searching code...`
- `find_files` -> `Finding files...`
- `list_directory` -> `Listing directory...`
- `delegate_exploration` -> `Exploration Agent investigating...`

Unknown tools may fall back to a title-cased name.

## Exploration Agent Design

The Exploration Agent is a backend-only helper used by `delegate_exploration`.

Requirements:

- use the same provider and model as the active chat
- support ChatGPT OAuth consistently with the main Agent
- use the same active workspace and safety policy
- receive live workspace tools and corpus tools when available
- exclude recursive delegation
- run synchronously with a `90` second timeout
- return structured `success: false` envelopes for provider failures and timeouts

## Testing Strategy

### Backend Unit Coverage

Add unit tests for:

- workspace path normalization
- symlink escape blocking
- hidden-path allow and block decisions
- secret-file blocking
- binary detection
- truncation behavior
- invalid range handling
- deterministic sorting
- delegated report truncation

### Tool Coverage

Add focused tests for:

- `read_file` happy path and recoverable failures
- `search_code` include glob behavior, invalid regex handling, sorting, caps, and skipped files
- `find_files` scoped path behavior, glob behavior, sorting, and result caps
- `list_directory` depth behavior, entry caps, file metadata, and symlink-directory skipping

### Streaming and Integration Coverage

Verify where practical that:

- no active workspace means no live tools are registered
- corpus auto-build failure no longer aborts the streaming turn
- workspace-aware behavior never falls back to the process current directory
- delegated exploration prompt construction and tool registration are correct without requiring live provider credentials

### Frontend Coverage

Frontend changes are intentionally small. Add label-mapping verification only if the repo already has a cheap path to test that code. Otherwise keep frontend verification manual and focused.

## Documentation Changes

Implementation must also:

- add glossary entries to `CONTEXT.md` for `Live Workspace Tools` and `Exploration Agent`
- add an ADR for workspace-scoped live exploration and the corpus-versus-live-source distinction

## Non-Goals

This chain does not include:

- arbitrary filesystem access outside the active workspace
- write, edit, or delete file tools
- recursive multi-agent delegation
- replacing the corpus with live search for every task

## Recommended Delivery Order

1. `#75`: create the shared workspace access and safety-policy layer, then ship `read_file`
2. `#76`: add `search_code` on top of the shared safety and scoping layer
3. `#77`: add `find_files` and `list_directory` using the same traversal rules
4. `#78`: update streaming integration so corpus failure becomes recoverable and workspace resolution is strict
5. `#79`: add `delegate_exploration`, glossary updates, and the workspace exploration ADR

This order keeps the foundation small, validates the safety layer early, and avoids reworking streaming behavior after delegation is introduced.
