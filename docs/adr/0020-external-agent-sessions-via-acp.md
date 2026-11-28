# ADR 0020: External Agent Sessions via the Agent Client Protocol

## Status

Accepted. Amends the prompt-assembly contract for external-agent backends and
qualifies ADR-0005, ADR-0017, and ADR-0019 for sessions whose continuation
lives outside Gospel.

## Context

Factory Droid is integrated through the Agent Client Protocol (ACP): Gospel
spawns the user's `droid` binary as a subprocess and speaks JSON-RPC over
stdio. This differs structurally from every existing provider. Rig-backed
providers are stateless completion endpoints — Gospel owns the entire Model
History and re-sends it on every turn. Droid is a stateful agent: it owns its
own session state server-side (in the subprocess), including its internal
message history, tool-call records, compaction, and caches. Gospel's
continuation token is the ACP session id, and `session/load` resumes exact
state.

Several existing contracts assume the stateless-completion shape:

- `CONTEXT.md` places skills in the system preamble and forbids Memory from
  the user prompt or Model History. ACP defines no system-prompt channel that
  Droid advertises (live Droid 1.21.0 does not list `systemPrompt` among its
  prompt capabilities), so the only channel for Harness context is the user
  message itself.
- ADR-0005/0017/0019 define the two histories (Display Transcript, Model
  History) as the complete continuation state, with the Conversation as sole
  interpreter. ACP sessions add a third, external continuation store that
  Gospel cannot serialize, prune, or slice.
- ADR-0018's runtime contract (max-turn, deadline, loop guards, verification)
  assumes Gospel orchestrates tool calls. In an ACP turn, Droid orchestrates
  its own loop internally; Gospel observes the stream.

## Decision

**Context carriage.** For external-agent backends only, Harness context
(invoked/matched skill sections, Memory, transcript seed context) is composed
into the user message as clearly delimited sections. This is a documented,
scoped exception to the `CONTEXT.md` preamble contract: it applies solely
when the backend exposes no system-prompt channel. If a future ACP schema or
agent advertises a system-prompt capability, Gospel must prefer it and narrow
this exception.

**Continuation ownership.** For ACP sessions, the Display Transcript remains
the user-visible record and is owned by the Conversation exactly as before.
The Model History column is `NULL` because there is no provider-native
history Gospel can faithfully represent — the authoritative continuation
state is the external ACP session, keyed by the stored `acp_session_id` and
`acp_backend` columns. Resume order: `session/load` with the stored id; on
failure (expired or evicted), a fresh session seeded with a concise
transcript-context preamble. That fallback is the sanctioned degraded path
and applies whether or not an id was stored.

- Failed turns: the prompt is recorded in the Display Transcript as usual;
  there is no Model History row to amend. ADR-0017's failure rule is vacuous
  for ACP sessions because the external agent owns what it retained.
- Retry: retrying the latest turn reseeds from the transcript (the sanctioned
  fallback). Mid-thread retry is blocked with a user-facing error directing
  to Fork, because `session/fork` preserves exact external state while a
  transcript-reseeded mid-thread retry would silently rewrite later history.
  If `messageRetraction` semantics are verified end-to-end in future, this
  guard may be relaxed in a follow-up.
- Fork uses `session/fork` when the agent advertises the capability, then
  persists the new external id on the forked session.

**Runtime contract.** ACP turns run outside `agent_runtime`'s tool loop by
necessity: the external agent is the loop. Gospel retains deadline-free
streaming, cancellation (via `session/cancel` plus process teardown), and the
approval bridge. Step verification is delegated: when a fallback Rig provider
is configured, verification and title generation run through it; otherwise
they are skipped rather than faked through the ACP stream.

**Filesystem and approvals.** The ACP client capability surface is minimized
to Gospel's existing policy: `fs/read_text_file` and `fs/write_text_file` are
advertised only when a workspace is open, writes additionally require
`source_edit_allowed`, all paths are canonicalized and contained to the
workspace, `.gospel` internals and secret-like files are always refused, and
read-only shell commands auto-resolve while mutating commands route through
the Approval Broker with workspace-escape analysis. Droid's autonomy level is
pinned to `normal` (Auto Off) at session setup so mutations reach Gospel as
permission requests rather than being executed unilaterally; this mirrors
Auto mode because the Broker auto-approves in that mode.

**Deferred.** Two spec items are deliberately deferred and tracked as
follow-ups, documented here so the deferral is explicit: (1) long-lived ACP
connection reuse across turns — every turn currently spawns a fresh `droid
exec` process, which is correct but slower; (2) `terminal/*` client handlers
and the `terminal-auth` fallback — terminal output is not yet surfaced.

## Consequences

- The exception is narrow: Memory/skills compose into the user message only
  for external-agent backends, and the sections remain delimited so they are
  recognizable in the external session record.
- The Session Store grows `acp_backend` alongside `acp_session_id` so a
  stored id is always interpreted against the backend that issued it.
- Because Gospel cannot inspect the external history, transcript fidelity
  depends on mapping the ACP update stream faithfully; unknown update kinds
  are dropped rather than persisted raw.
- Cancellation teardown kills the subprocess, so a cancelled turn's external
  session is abandoned; the next turn reseeds. This is acceptable because the
  transcript is the user's record of truth.
