# ADR 0018: Direct Desktop Agent Runtime Replaces Rig ECS

Supersedes ADR-0015 (`docs/adr/0015-ecs-agent-runtime.md`).

## Context

ADR-0015 put a Bevy ECS world (`rig-ecs` + `bevy_*`, `src-tauri/src/ecs_runtime.rs`)
between Gospel and the model for every agent run, to gain bounded retries with
backoff on transient errors. The ECS kernel only owns turn plumbing; Gospel
still owned budgets, loop guards, approval, tools, persistence, and stream
events via five custom systems plus a Tokio bridge and a blocking worker loop.

The only thing Gospel wanted from the ECS move was the backoff.

## Decision

Drive `rig-core` directly: one sequential model→tool loop per run in
`src-tauri/src/agent_runtime.rs`, no Bevy, no `rig-ecs`:

- Per-model-call retry (up to 3, 1s doubling capped at 30s, provider
  `Retry-After` honored up to 60s) re-sending the same request snapshot, so
  committed tools never re-execute — the same guarantee the ECS review system
  gave. The `upstream_error` patch stays until Rig classifies that code.
- Unchanged contract: `RunRequest`/`run` signature, `StreamEvent`s
  (including `RetryReset`), `max_turns`/`None`-means-unlimited, loop and
  deterministic-failure guards, approval suspension/cancellation, deadline
  timeouts, subagent step ordering, session prompt-cache key, variant params
  and reasoning effort.
- All 19 runtime contract tests port verbatim (`agent_runtime/tests.rs`).

## Consequences

- Drops `rig-ecs` and the four pinned `bevy_*` crates; deletes ~1130 lines of
  runtime/bridge and ~530 pruned lock lines.
- No AgenticPlugin stages, no session journal (already disabled per ADR-0015),
  no jitter on backoff (single desktop user).
- Callers (`llm`, `verification`, `review`, `bounty`, `harness_profile`) change
  only the module path.
