---
name: bounty-report
description: Record bug-bounty signals and findings with evidence, reproduction, impact, and honest confidence.
---

# Bounty Report

A signal is a candidate. A finding is a reproducible claim. Keep them separate.

## Recording signals

Use the `record_bounty_signal` tool. It appends one JSON object per line to `.gospel/bounty/runs/<run-id>/signals.jsonl` and deduplicates on (kind, method, endpoint) — a repeat returns the existing signal id instead of writing.

`kind` is one of the hypothesis classes:

- `enforcement_gap` — supposed to enforce something, but doesn't
- `inconsistent_enforcement` — enforced in one place, assumed in another
- `unchecked_assumption` — relied on, never checked
- `undocumented_dependency` — depends on undocumented behavior
- `composition_vulnerability` — safe alone, dangerous chained
- `inconsistent_access_path` — same object, differently-permissioned paths
- `identity_rebinding` — object rebound without required checks
- `failed_request_side_effect` — rejected request still changes something
- `ordering_dependency` — check holds only under one event order
- `partial_rollback_side_effect` — rollback leaves side effects behind
- `post_delete_access` — deleted/revoked object still usable
- `unauthorized_state_reversal` — state reversal without permission
- `contradictory_state` — two contradictory states at once
- `other`

Required args: `kind`, `endpoint`, `summary`. Optional: `method`, `evidence_ids`, `confidence` (`low|medium|high`, default `medium`), `status` (`candidate|confirmed|rejected|needs_review`, default `candidate`), `hypothesis`.

Every signal should carry the `evidence_ids` from the `http_request` calls that demonstrate it — a signal without evidence is a guess, not a lead.

## Finding report

For each confirmed finding:

- Title and severity.
- Affected endpoint and parameter.
- Roles/auth profiles involved.
- Secure expectation and observed behavior.
- Evidence IDs and a minimal reproduction.
- Impact and a realistic attack scenario.
- Suggested fix.
- What you did not test.

## Honesty rules

- No finding without a reproduction and an evidence ID.
- Do not inflate severity from a status code alone.
- Mark untested variants `needs_review` instead of guessing.
- Redact tokens, cookies, and personal data from the report; cite evidence IDs.
