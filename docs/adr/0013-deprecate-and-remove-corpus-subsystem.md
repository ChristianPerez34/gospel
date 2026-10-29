# ADR 0013: Deprecate and Remove Corpus Subsystem

## Status

Accepted — supersedes ADR-0002.

## Context

Gospel originally relied on an offline Tree-sitter knowledge graph ("corpus") and SQLite FTS index ("context-search") to supply structural codebase knowledge to models when LLM context windows were small (8k–32k) and iterative multi-turn exploration was expensive.

ADR-0002 made corpus optional and introduced live workspace exploration tools (`read_file`, `search_code`, `find_files`, `list_directory`). With current frontier models, autonomous codebase navigation using ripgrep and targeted file reads is fast, accurate, and always consistent with live disk state.

Maintaining the bespoke AST graph and context search subsystems imposed significant liabilities:
- Over 3,500 lines of custom Rust code and external C Tree-sitter bindings (`tree-sitter`, `tree-sitter-rust`, `tree-sitter-typescript`).
- Narrow language support: only Rust and TypeScript were parsed; all other languages derived no AST benefit while still paying indexing overhead.
- In active git worktrees and branches, static graph caches become stale after edits.
- Security and concurrency surface: repeated hardening was needed for symlink workspace escapes (Plan 008), secret leaks into SQLite FTS databases (Plan 033), and non-reentrant auto-build locks (`CORPUS_BUILD_LOCK`).

## Decision

Completely remove the corpus AST graph, context-search index, and auto-build machinery:

1. **Delete Corpus Subsystem & Context Search**: Remove `src-tauri/src/corpus/`, `src-tauri/src/context_search.rs`, and associated examples.
2. **Preserve Symlink Guards**: Move `symlink_guard.rs` to `src-tauri/src/symlink_guard.rs` so workspace boundary security for `.gospel/PLAN.md` remains intact.
3. **Drop Tree-Sitter Dependencies**: Remove `tree-sitter`, `tree-sitter-rust`, and `tree-sitter-typescript` from `src-tauri/Cargo.toml`.
4. **Clean Backend Orchestration**:
   - Remove background auto-build startup hooks, workspace switch builders, non-reentrant locks, and event emitters.
   - Remove `corpus_summary`, `corpus_query`, `corpus_neighbors`, and `context_search` tools from `HarnessProfile`.
   - Remove `corpus_available` and `corpus_enabled` gating from `ActiveWorkspaceContext`, `HarnessProfile`, and `SessionTurnWorkspace`.
   - Remove corpus Tauri IPC commands (`build_corpus`, `get_corpus_status`, `get_corpus_summary`, `query_corpus`, `get_corpus_neighbors`, `context_search`).
   - Remove the built-in `codebase_knowledge` MCP server.
5. **Clean Frontend**: Remove `corpus-auto-build-complete` listener in `useChatStream.ts` and tool activity card formatters in `toolActivityCards.ts`.

## Consequences

- Worktrees based on `origin/dev` start instantly with zero background indexing delay and zero disk cache footprint in `.gospel/`.
- All language navigation is unified under safe, live workspace exploration tools (`read_file`, `search_code`, `find_files`, `list_directory`).
- Binary build time and dependency footprint are reduced by dropping Tree-sitter C dependencies.
- Future deep semantic intelligence (definitions, references, call hierarchies) can be integrated via standard compiler-backed Language Server Protocol (LSP) instead of maintaining an ad-hoc AST graph database.
