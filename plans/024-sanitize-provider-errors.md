# Plan 024: Sanitize Provider Errors Before User-Facing Persistence

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer dispatched you and told you they maintain
> the index.
>
> **Drift check (run first)**: `git diff --stat 0ec1edd..HEAD -- src-tauri/src/llm.rs src-tauri/src/session_turn.rs src-tauri/src/lib.rs src-tauri/src/trace.rs`
> Compare all error conversion and sink excerpts below against live code before
> proceeding.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `0ec1edd`, 2026-07-31

## Why this matters

Provider SDK errors are uncontrolled strings. They may contain HTTP response
bodies, request URLs, authorization details, proxy diagnostics, or other
transport data that should not be shown in the UI or written into a persisted
session transcript. Gospel already has a redaction seam in `trace.rs`, and the
review pipeline already limits provider failure details, but the general LLM
error DTO still copies provider text verbatim. This plan centralizes a bounded,
redacted user-facing provider message before UI emission and failure
persistence while keeping error codes stable.

## Current state

Relevant files:

- `src-tauri/src/llm.rs` — `LlmError`, provider error construction, and
  `LlmError::to_dto`.
- `src-tauri/src/session_turn.rs` — appends `error.to_dto().message` to the
  persisted Display Transcript on failed turns.
- `src-tauri/src/lib.rs` — emits `LlmErrorDto` to the UI and exposes
  `test_connection`.
- `src-tauri/src/trace.rs` — existing `redacted_text`/JSON redaction helpers
  and token patterns; trace output is not a reason to leave UI messages raw.

The DTO currently exposes the raw provider string:

```rust
// src-tauri/src/llm.rs:72-95
LlmError::ProviderError(msg) => LlmErrorDto {
    code: "PROVIDER_ERROR".to_string(),
    message: format!("Completion failed: {}", msg),
},
```

Raw provider strings enter the error type from normal and streaming paths:

```rust
// src-tauri/src/llm.rs:172-183 and 903-913
.map_err(|e| LlmError::ProviderError(e.to_string()))?
...
Err(error) => return Err(LlmError::ProviderError(error.to_string())),
```

Failed turns persist the DTO message:

```rust
// src-tauri/src/session_turn.rs:504-526
_ => (format!("Error: {}", error.to_dto().message), false),
...
display_transcript: serde_json::to_string(&transcript)...
```

The UI and trace adapter consume the DTO:

```rust
// src-tauri/src/lib.rs:1250-1285
error_message: error.to_dto().message,
...
"message": dto.message,
```

`test_connection` still returns `e.to_string()` directly at
`src-tauri/src/lib.rs:906-914`, bypassing the DTO entirely.

The existing redaction API is `trace::redacted_text` at
`src-tauri/src/trace.rs:193-203`; its tests cover bearer tokens, provider
token-prefix patterns, sensitive JSON keys, and free-form error strings at
`src-tauri/src/trace.rs:472-555`.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| LLM tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml -- llm::` | all LLM tests pass |
| Session-turn tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml -- session_turn::` | all session-turn tests pass |
| Backend full tests | `cargo test --locked --manifest-path src-tauri/Cargo.toml` | exit 0 |
| Backend lint | `cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` | exit 0 |

## Scope

**In scope** (the only files to modify):

- `src-tauri/src/llm.rs` — add the bounded provider-error presentation helper
  and make `LlmError::to_dto` use it; add unit tests.
- `src-tauri/src/lib.rs` — route `test_connection` errors through the same
  user-facing DTO message; add no provider/network test.
- `src-tauri/src/session_turn.rs` — update failure-persistence tests or helper
  usage only if required to prove the persisted message is sanitized.

**Out of scope** (do not touch):

- Provider SDK configuration, retry policy, credentials, keychain storage, or
  provider selection.
- Trace redaction rules unless a test proves the shared helper cannot be used
  safely; trace changes would require a separate plan.
- Review-specific failure formatting in `src-tauri/src/review/mod.rs`, which
  already sanitizes detector failures.
- Returning raw diagnostics through a new channel. Diagnostics must remain
  bounded and redacted at every user-facing/persisted sink.

## Git workflow

- Branch: `advisor/024-sanitize-provider-errors`
- Commit style example: `fix: redact provider errors before persistence`.
- Do not push or open a PR unless explicitly instructed.

## Steps

### Step 1: Define one bounded provider-error presentation helper

Add a private helper in `src-tauri/src/llm.rs` near `LlmError::to_dto` that:

1. Applies the existing `trace::redacted_text` behavior to the raw message.
2. Keeps only the first non-empty line so multiline response bodies are not
   persisted or shown.
3. Truncates on a UTF-8 character boundary to a fixed cap (the review module's
   500-character cap is a reasonable local precedent).
4. Falls back to a stable generic message when the sanitized detail is empty.

Preserve the `PROVIDER_ERROR` code. Keep ordinary short diagnostics useful when
they contain no sensitive pattern, but never claim this helper makes arbitrary
provider responses safe if they still contain unrecognized secrets. The
resulting `to_dto` message must be the only user-facing representation of
`ProviderError`.

**Verify**: `cargo test --locked --manifest-path src-tauri/Cargo.toml -- llm::` -> the existing LLM tests pass after the helper compiles.

### Step 2: Cover token, multiline, and bounded-message behavior

Add unit tests in the existing `llm::tests` module. Use synthetic placeholder
token-shaped strings only; never use a real credential. Assert:

- A normal short message remains useful.
- A bearer/token-shaped substring is absent from the DTO message.
- A multiline message exposes only the sanitized first line.
- An overlong first line is bounded and remains valid UTF-8.
- `failure_turn_persistence` stores the same sanitized DTO message and does
  not replace existing Model History.

Model the persistence assertion on
`src-tauri/src/session_turn.rs:1638-1656`.

**Verify**: `cargo test --locked --manifest-path src-tauri/Cargo.toml -- llm::` -> all LLM tests pass; `cargo test --locked --manifest-path src-tauri/Cargo.toml -- session_turn::` -> all session-turn tests pass.

### Step 3: Remove the direct `test_connection` bypass

At `src-tauri/src/lib.rs:906-914`, replace the direct `e.to_string()` error
return with the same sanitized DTO message used by normal completion. Do not
change the successful `Ok(true)` result or the API-key lookup behavior.

If the command's `Result<bool, String>` shape cannot consume the helper without
changing a frontend contract, use `e.to_dto().message` and preserve the string
return type; do not return the raw `Display` implementation.

**Verify**: inspect the `test_connection` body with `rg -n -A12 "async fn test_connection" src-tauri/src/lib.rs` -> provider failures use `e.to_dto().message` or the shared sanitized helper, never `e.to_string()`; `cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` -> exit 0.

### Step 4: Run the full verification gates

Review all `LlmError::ProviderError` sinks with `rg`. Confirm UI emission,
failure persistence, `test_connection`, and trace error events all receive the
sanitized DTO or an already-sanitized message. Do not add logging of the raw
provider string while debugging tests.

**Verify**: `cargo test --locked --manifest-path src-tauri/Cargo.toml` -> exit 0; `cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` -> exit 0.

## Test plan

- Add focused DTO/presentation tests in `src-tauri/src/llm.rs`.
- Add or update `failure_turn_persistence` assertions in
  `src-tauri/src/session_turn.rs`.
- Cover UI-facing DTO and persisted transcript behavior without network calls.
- Follow existing redaction tests in `src-tauri/src/trace.rs` for synthetic
  token-shaped inputs and exact “raw value absent” assertions.
- Verification: `cargo test --locked --manifest-path src-tauri/Cargo.toml` -> all backend tests pass.

## Done criteria

- [ ] `LlmError::ProviderError` DTO messages are redacted, single-line, bounded,
      and UTF-8 safe.
- [ ] Failed session transcript entries contain the sanitized message only.
- [ ] `test_connection` does not return `LlmError::to_string()` for provider
      errors.
- [ ] Tests prove synthetic token-shaped and multiline/oversized details are
      not exposed.
- [ ] `cargo test --locked --manifest-path src-tauri/Cargo.toml` exits 0.
- [ ] `cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0.
- [ ] No files outside the listed three source files are modified.
- [ ] `plans/README.md` status row for plan 024 is updated.

## STOP conditions

Stop and report instead of improvising if:

- `LlmError::ProviderError` or `LlmErrorDto` has changed shape since the
  excerpts.
- The existing trace helper mutates input, requires runtime state, or cannot be
  safely reused without changing `trace.rs`.
- A provider SDK error contains structured data that cannot be safely reduced
  to a stable first-line message without a provider-specific contract.
- A test asks for a real provider, API key, network, or credential value.
- A caller depends on the exact raw provider error text as a public contract;
  report the caller and stop rather than preserving the leak.

## Maintenance notes

- New provider adapters must create `ProviderError` values with raw details
  only internally; all UI, transcript, and trace sinks must pass through the
  DTO/presentation boundary.
- Reviewers should search for new `e.to_string()` conversions in provider
  command paths and ensure they do not bypass `to_dto`.
- Provider-specific diagnostic detail can be added later through a deliberately
  redacted support channel; it must not be reintroduced into the normal session
  transcript.
