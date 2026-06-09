# ADR 0004: Persisted Sessions in App-Global Storage

## Status

Accepted

## Context

Gospel needs persistent sessions that survive restarts, support workspace affinity, and carry both user-visible transcript data and backend-only continuation history. The shared harness substrate (`.gospel/`) already exists as a workspace-scoped location for harness artifacts, but it is designed for human-inspectable, human-editable files like `PLAN.md`. Persisted sessions have different requirements:

- They must be indexed and queried by backend commands, not browsed as files.
- They carry backend-only Model History that should not be exposed through workspace-readable harness artifacts.
- They must survive workspace switches and support unscoped (workspace-free) mode.
- They need atomic transactional updates for concurrent read/write safety.

## Decision

Store persisted sessions in app-global SQLite storage, separate from the shared harness substrate. Session records live in the app data directory alongside the existing app config store, governed by backend commands. The `.gospel/` substrate remains the workspace-scoped location for human-facing harness artifacts.

## Consequences

- Session data stays behind backend commands and never leaks into workspace-readable files.
- Workspace-affine sessions can be filtered efficiently by workspace binding without filesystem traversal.
- Unscoped sessions (no workspace binding) have a natural home without special-casing `.gospel/`.
- The harness substrate stays clean: no binary blobs, no backend-only history, no indexed session data.
- Session persistence uses the same SQLite foundation as the app config store, keeping infrastructure minimal.
- If session storage fails, Gospel degrades to in-memory sessions for the current run with an explicit warning.
