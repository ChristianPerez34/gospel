# Plan 033: Apply the Secret-File Policy to Context-Search Indexing

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 2982ea5..HEAD -- src-tauri/src/corpus/commands.rs src-tauri/src/workspace_tools.rs`
> If either file changed since this plan was written, compare the "Current state"
> excerpts against the live code before proceeding; on a mismatch, treat it as
> a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `2982ea5`, 2026-10-10

## Why this matters

Gospel's workspace tools refuse to read secret-like files (private keys, credential files, most `.env.*` variants, `.npmrc`, `*.pem`/`*.key`/`*.pfx`…) through the `is_secret_like` policy in `src-tauri/src/workspace_tools.rs`. The context-search index (`src-tauri/src/corpus/commands.rs`) applies a much weaker hardcoded skip list that names only `.env`, `.env.local`, and `.env.production`. Every other secret-shaped file in the workspace — `.env.development`, `.npmrc`, `id_rsa`, `credentials.json`, `secrets.yaml`, `server.pem`, `client.key` — gets read at indexing time and chunked into the persistent SQLite FTS database at `.gospel/context_search/search_index.db`.

That creates a durable plaintext secret cache inside the workspace that (a) the `context_search` IPC can return to the agent, bypassing the `is_secret_like` read gate entirely, and (b) survives deletion of the source file until the index is rebuilt.

## Current state

`src-tauri/src/corpus/commands.rs` (~550 LOC) drives context-search indexing. The file-iteration loop reads each collected file and chunks it (~lines 50–64):

```rust
            } => {
                // Skip binary/dependencies/generated files
                if should_skip_for_indexing(path) {
                    continue;
                }

                // Read file content
                let full_path = workspace_path.join(path);
                if let Ok(content) = std::fs::read_to_string(&full_path) {
                    let chunks = if language == "markdown" || path.ends_with(".md") {
                        context_search::chunk_markdown_file(std::path::Path::new(path), &content)
                    } else {
                        context_search::chunk_source_file(std::path::Path::new(path), &content)
                    };
                    all_chunks.extend(chunks);
                }
            }
```

`should_skip_for_indexing` (lines ~100–139) — the full secret coverage is three env names:

```rust
fn should_skip_for_indexing(path: &str) -> bool {
    let skip_patterns = [
        "node_modules",
        "target",
        ".git",
        "dist",
        "build",
        ".gospel/corpus",
        ".gospel/context_search",
        ".gospel/sessions",
        ".gospel/traces",
        "Cargo.lock",
        "package-lock.json",
        "yarn.lock",
        ".env",
        ".env.local",
        ".env.production",
    ];
    ...
```

The policy it should honor already exists and is `pub(crate)` — `src-tauri/src/workspace_tools.rs:2444-2484`:

```rust
pub(crate) fn is_secret_like(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let lower = name.to_string_lossy().to_ascii_lowercase();

    if lower == ".env" || lower == ".npmrc" {
        return true;
    }

    if lower.starts_with(".env.") {
        return !matches!(
            lower.as_str(),
            ".env.example" | ".env.sample" | ".env.template"
        );
    }

    if lower.starts_with("id_rsa") || lower.starts_with("id_ed25519") {
        return true;
    }

    if lower.starts_with("credentials") || lower.starts_with("secrets") {
        return true;
    }

    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("pem") | Some("key") | Some("crt") | Some("cer") | Some("p12")
            | Some("pfx") | Some("der") | Some("jks") | Some("keystore")
    )
}
```

Note: `is_secret_like` inspects only the file name and extension, so passing the workspace-relative `path` argument works unchanged.

No data migration is needed: `index.clear()` runs at `commands.rs:40` on every index build, so previously indexed secret chunks are purged on the next rebuild.

**Convention**: unit tests for this function already exist at `commands.rs:541-550`:

```rust
        assert!(should_skip_for_indexing(
            ...
        ));
        assert!(should_skip_for_indexing("Cargo.lock"));
        ...
        assert!(!should_skip_for_indexing("src/node_modules_helper.rs"));
        assert!(!should_skip_for_indexing("src/targeted.rs"));
```

## Commands you will need

| Purpose   | Command                                              | Expected on success |
|-----------|------------------------------------------------------|---------------------|
| Rust tests (module) | `cargo test --manifest-path src-tauri/Cargo.toml corpus` | all tests pass |
| Full Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all pass |
| Clippy    | `bun run clippy`                                     | exit 0, no warnings |

## Scope

**In scope** (the only files you should modify):
- `src-tauri/src/corpus/commands.rs` — `should_skip_for_indexing` + its unit tests

**Out of scope**:
- `src-tauri/src/workspace_tools.rs` — `is_secret_like` is already correct and `pub(crate)`; do not move or widen it.
- `src-tauri/src/context_search.rs`, `src-tauri/src/corpus/persistence.rs`, `src-tauri/src/corpus/extractor.rs` — the query path and schema are fine; the bug is only the indexing gate.
- The `skip_patterns` hardcoded list — keep the three explicit `.env` entries (harmless redundancy with `is_secret_like`; do not "clean up").
- The symbol-level corpus extractor (`extract_directory`) — see Step 3: you will *verify* it cannot ingest secret-shaped files, not change it.

## Git workflow

- Branch: `advisor/033-secret-policy-context-search`
- Commit style: conventional commits, e.g. `fix(corpus): apply secret-file policy to context-search indexing`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Consult `is_secret_like` in the indexing gate

At the top of `should_skip_for_indexing` in `src-tauri/src/corpus/commands.rs`, add:

```rust
    if crate::workspace_tools::is_secret_like(std::path::Path::new(path)) {
        return true;
    }
```

Place it before the `skip_patterns` loop so the secret policy is evaluated first. If `Path`/`std::path::Path` is already imported in the module, use the existing import style (check the file's `use` block — `std::path::Path` is already used at the call site for `chunk_*` helpers, so a plain `Path::new(path)` may compile if imported; match whatever the module already does).

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml corpus` → existing tests pass (the three `.env*` patterns still skip — now via `is_secret_like`).

### Step 2: Extend the unit tests

In the existing test block near `commands.rs:541-550`, add assertions for the newly covered shapes:

```rust
        assert!(should_skip_for_indexing(".env.development"));
        assert!(should_skip_for_indexing(".env.test"));
        assert!(should_skip_for_indexing(".npmrc"));
        assert!(should_skip_for_indexing(".ssh/id_rsa"));
        assert!(should_skip_for_indexing("keys/id_ed25519"));
        assert!(should_skip_for_indexing("credentials.json"));
        assert!(should_skip_for_indexing("config/secrets.yaml"));
        assert!(should_skip_for_indexing("server.pem"));
        assert!(should_skip_for_indexing("client.key"));
        assert!(should_skip_for_indexing("cert.p12"));

        assert!(!should_skip_for_indexing(".env.example"));
        assert!(!should_skip_for_indexing(".env.sample"));
        assert!(!should_skip_for_indexing(".env.template"));
        assert!(!should_skip_for_indexing("src/main.rs"));
        assert!(!should_skip_for_indexing("docs/credentials-guide.md"));
```

Note the last case: `is_secret_like` matches on `file_name()` prefix, so `credentials-guide.md`… actually `lower.starts_with("credentials")` DOES match `credentials-guide.md` — change that expected line: use `docs/secrets-playbook.md` → it also matches (starts_with "secrets") → assert `true` instead, and use a plainly-safe name like `docs/credential-rotation.md`… which ALSO starts with "credentials"? No — `starts_with("credentials")` on `credential-rotation.md` is false (missing the trailing 's'). Use `docs/credential-rotation.md` → `assert!(!should_skip_for_indexing(...))`. Write the assertions to reflect actual `is_secret_like` semantics — run the tests and correct any expectation that mismatches rather than weakening the check.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml corpus` → all pass.

### Step 3: Verify the symbol-level extractor cannot ingest secret files (read-only check)

The separate corpus symbol index (`extract_directory` in `src-tauri/src/corpus/extractor.rs` → `persistence.save`) stores extracted symbols, not raw content — and it parses only languages backed by the declared tree-sitter grammars (`tree-sitter-rust`, `tree-sitter-typescript` in `Cargo.toml`). Confirm by reading `extractor.rs`'s file filter that secret-shaped files (`.pem`, `.key`, `id_rsa`, `.env.*`, `credentials*`) are not ingested. If you find the extractor DOES read such files into persisted output, do NOT fix it here — report it as a new finding.

**Verify**: no command — record the answer in your completion report.

## Test plan

Covered by Step 2 — unit tests in the existing `commands.rs` test block.

Verification: `cargo test --manifest-path src-tauri/Cargo.toml` → all pass.

## Done criteria

- [ ] `should_skip_for_indexing` consults `crate::workspace_tools::is_secret_like` first
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0; new assertions cover `.env.*` variants, `.npmrc`, `id_rsa`/`id_ed25519`, `credentials*`/`secrets*`, and `*.pem`/`*.key`/`*.p12`
- [ ] `bun run clippy` exits 0
- [ ] Step 3 extractor answer recorded (safe / new finding reported)
- [ ] Only `src-tauri/src/corpus/commands.rs` modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- `should_skip_for_indexing` is called from additional paths that pass absolute (not workspace-relative) paths — `is_secret_like` only inspects the file name so it still works, but flag it for review.
- `is_secret_like` has moved or its signature changed.
- The symbol extractor turns out to ingest secret files (Step 3) — that becomes a second finding, not part of this plan.
- Fixing requires touching `workspace_tools.rs` or the schema — out of scope; report.

## Maintenance notes

- The secret policy now has one definition (`is_secret_like`) and two consumers (workspace tool reads, context-search indexing). Any future tool that reads workspace files into persistent or agent-visible storage must consult the same function — that is the invariant a reviewer should check.
- `.gospel/context_search/` is already in `skip_patterns` and `.gitignore`d upstream — the index never indexes itself; this fix doesn't change that.
- Deferred: purging secrets already written to an existing `search_index.db` in a user's workspace happens automatically on the next index build (`index.clear()` at `commands.rs:40`). No migration was written — if the team wants an immediate purge, trigger one corpus rebuild after deploy.
