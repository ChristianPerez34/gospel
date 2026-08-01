# Plan 023: Make Session Exports Safe at the Share Boundary

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer dispatched you and told you they maintain
> the index.
>
> **Drift check (run first)**: `git diff --stat 0ec1edd..HEAD -- src-tauri/src/lib.rs`
> If the export command bodies or legacy command registration changed, compare
> the live code with the current-state excerpts before proceeding.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `0ec1edd`, 2026-07-31

## Why this matters

ADR-0005 defines the Display Transcript as a clean, exportable user record and
keeps provider-native Model History separate because Model History contains tool
calls, tool results, and internal context. The current `Transcript` export
returns the stored display payload verbatim, and that payload deliberately
contains tool arguments and raw tool results so the live UI can render action
cards. The legacy `export_conversation` command also remains registered and
returns raw in-memory provider history. This plan separates the live rendering
shape from the shareable export shape and removes the unused raw export IPC
path.

## Current state

Relevant files:

- `src-tauri/src/session_turn.rs` — builds the persisted display transcript;
  it includes ordered text/tool blocks for the UI.
- `src-tauri/src/lib.rs` — defines `ExportFormat`, `export_session`, the
  legacy `export_conversation` command, and Tauri command registration.
- `src-tauri/src/conversation.rs` — keeps in-memory provider history for active
  turns; it is not itself the share/export boundary.
- `docs/adr/0005-display-transcript-vs-model-history.md` — accepted contract:
  transcript export is safe to share, debug can include tool activity, and
  internal is the only full Model History path.

The persisted display payload intentionally contains tool blocks:

```rust
// src-tauri/src/session_turn.rs:571-617
AssistantContent::ToolCall(tool_call) => blocks.push(json!({
    "kind": "tool",
    "id": tool_call.id.clone(),
    "name": tool_call.function.name.clone(),
    "arguments": observable_tool_arguments(...),
    "result": null,
    "status": "completed",
}));
```

Tool results are attached as raw strings at
`src-tauri/src/session_turn.rs:622-657`. Only `source_edit.old_text` and
`source_edit.new_text` are redacted by `observable_tool_arguments`; ordinary
tool results and arguments are retained.

The export command currently returns that payload unchanged for both public
formats:

```rust
// src-tauri/src/lib.rs:2584-2607
match format {
    ExportFormat::Transcript => Ok(detail.display_transcript),
    ExportFormat::Debug => Ok(detail.display_transcript),
    ExportFormat::Internal => { /* display transcript + model history */ }
}
```

The legacy command is still registered and serializes raw in-memory history:

```rust
// src-tauri/src/lib.rs:1470-1490 and 2896-2921
fn export_conversation(..., session_id: String) -> Result<String, String> {
    let history = store.get_history(&session_id);
    serde_json::to_string_pretty(&history).map_err(|e| e.to_string())
}
```

The frontend has no caller for `clear_conversation_history` or
`export_conversation` (`rg` over `src/` finds no invocation). The in-memory
conversation store itself remains required by active streaming and is not
removed by this plan.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Backend export tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml -- session_export` | all new export tests pass |
| Backend full tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml` | exit 0 |
| Backend lint | `cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` | exit 0 |
| Frontend typecheck | `bun run typecheck` | exit 0, no errors |

## Scope

**In scope** (the only files to modify):

- `src-tauri/src/lib.rs` — add export projection helpers/tests, apply them to
  `export_session`, and remove the unused raw `export_conversation` command
  from its definition and registration.

**Out of scope** (do not touch):

- `src-tauri/src/session_turn.rs` — the live/display transcript shape must
  remain unchanged because the UI renders its action blocks.
- `src-tauri/src/conversation.rs` — provider history storage remains needed for
  streaming; only the unneeded export command is removed.
- `ExportFormat::Internal` — it is intentionally an internal diagnostic path
  and must remain the only `export_session` mode that includes `model_history`.
- `export_archived_sessions` in `session_store.rs` — it is an internal archive
  transfer payload consumed by archive import and requires Model History for
  continuation. Do not relabel it as a shareable transcript in this plan; a
  future UX plan must make that distinction explicit if archive JSON becomes a
  user-facing sharing feature.
- Frontend UI wiring or the ADR text; this plan changes the backend boundary
  and tests only.

## Git workflow

- Branch: `advisor/023-make-session-exports-safe`
- Commit style example: `fix: separate safe transcript exports from model history`.
- Do not push or open a PR unless explicitly instructed.

## Steps

### Step 1: Define export projections without changing the stored UI payload

Add small private helpers in `src-tauri/src/lib.rs` near `ExportFormat` that
parse `detail.display_transcript` as JSON and produce two projections:

- **Transcript**: preserve only user/assistant text and existing user-visible
  error/controlled-stop metadata. Remove `blocks`, tool IDs, tool names,
  arguments, and results. Do not remove the assistant's normal `content` text.
- **Debug**: preserve the ordered action shape needed for diagnostics, but do
  not export raw tool arguments or result bodies. Include tool name, ID, status,
  and bounded metadata such as result length or a fixed redaction marker. Use
  the existing redaction seam in `trace.rs` for secret-like values, but do not
  treat secret-token redaction alone as sufficient to make file contents safe.

The helpers must handle malformed or legacy display JSON deterministically. A
malformed transcript should return a safe empty/partial projection or a clear
export error; it must never fall back to returning the raw string.

Keep `Internal` behavior unchanged: it may include the display payload and
`model_history`, because callers must explicitly choose that mode.

**Verify**: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` -> exit 0; `cargo test --locked --manifest-path src-tauri/Cargo.toml -- session_export` -> the test filter may initially report no tests, but must compile after the test module is added in Step 2.

### Step 2: Add projection tests for tool and history boundaries

Add a `#[cfg(test)]` module named `session_export` in `src-tauri/src/lib.rs`.
Construct JSON fixtures representing a user message, an assistant text block,
an assistant tool block with a file-reading result, and an error entry. Use
non-sensitive placeholder text only.

Assert:

- Transcript output contains user and assistant text and error metadata.
- Transcript output contains no `blocks`, tool name, tool ID, arguments, or
  result body.
- Debug output retains tool identity/status but not the placeholder result body
  or raw source-edit snippets.
- Internal export remains the only projection that contains a
  `model_history` property.
- Malformed display JSON does not return the original raw input.

Model the JSON assertions on `session_turn` tests at
`src-tauri/src/session_turn.rs:1607-1631` and
`src-tauri/src/session_turn.rs:1678-1730`.

**Verify**: `cargo test --locked --manifest-path src-tauri/Cargo.toml -- session_export` -> all new projection tests pass.

### Step 3: Route `export_session` through the projections

In `export_session`, keep the existing session authorization check at
`src-tauri/src/lib.rs:2591-2597`. Replace the direct `Ok(detail.display_transcript)`
branches with the Transcript and Debug projection helpers. Leave the Internal
JSON structure and Model History inclusion unchanged.

Remove the `export_conversation` function and its
`generate_handler!` registration after confirming
`rg -n '"export_conversation"' src` has no frontend call site. Do not remove the
`ConversationStore` type or its tests.

**Verify**: `rg -n 'fn export_conversation|export_conversation,' src-tauri/src/lib.rs` -> zero command definition/registration matches; `cargo test --locked --manifest-path src-tauri/Cargo.toml -- session_export` -> all tests pass.

### Step 4: Run the full verification gates

Review the diff specifically for accidental raw fallbacks and for any
`model_history` inclusion in Transcript or Debug. Confirm only Internal keeps
that field.

**Verify**: `cargo test --locked --manifest-path src-tauri/Cargo.toml` -> exit 0; `cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` -> exit 0; `bun run typecheck` -> exit 0.

## Test plan

- Add `session_export` unit tests in `src-tauri/src/lib.rs` for clean transcript,
  bounded/redacted debug, internal-only Model History, and malformed JSON.
- Use JSON fixtures with ordinary placeholder strings; never put credentials or
  real workspace file contents in tests.
- Follow the existing JSON value assertions in `session_turn.rs` rather than
  snapshotting the entire export string.
- Verification: `cargo test --locked --manifest-path src-tauri/Cargo.toml` -> all backend tests pass.

## Done criteria

- [ ] Transcript export contains only user/assistant text plus safe status
      metadata; it contains no tool blocks/results/arguments.
- [ ] Debug export contains bounded diagnostic metadata without raw tool result
      bodies or source-edit snippets.
- [ ] Internal export remains the only `export_session` mode containing Model
      History.
- [ ] The unused raw `export_conversation` IPC command is removed and no
      frontend call site exists.
- [ ] `cargo test --locked --manifest-path src-tauri/Cargo.toml` exits 0.
- [ ] `cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0.
- [ ] `bun run typecheck` exits 0.
- [ ] No files outside `src-tauri/src/lib.rs` are modified.
- [ ] `plans/README.md` status row for plan 023 is updated.

## STOP conditions

Stop and report instead of improvising if:

- The current export formats or legacy command registration differ from the
  excerpts.
- A frontend caller for `export_conversation` is discovered, or removing it
  would break a non-test runtime path.
- The frontend requires tool blocks in the exported Transcript payload rather
  than only in the stored/display payload.
- A safe Debug projection cannot be defined without changing the archived
  transfer contract; leave archive transfer untouched and report the boundary.
- Any proposed test fixture would need a real credential, real user data, or a
  network/provider call.

## Maintenance notes

- The live display transcript and the shareable Transcript export are now
  intentionally different projections. Future display fields must be reviewed
  for whether they belong in a shareable export.
- Reviewers should verify that only explicit Internal export paths can expose
  provider-native Model History.
- If archive transfer is later exposed as a sharing feature, it needs its own
  safe format; do not reuse the internal migration payload.
