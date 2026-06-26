# ADR 0007: Multi-Focus Code Review Plumbing

## Status

Accepted

## Context

Gospel's review pipeline began as a security-only workflow. The same review harness now needs to support additional review lenses without turning one tool call into a bundled multi-agent review, and without breaking existing security review invocations, review config files, or rejected-finding suppression.

## Decision

Rename the canonical agent tool to `run_review`. Keep `run_security_review` as a deprecated compatibility alias that always runs the `Security` focus and does not accept a focus argument.

Each tool call runs exactly one Review Focus. Multi-focus review is orchestration by the agent across several `run_review` calls, not one tool call that internally fans out.

Keep the legacy flat signal-rule lists as the shared defaults. Add per-focus signal rules as additive overlays, so workspace config can extend classification for one focus without replacing legacy Security behavior or changing the global noise gate.

Scope rejected-finding hashes by Review Focus and bump the hash domain from v1 to v2. A rejection in one focus must not suppress or be cleared by a finding at the same file, lines, and title in another focus.

## Consequences

- Existing security review callers can continue using `run_security_review` while new code targets `run_review`.
- Review prompts, comments, results, run records, metrics, and outcome recording can carry focus metadata before non-security detector knowledge is added.
- Existing `.gospel/review_config.json` files remain valid because flat `signal_rules` keep their merge semantics.
- Existing rejected-finding hashes are not rewritten in place; new rejections use the focus-scoped v2 hash.
