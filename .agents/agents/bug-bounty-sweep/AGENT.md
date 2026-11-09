---
name: bug-bounty-sweep
description: Deterministic bug bounty swarm. A mutator generates bounded hypotheses from scope + threat model, cheap workers test them against the scoped HTTP tool, feedback rounds pursue new signals, and a lead model triages every signal into a report.
tools:
  - read_file
  - http_request
  - record_bounty_signal
skills:
  - bounty-authz
  - bounty-logic
  - bounty-report
max_turns: 12
deadline_seconds: 300
network: true
orchestration: sweep
# model and worker_model fall back to the session model when unset.
# model: frontier lead for mutator and triage stages, e.g.:
# model:
#   provider: openai
#   model: gpt-5
# worker_model: cheap workers that test each hypothesis, e.g.:
# worker_model:
#   provider: openai
#   model: gpt-4o-mini
---

Deterministic sweep orchestration. The Rust orchestrator owns the loop — you do not drive it.

How a `@bug-bounty-sweep <task>` run works:

1. **Scope gate** — refuses before any model call unless `.gospel/bounty/scope.yaml` exists, parses, sets `authorized: true`, and has in-scope rules.
2. **Mutate** — the lead model reads scope, the task, any `recon.md`/`threat-model.md` under `.gospel/bounty/` or the run dir, and prior signals/hypotheses, then emits a capped JSON hypothesis set.
3. **Workers** — each hypothesis runs one bounded worker turn on `worker_model` with only `read_file`, `http_request`, and `record_bounty_signal` (max 12 turns, 5-minute deadline, 4 concurrent). Worker signals append to `signals.jsonl` and deduplicate on (kind, method, endpoint).
4. **Feedback** — rounds that produced new signals feed a bounded follow-up mutation (up to 2 rounds).
5. **Triage** — the lead model classifies every signal as confirmed, rejected, or needs_review, writes `triage.jsonl`, and produces `sweep-report.md`.

Artifacts land under `.gospel/bounty/runs/<run-id>/`: `hypotheses.jsonl`, `signals.jsonl`, `triage.jsonl`, `sweep-report.md`, `evidence/`, and `requests.jsonl`.
