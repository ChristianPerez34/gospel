# ADR 0005: Display Transcript vs Model History Separation

## Status

Accepted

## Context

Gospel streams agent responses to users in real time. The user-visible messages (user prompts and assistant replies) form the Display Transcript — what the user sees and can export. Behind the scenes, the LLM provider maintains a Model History — the full conversation state including tool calls, tool results, and internal context that the provider needs to continue the conversation. These two views serve different purposes and have different lifecycle constraints:

- The Display Transcript is the user's record. It should be clean, exportable, and safe to share.
- The Model History is the provider's continuation state. It includes internal tool activity, may contain sensitive payloads, and is only useful for feeding back to the same provider/model.
- Failed or interrupted turns may produce partial Model History that should not overwrite the last good continuation state.
- Users may want to export, delete, or share transcripts without exposing backend internals.

## Decision

Persist the Display Transcript and backend Model History as separate fields on the Session record. The Display Transcript stores user and assistant messages in a UI-safe format. The Model History stores the full provider-native conversation state. On successful completion, both are updated. On failure, only the Display Transcript receives a visible error entry; the Model History retains the last valid state.

## Consequences

- UI and export flows read only the Display Transcript, keeping Model History hidden from user-facing surfaces.
- Failed turns append a visible error to the Display Transcript without corrupting the provider continuation state.
- Export formats can offer transcript-only (safe to share), debug (includes tool activity), and internal (full Model History) modes.
- The provider can resume from Model History without replaying visible transcript entries.
- Session delete removes both fields atomically, so no orphaned history remains.
