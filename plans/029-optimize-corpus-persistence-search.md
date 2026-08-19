# Plan 029: Optimize Corpus Persistence Search Connection Reuse and Query Indexing

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md` — unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 9df8439..HEAD -- src-tauri/src/corpus/persistence.rs`
> If any in-scope file changed since this plan was written, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: perf
- **Planned at**: commit `9df8439`, 2026-08-02

## Why this matters

The corpus subsystem provides workspace symbol indexing and fast retrieval for LLM context retrieval. However, `CorpusPersistence::search(&self, name: &str)` currently:
1. Re-opens a new SQLite database connection (`Connection::open(&db_path)`) on every single search call.
2. Executes `SELECT id, node_data FROM nodes WHERE node_data LIKE %query% LIMIT 50`, performing an unindexed full table scan across serialized JSON text blobs.

Refactoring `search` to reuse a pooled/cached connection handle and optimizing the search query pattern eliminates disk IO churn and ensures responsive symbol lookups as corpus size grows.

## Current state

- File: [`src-tauri/src/corpus/persistence.rs`](file:///Users/lifelinelogics/RustProjects/gospel/src-tauri/src/corpus/persistence.rs) (lines 224–245):
```rust
    /// Search corpus nodes by name (safe parameterized query)
    pub fn search(&self, name: &str) -> Result<Vec<QueryResult>, PersistenceError> {
        let db_path = self.corpus_dir.join(SQLITE_DB_FILE);
        if !db_path.exists() {
            return Ok(Vec::new());
        }
        let conn = Connection::open(&db_path)?;
        let sql = "SELECT id, node_data FROM nodes WHERE node_data LIKE ?1 LIMIT 50";
        let pattern = format!("%{}%", name);
        let mut stmt = conn.prepare(sql)?;
```

## Commands you will need

| Purpose   | Command                                                        | Expected on success |
|-----------|----------------------------------------------------------------|---------------------|
| Check Rust| `cargo check --manifest-path src-tauri/Cargo.toml`             | exit 0, no errors   |
| Clippy    | `bun run clippy`                                               | exit 0, clean       |
| Rust test | `cargo test --manifest-path src-tauri/Cargo.toml -- corpus`    | all pass            |

## Scope

**In scope**:
- `src-tauri/src/corpus/persistence.rs`

**Out of scope**:
- Public API signatures of `search` or `QueryResult`.
- Changes to `src-tauri/src/corpus/extractor.rs`.

## Git workflow

- Branch: `advisor/029-optimize-corpus-persistence-search`
- Commit message style: `perf(corpus): reuse connection handle and optimize node search query`

## Steps

### Step 1: Cache or reuse connection handle in `CorpusPersistence`

In `src-tauri/src/corpus/persistence.rs`, modify `CorpusPersistence` to maintain a connection handle `conn: std::sync::Mutex<Option<Connection>>` or a helper method `get_connection(&self) -> Result<MutexGuard<'_, Option<Connection>>, PersistenceError>`:

```rust
pub fn get_connection(&self) -> Result<Connection, PersistenceError> {
    let db_path = self.corpus_dir.join(SQLITE_DB_FILE);
    if !db_path.exists() {
        return Err(PersistenceError::DatabaseNotFound);
    }
    Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ).map_err(Into::into)
}
```
Using `SQLITE_OPEN_READ_ONLY` for search operations avoids database lock contention with index write tasks.

**Verify**: `cargo check --manifest-path src-tauri/Cargo.toml` → exits 0.

### Step 2: Index or optimize node search query

Ensure node creation creates an index on `nodes(id)` or symbol name column, or optimize the parameterized search in `search`:

```rust
// Create index during table initialization if missing
conn.execute("CREATE INDEX IF NOT EXISTS idx_nodes_id ON nodes(id)", [])?;
```

Update `search(&self, name: &str)` to use `SQLITE_OPEN_READ_ONLY` flags and handle empty queries gracefully.

**Verify**: `bun run clippy` → exits 0 without warnings.

### Step 3: Run existing and new corpus unit tests

Execute all corpus tests to confirm search returns accurate results.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml -- corpus` → passes.

## Test plan

- Test `search`:
  - Initialize corpus persistence in a temp directory.
  - Insert sample symbol nodes ("SymbolA", "SymbolB").
  - Perform `search("SymbolA")` and verify `QueryResult` returned.
  - Verify multiple consecutive searches execute cleanly without file lock errors.

## Done criteria

- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` exits 0
- [ ] `bun run clippy` exits 0 with zero warnings
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml -- corpus` passes
- [ ] `plans/README.md` status row updated

## STOP conditions

- If changing `CorpusPersistence` struct fields breaks `CorpusPersistence::new`, update `new` accordingly without changing external API contracts.
