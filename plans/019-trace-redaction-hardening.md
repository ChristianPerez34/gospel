# Plan 019: Harden Trace Log redaction for free-form strings and pretty-printed JSON

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 72819cd..HEAD -- src-tauri/src/trace.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `72819cd`, 2026-07-20

## Why this matters

The Trace Log is a redacted JSONL file under app-global storage. It records
every agent turn's tool calls, warnings, errors, and stop reasons — including
free-form provider `error_message` strings and pretty-printed JSON payloads.
The redactor only does a substring scan keyed on `"key":"` (no whitespace
after the colon), so it misses:

- Pretty-printed JSON like `"api_key": "sk-…"`.
- Free-form error strings that commonly embed `Bearer …`, `?key=…`,
  `Authorization:` headers (provider 401/5xx response bodies are commonly
  stringified with the request URL echoed).

Secrets leaking into provider error messages end up persisted,
unredacted, in `~/Library/Application Support/gospel/traces/trace-*.jsonl`,
with a 30-day retention window (CONTEXT.md "Trace Log"). The plan strengthens
the redactor to handle whitespace-tolerant JSON and to scan free-form strings
for known secret-token shapes.

Per Hard Rule 4 of the advisor skill: any existing credentials found during
this work must be referenced by `file:line` and credential type only — never
copy a secret value into a finding, plan, commit message, or test fixture.
Test fixtures use fake placeholder values (e.g. `REDACTED_FAKE_KEY`).

## Current state

`src-tauri/src/trace.rs`:

- Lines 204–221: `SENSITIVE_KEYS` constant — list of key names like `key`,
  `api_key`, `token`, etc.
- Lines 239–252: `redact_sensitive_value` recurses into JSON when the
  value starts with `{` or `[`; otherwise falls back to
  `redact_sensitive` (the substring scan).
- Lines 257–273: `redact_sensitive` does substring matching on the exact
  sequence `"key":"` and replaces the matched value with an off-the-shelf
  placeholder. The substring match misses optional whitespace after the
  colon (pretty-printed JSON diverges).
- `TraceEvent::Error { error_message: String, ... }` (lines 58–64) is a
  plain string — when a provider returns an HTTP 401/5xx, the error
  string frequently embeds the request URL with `?key=…` query strings or
  an `Authorization: Bearer …` header.

CONTEXT.md "Trace Log" spec: "redacted, capped JSONL… Never exposed as
agent-readable memory." Redaction is the only scrub layer; this plan
strengthens it.

Conventions:
- Tests live inline `#[test]` (search `rg "#\[test\]" src-tauri/src/trace.rs`).
- The repo's redaction tests use placeholder strings like `REDACTED` /
  `***` — match the existing assertion style.

## Commands you will need

| Purpose   | Command                                                                  | Expected on success |
|-----------|--------------------------------------------------------------------------|---------------------|
| Backend tests | `cargo test --manifest-path src-tauri/Cargo.toml -- trace::`         | all pass           |
| Backend lint | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`     | exit 0            |
| Backend (full) | `cargo test --manifest-path src-tauri/Cargo.toml`                     | all pass          |

## Scope

**In scope**:
- `src-tauri/src/trace.rs` (redactor changes + tests)

**Out of scope**:
- Stopping secret-bearing strings from reaching the Trace Log upstream
  (orthogonal; over-redaction at the redactor is the safer failure mode
  per AGENTS.md "Implementation notes").
- New TraceEvent variants — this plan only hardens redaction of existing
  payloads.
- A different retention policy or cap size (CONTEXT.md spec is 250 MB / 30d).
- Rotation of any real credential found during the work (if a stray old
  token is observed in the existing on-disk trace files, that is out of
  scope; report the `file:line` of the offending on-disk file to the user
  and recommend rotation — do not delete trace files as part of this plan).

## Git workflow

- Branch: `advisor/019-trace-redaction-hardening`
- Commit example: `fix: redact whitespace-tolerant JSON and free-form secret patterns in trace redactor`.
- Do NOT push unless instructed.

## Steps

### Step 1: Make the JSON substring scan whitespace-tolerant

In `redact_sensitive`, replace the exact `"key":"` substring scan with a
regex (or a manual scan with optional whitespace between the colon and the
quoted value). The crate `regex` may already be a dependency — check
`src-tauri/Cargo.toml` for `regex`. If present, use it; otherwise implement
a small manual scanner that allows zero-or-more whitespace characters after
the colon.

Target behavior (illustrative pseudocode of the matcher, not the exact
replacement):

```text
For each key in SENSITIVE_KEYS:
    match the pattern: "KEY"\s*:\s*"<chars-up-to-next-quote>"
```

Replace the captured quoted value with the redaction placeholder. Keep the
existing key preservation (e.g. `"token":"***"`); do not collapse whitespace
or otherwise alter surrounding JSON structure outside the value.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml -- trace::` →
existing redaction tests still pass.

### Step 2: Always run `redact_sensitive_value` as JSON when it could be JSON

In `redact_sensitive_value` (lines 239–252):

- Today: if the value begins with `{` or `[`, parse as JSON and recurse;
  otherwise fall back to `redact_sensitive`.
- New: when the value is parseable as JSON (try `serde_json::from_str` within
  a graceful fallback), recurse into the parsed structure regardless of the
  leading character. When parsing fails, fall back to a free-form string
  scan (Step 3). Do not change the successful parsing branches; just widen
  them.

This closes the case where a string value is itself standalone JSON with
leading whitespace or pretty-printed spacing. Embedded JSON inside surrounding
free-form text is handled by Step 1's whitespace-tolerant substring scanner,
not by whole-string JSON parsing.

**Verify**: Add a test `redacts_pretty_printed_json_string_value` whose string
content is the standalone parseable value
`"  {\n  \"api_key\": \"REDACTED_FAKE\"\n}"`. Assert the output no longer
contains `REDACTED_FAKE` and uses the repository's actual placeholder,
`[REDACTED]`.

### Step 3: Free-form scan for known secret-token shapes

In `redact_sensitive` (or a new sibling `redact_freeform_secrets`), add
matching for common secret token shapes that appear in HTTP error
messages:

Do NOT match arbitrary long strings; match specific prefix patterns with
concrete character classes:

- `Bearer ` followed by a non-whitespace token (≥20 chars) → redact the
  token, keep the `Bearer ` label.
- `token=` / `key=` / `api_key=` in query-string-style substrings → redact
  the value up to the next `&` or string end.
- Token-prefix patterns used by major providers — match these specific
  shapes only:
  - `sk-[A-Za-z0-9_-]{20,}` (OpenAI convention)
  - `ghp_[A-Za-z0-9]{36,}` (GitHub personal access token)
  - `gho_[A-Za-z0-9]{36,}` (GitHub OAuth token)
  - `ghs_[A-Za-z0-9]{36,}` (GitHub server-to-server)
  - `sk-ant-[A-Za-z0-9_-]{20,}` (Anthropic)

Use the `regex` crate if available; otherwise manual scanners with bounded
character classes. Inline a constant table of token-prefix patterns at the
top of the module (near `SENSITIVE_KEYS`) so future additions are obvious.

CRITICAL: All regex patterns and test fixtures use FAKE placeholder values
(e.g. `REDACTED_FAKE_KEY`). Never copy a real credential into the regex,
test fixture, or commit message. If you need to verify the pattern shape,
read the public provider docs.

**Verify**: Add tests (each its own `#[test]`):

- `redacts_bearer_token_in_error_message` — input `"HTTP 401: Authorization: Bearer ghp_REDACTEDFAKE0123456789ABCDEFGH"` ; assert `Bearer ` is preserved
  and the token is replaced with the placeholder.
- `redacts_openai_key_in_query_string` — input `"GET /v1/completions?key=sk-REDACTEDFAKE123456789 failed"` ; assert the `sk-…` value is redacted.
- `redacts_github_oauth_token` — input `"gho_REDACTEDFAKE0123456789ABCDEFGH"` ; assert redacted.
- `does_not_redact_short_non_secret_strings` — input `"Bearer abc"` (<20 chars)
  → assert NOT redacted (avoids over-redacting legitimate short text).

### Step 4: Run the full test suite + lint

```
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

**Verify**: both exit 0. The trace-test count should rise by the number
of new tests added in Steps 2 and 3.

## Test plan

- New tests in `src-tauri/src/trace.rs`:
  - `redacts_pretty_printed_json_string_value` (Step 2)
  - `redacts_bearer_token_in_error_message` (Step 3)
  - `redacts_openai_key_in_query_string` (Step 3)
  - `redacts_github_oauth_token` (Step 3)
  - `does_not_redact_short_non_secret_strings` (Step 3)
- Existing redaction tests (the `"key":"value"` exact-match tests) MUST
  still pass — the whitespace-tolerant matcher is a superset, not a
  replacement.
- Pattern: existing `#[test]` cohort in `trace.rs`.

## Done criteria

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml -- trace::` exits 0; at least 5 new tests pass
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] `rg "Bearer |\bsk-|ghp_|gho_|ghs_|sk-ant-" src-tauri/src/trace.rs` returns the new token-prefix patterns (or equivalent manual scanners)
- [ ] No real credential value exists in any test fixture or commit message; fixtures use `REDACTED_FAKE…` placeholders only
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row for plan 019 updated

## STOP conditions

Stop and report back if:

- `trace.rs:204-273` doesn't match the structure described (drift); re-read and adapt.
- The `regex` crate is not in `Cargo.toml` AND adding it would meaningfully
  increase compile time — STOP and propose manual scanners (with bounded
  character classes) instead of bringing in `regex`. Do not add a crate
  dependency for this mitigation without confirmation.
- An existing redaction test asserts the OLD `"key":"value"` matcher with a
  whitespace-sensitive expected output — update it; do NOT remove coverage.
  Call the change out in the commit body.
- During the work, a pre-existing real credential is observed in any on-disk
  trace file (e.g. an old test fixture that uses a real-looking token):
  immediately STOP, do not include the value in any output, and report the
  `file:line` + credential TYPE to the user with a recommendation to
  rotate. Do not silently delete or modify the offending file.
- The free-form pattern matchers produce too many false positives in the
  existing integration-simulated tests (over-redaction making test
  assertions fail) — STOP and tighten the character classes (longer
  minimums, stricter prefix matching); do not loosen the redaction.

## Maintenance notes

- When a new provider is added (e.g. a new OAuth token prefix), add its
  token shape to the constant table in Step 3 — the table is the
  deduplication seam; do not pattern-match ad hoc.
- Reviewer: confirm no real credential value appears in any new test
  (use `rg "Bearer [a-zA-Z0-9]" src-tauri/src/trace.rs` on the diff
  and visually inspect — must be all-FAKE).
- Follow-up deferred: a content-type aware redactor that re-serializes
  structured provider response bodies (rather than scanning substrings)
  is a larger change with its own risk; out of scope here.
