# ADR 0008: Session Mode

## Status

Accepted

## Context

Gospel sessions previously had no explicit low-risk mode for planning, review, or exploration. A solo developer could ask the agent to avoid edits, but the main workspace-aware agent still received `source_edit` whenever an active workspace was available. That made accidental workspace mutation a prompt-discipline problem instead of a harness guarantee.

The existing skill system can steer behaviour through preamble injection, but ADR-0003 keeps skills separate from tool registration. A skill should not become the mechanism that revokes tools. A separate session kind would also split existing session lifecycle concepts without changing Display Transcript or Model History semantics.

## Decision

Add a persisted Session Mode metadata field with two values:

- `Build`: the normal coding mode. The main workspace-aware agent receives `source_edit`.
- `ReadOnly`: an exploration and planning mode. The main workspace-aware agent does not receive workspace source mutation tools such as `source_edit`.

Session Mode is stored on the Session row in app-global SQLite and is included in create, get, and list payloads. Existing sessions default to `Build` during schema migration.

Mode gating applies at turn setup beside the existing role-based `source_edit` gate. It does not change the Display Transcript, Model History, active workspace binding, corpus availability, Trace Log policy, or Verification Agent scheduling. The Harness Control Area remains available: `write_harness_file` is still registered so `.gospel/PLAN.md` can remain a live planning artifact in Read-Only Sessions.

The UI exposes one always-visible two-state control on every session. Switching modes is allowed mid-session and applies to subsequent turns. The confirmation is inline and recoverable, not modal.

## Consequences

- Users can plan, review, and inspect a workspace with a backend-enforced guarantee that `source_edit` is unavailable for future turns.
- Skills remain behaviour-steering mechanisms, not tool-registration policy.
- Display Transcript and Model History continuity survive mode flips because the Session identity does not change.
- Existing Session rows migrate non-destructively to `Build`.
- Tool registration now depends on both agent role and Session Mode, so future workspace-mutation tools must join the same mode gate.
