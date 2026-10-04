# ADR 0012: Build-Only Sessions

## Status

Accepted — supersedes ADR-0008.

## Context

ADR-0008 added a persisted Session Mode (`Build` / `ReadOnly`, labelled "Plan" in the UI) that withheld `source_edit` from the main agent for planning, review, and exploration sessions. In practice the mode went unused: current models follow "don't edit" instructions reliably enough that a backend-enforced read-only mode earns its complexity poorly, and planning remains available through `.gospel/PLAN.md` and the Harness Control Area in every Session.

## Decision

Session Mode is removed as a persisted, user-facing concept. Every Session runs as Build: the main agent receives `source_edit` whenever a workspace is active, and no mode control appears in the UI. Sub-agent roles stay read-only through the existing role gate. Internal consumers that must replay a turn without write access — the skill-optimization student and optimizer — withhold `source_edit` via a flag on `ActiveWorkspaceContext`, not via a Session concept.

## Consequences

- The `mode` column in `sessions`/`archived_sessions` becomes vestigial in existing databases and is not created in new ones; no data migration is needed.
- The `update_session_mode` IPC is removed and `create_session` no longer accepts a mode.
- The Harness Profile's mutation gate is `role == Main && source_edit_allowed`; future workspace-mutation tools must join the same gate.
