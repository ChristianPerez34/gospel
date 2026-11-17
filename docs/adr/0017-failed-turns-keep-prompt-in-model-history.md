# ADR 0017: Failed Turns Keep Their Prompt in Model History

## Status

Accepted. Supersedes the failure rule in ADR-0005.

## Context

ADR-0005 kept Model History at its last successful state when a turn failed or hit a Controlled Stop. Only the Display Transcript recorded the failed prompt and its error.

The rule protected provider continuation state from partial tool activity, but it had a cost. After a failure, the next prompt (often "continue" or "try again") reached the model without the request it referred to. The user could see the failed prompt, but the model could not. Retry also had to handle a Model History with one fewer user turn than the Display Transcript.

## Decision

When a turn fails or hits a Controlled Stop, Gospel appends the prompt that was sent to the model to Model History as a plain user message. It does not record any partial assistant output, tool calls, or tool results from the failed run, so the history stays valid for the provider.

- Model History stores the effective prompt (including delegated reports and verification notes), the same text a successful turn records. The Display Transcript stores the prompt as typed.
- The failure row in the Display Transcript carries `prompt_in_model_history: true`. Merging a rebuilt Display Transcript uses this flag to match failed turns by position. A failed turn keeps its persisted entries rather than being duplicated by the rebuild.
- Failure rows without the flag (written before this decision, or when the persisted Model History could not be parsed) are display-only, as under ADR-0005.
- The in-memory Model History cache is refreshed from the same result that is persisted, so the cache and SQLite agree. If the session has no persisted record, neither is changed.

Successful turns, export, and the separation of the Display Transcript from Model History are unchanged from ADR-0005.

## Consequences

- A follow-up prompt after a failure sees the failed request.
- Model History can contain consecutive user messages (a failed prompt followed by the next one).
- Retry counts failed prompts as user turns. Older sessions that lack them are still accepted.
- Repeated identical prompts cannot be confused, because failed turns are matched by position, not text.
