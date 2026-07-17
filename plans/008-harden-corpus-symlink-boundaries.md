# Plan 008: Harden Corpus Symlink Boundaries

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 8c6e3ad..HEAD -- src-tauri/src/corpus/extractor.rs src-tauri/src/corpus/persistence.rs src-tauri/src/context_search.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `8c6e3ad`, 2026-07-17

## Why this matters

The corpus extractor and the corpus/context-search persistence layers all walk
the workspace with `Path::is_file` / `Path::is_dir` semantics that follow
symlinks, or write through paths that can be symlinked. Three concrete
exposures:

1. `collect_files` in `corpus/extractor.rs` (line 496-537) recurses with
   `is_file` / `is_dir`, which follow symlinks. A workspace that contains a
   symlink pointing outside its root will have those external files ingested
   into the corpus and indexed.
2. `CorpusPersistence::new` in `corpus/persistence.rs` (line 47-52) joins
   `.gospel/corpus` to the workspace path. If `.gospel` inside the workspace
   is a symlink to a directory outside the workspace, every write (graph
   JSON, SQLite DB, manifest) follows the symlink and writes outside the
   intended storage root.
3. `ContextSearchIndex::new` and `ContextSearchIndex::open_if_exists` in
   `context_search.rs` (lines 42-82) do the same with `.gospel/context_search`.

The corpus is a sensitive artifact: it is fed back to agents during review
and exploration. Ingesting attacker-controlled files outside the workspace
or letting the corpus write itself outside the workspace is a clear
boundary violation. This plan plugs the three holes.

## Current state

### `src-tauri/src/corpus/extractor.rs` (lines 495-537)

```rust
#[allow(clippy::only_used_in_recursion)]
fn collect_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<std::path::PathBuf>,
    ignore_patterns: &[&str],
) -> Result<(), ExtractionError> {
    if current.is_file() {
        files.push(current.to_path_buf());
        return Ok(());
    }

    if !current.is_dir() {
        return Ok(());
    }

    // Check ignore patterns
    if let Some(name) = current.file_name().and_then(|n| n.to_str()) {
        for pattern in ignore_patterns {
            if glob_match(pattern, name) {
                return Ok(());
            }
        }
    }

    let Ok(entries) = std::fs::read_dir(current) else {
        return Ok(());
    };

    for entry in entries.flatten() {
        let path = entry.path();
        // Skip hidden files and directories
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }
        }
        // Recurse
        let _ = collect_files(root, &path, files, ignore_patterns);
    }

    Ok(())
}
```

The function never canonicalizes and never checks symlink targets. It
recurses through `entry.path()` (which yields the symlink path, not the
target), but at every level `is_file`/`is_dir` follow the link.

### `src-tauri/src/corpus/persistence.rs` (lines 47-52)

```rust
impl CorpusPersistence {
    /// Create a new persistence manager for the given workspace
    pub fn new(workspace_path: &Path) -> Result<Self, PersistenceError> {
        let corpus_dir = workspace_path.join(CORPUS_DIR_NAME);
        Ok(Self { corpus_dir })
    }
```

`save` (line 55-78) then writes through `corpus_dir` directly. If the
workspace contains a symlink at `.gospel`, the writes follow it.

### `src-tauri/src/context_search.rs` (lines 41-82)

```rust
impl ContextSearchIndex {
    pub fn new(workspace_path: &Path) -> Result<Self, ContextSearchError> {
        let index_dir = workspace_path.join(".gospel").join("context_search");
        std::fs::create_dir_all(&index_dir)?;

        let db_path = index_dir.join("search_index.db");
        let conn = Connection::open(db_path)?;
        // ...
    }

    pub fn open_if_exists(workspace_path: &Path) -> Result<Self, ContextSearchError> {
        let db_path = workspace_path
            .join(".gospel")
            .join("context_search")
            .join("search_index.db");
        // ...
    }
```

Same pattern, same exposure.

## Commands you will need

| Purpose         | Command                                              | Expected on success               |
|-----------------|------------------------------------------------------|-----------------------------------|
| Compile check   | `cargo check --manifest-path src-tauri/Cargo.toml`   | exit 0, no errors                 |
| Lint            | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | exit 0       |
| Run tests       | `cargo test --manifest-path src-tauri/Cargo.toml`    | exit 0, all pass                  |

## Scope

**In scope**:
- `src-tauri/src/corpus/extractor.rs` — `collect_files` plus its `tests` module.
- `src-tauri/src/corpus/persistence.rs` — `CorpusPersistence::new` plus its
  `tests` module.
- `src-tauri/src/context_search.rs` — `ContextSearchIndex::new` and
  `ContextSearchIndex::open_if_exists` plus its `tests` module.

**Out of scope**:
- Changes to corpus *extraction* semantics for non-symlink files.
- Changes to corpus *content* (graph, node types, relationships).
- Changes to the corpus CLI (`src-tauri/src/bin/gospel-corpus.rs`) beyond
  whatever flows from the helper changes.
- Changes to the agent-side tool wrappers (`src-tauri/src/corpus/commands.rs`,
  `src-tauri/src/corpus/tools.rs`).
- Changes to workspace-tool policy (`workspace_tools.rs`).

## Git workflow

- Branch: `advisor/008-harden-corpus-symlink-boundaries`
- Commit message style: `security: keep corpus extraction and persistence within the workspace root`
- Use one commit for the three files, or split per-file if reviewer prefers;
  the diff should be reviewable end-to-end.

## Steps

### Step 1: Define a shared helper for canonical containment checks

Add a new private helper module file `src-tauri/src/corpus/symlink_guard.rs`
with two functions:

```rust
//! Helpers that ensure corpus operations stay within the active workspace,
//! even when the workspace contains attacker-planted symlinks.

use std::path::Path;

/// Resolve `path` to its canonical form. Returns `None` if the path does
/// not exist or cannot be canonicalized.
pub fn canonical(path: &Path) -> Option<std::path::PathBuf> {
    std::fs::canonicalize(path).ok()
}

/// Return `true` when `target` (already canonical) lives under `root`
/// (already canonical). Empty root is rejected.
pub fn is_within(root: &Path, target: &Path) -> bool {
    if root.as_os_str().is_empty() {
        return false;
    }
    target.starts_with(root)
}
```

Register the module in `src-tauri/src/corpus/mod.rs` near the other private
helpers. Add a small inline test in the same file covering both functions
with a real `tempfile::tempdir()` for the positive case and a fabricated
non-existent path for the negative case.

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` exits 0.

### Step 2: Tighten `collect_files` to skip symlinks and stay in workspace

In `src-tauri/src/corpus/extractor.rs`:

1. Change `collect_files` to take the canonical workspace root as an extra
   parameter (e.g. `canonical_root: &Path`) and skip any entry whose
   `symlink_metadata` reports `file_type().is_symlink()`. Use the same
   `symlink_metadata` check on directories: if a directory entry is itself a
   symlink, do not recurse into it.
2. After skipping a symlink, do not add the target to the file list. This
   means files outside the workspace reachable through a symlink are never
   ingested.
3. In the file-append branch (`if current.is_file()`), verify that the
   canonical form of `current` is inside the canonical root using the new
   helper. If it is not, skip the file silently (mirror the existing
   `read_dir` error-swallowing pattern at line 520-522).

Update the single caller `extract_directory` (line 474-493) to pass the
canonical root through. Resolve the canonical root once at the top of
`extract_directory` and pass it down. If the root cannot be canonicalized
(e.g. the path does not exist), the function should still proceed with the
lexical root, but the per-file check will simply reject anything that
canonicalizes away.

Sketch:

```rust
pub fn extract_directory(
    corpus: &mut Corpus,
    root_path: &Path,
    ignore_patterns: &[&str],
) -> Result<(), ExtractionError> {
    let mut files = Vec::new();
    let canonical_root = crate::corpus::symlink_guard::canonical(root_path);
    collect_files(root_path, root_path, &mut files, ignore_patterns, canonical_root.as_deref())?;

    for file_path in files {
        // ...existing extension-based dispatch...
    }

    Ok(())
}

fn collect_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<std::path::PathBuf>,
    ignore_patterns: &[&str],
    canonical_root: Option<&Path>,
) -> Result<(), ExtractionError> {
    if current.is_file() {
        if let Some(canonical_root) = canonical_root {
            if let Some(canonical_current) = crate::corpus::symlink_guard::canonical(current) {
                if !crate::corpus::symlink_guard::is_within(canonical_root, &canonical_current) {
                    return Ok(());
                }
            }
        }
        files.push(current.to_path_buf());
        return Ok(());
    }

    if !current.is_dir() {
        return Ok(());
    }

    if let Some(name) = current.file_name().and_then(|n| n.to_str()) {
        for pattern in ignore_patterns {
            if glob_match(pattern, name) {
                return Ok(());
            }
        }
    }

    let Ok(entries) = std::fs::read_dir(current) else {
        return Ok(());
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }
        }
        // Skip symlinked entries entirely so the corpus cannot ingest
        // content that lives outside the workspace root.
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        let _ = collect_files(root, &path, files, ignore_patterns, canonical_root);
    }

    Ok(())
}
```

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` exits 0.

### Step 3: Tighten `CorpusPersistence::new` to require in-workspace storage

In `src-tauri/src/corpus/persistence.rs`, replace the body of
`CorpusPersistence::new` so it canonicalizes the workspace and verifies the
resulting corpus directory lives under it.

```rust
impl CorpusPersistence {
    /// Create a new persistence manager for the given workspace.
    ///
    /// Returns an error if the workspace cannot be canonicalized or if the
    /// canonical corpus directory does not live under the canonical
    /// workspace root. This prevents writes from following a symlinked
    /// `.gospel` directory out of the workspace.
    pub fn new(workspace_path: &Path) -> Result<Self, PersistenceError> {
        let workspace_canonical = std::fs::canonicalize(workspace_path).map_err(|e| {
            PersistenceError::IoError(format!("Failed to canonicalize workspace: {}", e))
        })?;
        let corpus_dir = workspace_path.join(CORPUS_DIR_NAME);
        let canonical_corpus = std::fs::canonicalize(&corpus_dir).ok();
        if let Some(canonical_corpus) = canonical_corpus {
            if !canonical_corpus.starts_with(&workspace_canonical) {
                return Err(PersistenceError::IoError(format!(
                    "Corpus directory {} escapes the workspace",
                    canonical_corpus.display()
                )));
            }
        }
        Ok(Self { corpus_dir })
    }
```

The `PersistenceError` enum at line 535-546 currently has variants
`IoError`, `JsonError`, `DatabaseError`, `NotFound`. If `IoError` already
exists as a `String` variant, use it; if it is `std::io::Error`, add a new
string variant or use `NotFound`. Read the enum at line 535 and use the
existing shape — do not invent a new variant unless `IoError` cannot
hold the new message.

`save` already runs `std::fs::create_dir_all(&self.corpus_dir)` (line 57);
if `corpus_dir` is a symlinked `.gospel`, that operation will create the
*target* of the symlink. The check above prevents that, so
`create_dir_all` becomes a no-op or creates an in-workspace dir.

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` exits 0.

### Step 4: Tighten `ContextSearchIndex::new` and `open_if_exists` analogously

Apply the same canonical-containment check to
`ContextSearchIndex::new` and `ContextSearchIndex::open_if_exists` in
`src-tauri/src/context_search.rs`. The `ContextSearchError` enum at line 9-16
has an `Io(#[from] std::io::Error)` variant, so use it for the new error
path.

```rust
pub fn new(workspace_path: &Path) -> Result<Self, ContextSearchError> {
    let workspace_canonical = std::fs::canonicalize(workspace_path)?;
    let index_dir = workspace_path.join(".gospel").join("context_search");
    if let Ok(canonical_index) = std::fs::canonicalize(&index_dir) {
        if !canonical_index.starts_with(&workspace_canonical) {
            return Err(ContextSearchError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "Context search directory {} escapes the workspace",
                    canonical_index.display()
                ),
            )));
        }
    }
    std::fs::create_dir_all(&index_dir)?;
    // ... rest unchanged
}

pub fn open_if_exists(workspace_path: &Path) -> Result<Self, ContextSearchError> {
    let workspace_canonical = std::fs::canonicalize(workspace_path)?;
    let db_path = workspace_path
        .join(".gospel")
        .join("context_search")
        .join("search_index.db");
    if !db_path.exists() {
        return Err(ContextSearchError::NotInitialized);
    }
    if let Ok(canonical_db) = std::fs::canonicalize(&db_path) {
        if !canonical_db.starts_with(&workspace_canonical) {
            return Err(ContextSearchError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "Context search index {} escapes the workspace",
                    canonical_db.display()
                ),
            )));
        }
    }
    let conn = Connection::open(db_path)?;
    Ok(Self { conn })
}
```

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` exits 0.

### Step 5: Add tests for the new containment behavior

In the relevant `tests` modules, add unix-guarded tests. Existing
`rejects_symlink_escape` in `skills.rs:959` is a good template.

#### `src-tauri/src/corpus/extractor.rs` test

```rust
#[cfg(unix)]
#[test]
fn collect_files_skips_symlink_escape() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let outside_file = outside.path().join("outside.rs");
    std::fs::write(&outside_file, "fn outside() {}").unwrap();

    let inside_dir = dir.path().join("src");
    std::fs::create_dir_all(&inside_dir).unwrap();
    let inside_file = inside_dir.join("inside.rs");
    std::fs::write(&inside_file, "fn inside() {}").unwrap();

    // Symlink in the workspace that points outside.
    symlink(&outside_file, inside_dir.join("escape.rs")).unwrap();

    let mut corpus = Corpus::new();
    extract_directory(&mut corpus, dir.path(), &[]).unwrap();

    // The symlinked file must not be present in the corpus.
    let has_outside = corpus.nodes.values().any(|n| matches!(
        &n.node_type,
        NodeType::File { path, .. } if path.contains("outside.rs")
    ));
    assert!(!has_outside, "symlinked outside file should not be ingested");

    // The in-workspace file should be present.
    let has_inside = corpus.nodes.values().any(|n| matches!(
        &n.node_type,
        NodeType::File { path, .. } if path.contains("inside.rs")
    ));
    assert!(has_inside, "in-workspace file should be ingested");
}
```

`Corpus`, `NodeType`, and `extract_directory` are in scope within the
`tests` module (line 556-579). Adjust imports if needed.

#### `src-tauri/src/corpus/persistence.rs` test

```rust
#[cfg(unix)]
#[test]
fn corpus_persistence_rejects_symlinked_gospel_dir() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let target = outside.path().join("corpus");
    std::fs::create_dir_all(&target).unwrap();

    // Plant a .gospel symlink in the workspace pointing outside.
    let gospel = dir.path().join(".gospel");
    symlink(&target, &gospel).unwrap();

    let result = CorpusPersistence::new(dir.path());
    assert!(result.is_err(), "symlinked .gospel should be rejected");
}
```

#### `src-tauri/src/context_search.rs` test

```rust
#[cfg(unix)]
#[test]
fn context_search_new_rejects_symlinked_gospel_dir() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let target = outside.path().join("context_search");
    std::fs::create_dir_all(&target).unwrap();

    let gospel = dir.path().join(".gospel");
    std::fs::create_dir_all(&gospel).unwrap();
    symlink(&target, gospel.join("context_search")).unwrap();

    let result = ContextSearchIndex::new(dir.path());
    assert!(result.is_err(), "symlinked .gospel/context_search should be rejected");
}
```

If `context_search.rs` does not currently have a `tests` module, add one
modeled on the other modules' test setups. Search for an existing `mod
tests` block in the file first.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml -- corpus:: context_search::` exits 0 and the three new tests pass.

### Step 6: Run lint and full test suite

- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — exit 0.
- `cargo test --manifest-path src-tauri/Cargo.toml` — exit 0, all tests pass.

**Verify**: Both commands exit 0.

## Test plan

- New tests:
  - `corpus::extractor::tests::collect_files_skips_symlink_escape` — `#[cfg(unix)]`
  - `corpus::persistence::tests::corpus_persistence_rejects_symlinked_gospel_dir` — `#[cfg(unix)]`
  - `context_search::tests::context_search_new_rejects_symlinked_gospel_dir` — `#[cfg(unix)]`
- Pre-existing tests that must continue to pass: all of the
  `corpus::*` and `context_search` test modules.
- Verification: `cargo test --manifest-path src-tauri/Cargo.toml` exits 0 with
  3 more tests than before.

## Done criteria

- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` exits 0
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0; the 3 new
      tests exist and pass
- [ ] No files outside the in-scope list are modified
- [ ] `plans/README.md` status row for plan 008 updated to `DONE`

## STOP conditions

- The drift check shows non-empty output for any in-scope file. Re-open,
  reconcile, treat as a planning defect.
- A pre-existing `corpus::` or `context_search` test regresses because
  `CorpusPersistence::new` is now strict about canonicalization. If the
  failure points to the new check itself, STOP — the test setup is likely
  not on a canonicalizable path. If the failure is in pre-existing
  assertions, fix the helper rather than the assertions.
- The new helper `symlink_guard` shadows an existing module name. STOP and
  rename.
- The persistence check breaks CLI runs that operate on a path the user
  typed but has not canonicalized (e.g. via `realpath` not having resolved
  symlinks). STOP and report; this would be a behavior change beyond the
  threat model.

## Maintenance notes

- The `corpus_dir` field in `CorpusPersistence` (line 44) is still the
  lexical path. Code that consumes the persistence layer (e.g. `save`)
  uses this path to write. The canonicalization check is a *gate*, not
  a *redirect* — the writes still go where the user pointed, and a
  rejection means the user has planted a bad symlink.
- The `collect_files` change means symlinks that happen to point to
  files *inside* the workspace will also be skipped. This is intentional:
  a symlink that is part of a workspace's normal content is rare, and
  ingestion of duplicate paths through a symlink can cause corpus noise
  (id collisions, double counting). If a future plan needs symlink
  support, layer it on top with an explicit allowlist.
- A reviewer should check that `extract_directory`'s error type is
  preserved. If the new check fails, the function returns `Ok(())` and
  the corpus is simply empty for that workspace — that is the desired
  fail-soft behavior.
