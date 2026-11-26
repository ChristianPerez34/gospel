# ADR 0019: One Owner for a Session's Two Histories

## Status

Accepted. Amends the cache clause of ADR-0017.

## Context

A Session is recorded as two kept-in-step histories: the user-visible Display Transcript and the provider-native Model History. Before this decision, the logic that interprets those shapes was scattered across at least five modules: `session_turn.rs` (merge and rebuild algebra, success/failure persistence policy), `session_actions.rs` (Retry/Fork slicing), `lib.rs` (paging, export projections), `model_history.rs` (legacy migration), and `conversation.rs` (an in-memory Model History cache). Every one of those modules had to understand both columns' shapes, their merge rules, and their edge cases, and the bug habitat lived in the seams — the retry command abused `successful_turn_persistence` purely to serialize a prefix, and failure persistence forked into a separate glue function with its own re-read of state the turn had already loaded.

The in-memory cache made this worse. It was only a memo of `prune(parse(sqlite.model_history))` — it carried no information SQLite lacked, had no invalidation API (nothing invalidated it on delete, archive, restore, or import), and saved one JSON parse per turn against a multi-second LLM roundtrip. It also did nothing for the local-only sessions it was presumably meant to help, because those pass `sessionId: null` and never touch it.

## Decision

One module — the **Conversation** — owns every write of the Display Transcript and Model History, and every read that interprets their shape: continuation loading for new turns (parse, legacy-migrate, prune), successful/failed/controlled-stopped turn outcomes, Retry and Fork slicing, transcript paging, export projections, and the skill-harvest feed.

- The interface is session-scoped (`Conversation::for_session(store, session_id)`) with intention-revealing operations, each re-reading the persisted row it needs. It composes `SessionStore` internally; consumers — Session Turn, the Tauri commands — hold it concretely. There is deliberately no trait at this seam: SQLite already substitutes in-memory in tests, and a one-implementation port would recreate the parallel-universe test harness this decision removes.
- The in-memory Model History cache is deleted. SQLite is the single source of truth; migration and pruning are pure functions applied on read.
- The Conversation does **not** own verbatim row copying (archive, restore, import), Session metadata (title, status, workspace binding, notes), workspace authorization, in-flight-turn guards, cancellation, or Session Turn Events. Those stay in the Session Store or the command layer.
- Persistence-failure policy in a Session Turn is unchanged: errors are logged, the turn still completes, and Memory ingest only follows a successful write.

Alternatives considered and rejected: a minimal two-verb (`read`/`write`) sum-type interface, which moves complexity into untyped variant pairing and turns misuse into runtime panics; a maximal outcome/cut-matrix interface with speculative generality no caller consumes; a snapshot-backed object with revision-guarded compare-and-write — sound hardening, but it guards a race the in-flight-turn guard already closes. The revision guard remains compatible future hardening behind the same interface.

## Consequences

- ADR-0017's cache clause is vacuous: there is no cache left to keep in agreement. The persistence rules it defines are unchanged and now live entirely inside the Conversation.
- `SessionTurnSessions` shrinks to metadata operations; the `SessionTurnConversation` port and the cache-fronting read paths are gone. Session Turn tests assert persistence outcomes against real in-memory SQLite rather than fake recordings — strictly stronger, since fakes can drift and SQLite cannot.
- Pruning now happens exactly once, on read, instead of at cache-write time; the persisted Model History is never rewritten by reads.
- The degraded-mode storage promise of ADR-0004 remains broken but no worse; fixing it (in-memory SQLite fallback at startup) is tracked separately as gospel#332.
