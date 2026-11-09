---
name: bounty-logic
description: Test business-logic and state-machine bugs on black-box web targets: ordering, rollback side effects, contradictory states, and reversible changes.
---

# Bounty Logic

Logic bugs live in the gap between what the UI allows and what the server enforces.

## Model the state machine

For each workflow (signup, invite, checkout, refund, transfer, publish, delete, revoke), list:

- States and the events that move between them.
- Which events are supposed to require which role or payment.
- Which steps are idempotent.
- Where the server checks state, and where it only assumes it.

## Test

- Reorder steps. Skip a step. Repeat a step. Send two steps concurrently.
- Fail or cancel a transaction, then check whether the side effect persisted.
- Put an object into two contradictory states (paid and refunded, invited and removed, published and deleted) and try every operation.
- Reverse a state change as a role that should not be able to.
- Use an object after deletion or revocation.
- Chain two individually safe operations into an unsafe outcome.
- Replay a request after its precondition changed.

## Evidence

Record the full sequence, not just the final request. Include timestamps, request/response evidence IDs, and the state you observed after each step. A logic finding without the sequence is not reproducible.
