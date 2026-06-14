# ADR 0006: Exact-Replacement Source Edit Tool

## Status

Accepted

## Context

Gospel's Live Workspace Tools originally gave the main agent source-of-truth read access to the active workspace. That made investigation reliable, but mutation still depended on the assistant explaining edits for the human to apply elsewhere. Gospel needs a narrow write path that can support normal coding turns while keeping workspace safety, trace redaction, and user-visible change review explicit.

## Decision

Add a `source_edit` Live Workspace Tool for the main workspace-aware agent only. The tool applies exactly one in-place replacement to an existing UTF-8 file in the active workspace.

The tool requires:

- a workspace-relative or in-workspace absolute `path`
- non-empty `old_text`
- `new_text` that differs from `old_text`
- exactly one occurrence of `old_text` in the target file

The tool rejects unsafe targets with explicit reasons:

- paths outside the active workspace, including symlink escapes
- `.gospel/**` harness control data
- hidden control directories
- secret-like files
- generated/noisy directories
- lockfiles
- generated files
- symlinked files
- binary or invalid UTF-8 files
- files above the edit size cap
- missing, ambiguous, or no-op replacements

Successful edits write atomically and return structured metadata plus a capped replacement-scoped diff preview. The model receives the normal tool result, but app-facing tool-call payloads and Trace Log arguments redact `old_text` and `new_text`.

`source_edit` is not registered for the Exploration Agent, Security Review detector/validator agents, or the Verification Agent. Verification remains read-only.

## Consequences

- The main agent can complete small code edits without leaving the user to manually apply snippets.
- Every successful source edit is visible in chat as an **Edit file** action card with a capped diff preview.
- Source-edit turns can be detected after streaming and scheduled for read-only Verification Agent follow-up even when the assistant's final response is short.
- Harness control artifacts remain mutable through the existing `write_harness_file` contract, not through general source edits.
