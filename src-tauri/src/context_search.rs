//! Context Search - SQLite FTS-based search over corpus content

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContextSearchError {
    #[error("context search IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("context search database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("context search not initialized")]
    NotInitialized,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchChunk {
    pub id: String,
    pub source_type: String,
    pub source_path: String,
    pub chunk_index: usize,
    pub content: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub chunk: SearchChunk,
    pub rank: f64,
}

pub struct ContextSearchIndex {
    conn: Connection,
}

pub const MAX_CONTEXT_SEARCH_LIMIT: usize = 50;

impl ContextSearchIndex {
    pub fn new(workspace_path: &Path) -> Result<Self, ContextSearchError> {
        let index_dir = workspace_path.join(".gospel").join("context_search");
        std::fs::create_dir_all(&index_dir)?;

        let db_path = index_dir.join("search_index.db");
        let conn = Connection::open(db_path)?;

        // Create FTS5 virtual table
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS search_chunks USING fts5(
                id,
                source_type,
                source_path,
                chunk_index,
                content,
                start_line,
                end_line,
                tokenize='porter unicode61'
            );

            CREATE TABLE IF NOT EXISTS search_meta (
                key TEXT PRIMARY KEY,
                value TEXT
            );",
        )?;

        Ok(Self { conn })
    }

    pub fn open_if_exists(workspace_path: &Path) -> Result<Self, ContextSearchError> {
        let db_path = workspace_path
            .join(".gospel")
            .join("context_search")
            .join("search_index.db");
        if !db_path.exists() {
            return Err(ContextSearchError::NotInitialized);
        }

        let conn = Connection::open(db_path)?;
        Ok(Self { conn })
    }

    pub fn clear(&self) -> Result<(), ContextSearchError> {
        self.conn.execute("DELETE FROM search_chunks", [])?;
        Ok(())
    }

    pub fn index_chunks(&mut self, chunks: &[SearchChunk]) -> Result<(), ContextSearchError> {
        let tx = self.conn.transaction()?;

        {
            let mut stmt = tx.prepare(
                "INSERT INTO search_chunks (id, source_type, source_path, chunk_index, content, start_line, end_line)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;

            for chunk in chunks {
                stmt.execute(params![
                    chunk.id,
                    chunk.source_type,
                    chunk.source_path,
                    chunk.chunk_index,
                    chunk.content,
                    chunk.start_line,
                    chunk.end_line,
                ])?;
            }
        }

        tx.execute(
            "INSERT OR REPLACE INTO search_meta (key, value) VALUES ('chunk_count', ?1)",
            params![chunks.len().to_string()],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO search_meta (key, value) VALUES ('last_updated', ?1)",
            params![chrono::Utc::now().to_rfc3339()],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>, ContextSearchError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_type, source_path, chunk_index, content, start_line, end_line,
                    rank
             FROM search_chunks
             WHERE search_chunks MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let results = stmt
            .query_map(params![query, limit as i64], |row| {
                Ok(SearchResult {
                    chunk: SearchChunk {
                        id: row.get(0)?,
                        source_type: row.get(1)?,
                        source_path: row.get(2)?,
                        chunk_index: row.get(3)?,
                        content: row.get(4)?,
                        start_line: row.get(5)?,
                        end_line: row.get(6)?,
                    },
                    rank: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    pub fn get_stats(&self) -> Result<ContextSearchStats, ContextSearchError> {
        let chunk_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM search_chunks", [], |row| row.get(0))?;

        let last_updated: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM search_meta WHERE key = 'last_updated'",
                [],
                |row| row.get(0),
            )
            .ok();

        Ok(ContextSearchStats {
            chunk_count,
            last_updated,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSearchStats {
    pub chunk_count: i64,
    pub last_updated: Option<String>,
}

pub fn chunk_source_file(path: &Path, content: &str) -> Vec<SearchChunk> {
    let mut chunks = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let chunk_size = 50; // lines per chunk
    let overlap = 10; // lines overlap

    if lines.is_empty() {
        return chunks;
    }

    let mut start = 0;
    let mut chunk_index = 0;

    while start < lines.len() {
        let end = std::cmp::min(start + chunk_size, lines.len());
        let chunk_content = lines[start..end].join("\n");

        chunks.push(SearchChunk {
            id: format!("{}:{}", path.display(), chunk_index),
            source_type: "source".to_string(),
            source_path: path.to_string_lossy().to_string(),
            chunk_index,
            content: chunk_content,
            start_line: Some(start + 1),
            end_line: Some(end),
        });

        chunk_index += 1;
        start += chunk_size - overlap;

        if start + overlap >= lines.len() {
            break;
        }
    }

    chunks
}

pub fn chunk_markdown_file(path: &Path, content: &str) -> Vec<SearchChunk> {
    let mut chunks = Vec::new();
    let mut current_section = String::new();
    let mut current_content = String::new();
    let mut chunk_index = 0;
    let mut start_line = 1;

    for (i, line) in content.lines().enumerate() {
        if line.starts_with('#') {
            if !current_content.is_empty() {
                chunks.push(SearchChunk {
                    id: format!("{}:{}", path.display(), chunk_index),
                    source_type: "documentation".to_string(),
                    source_path: path.to_string_lossy().to_string(),
                    chunk_index,
                    content: format!("## {}\n\n{}", current_section, current_content),
                    start_line: Some(start_line),
                    end_line: Some(i),
                });
                chunk_index += 1;
            }
            current_section = line.trim_start_matches('#').trim().to_string();
            current_content.clear();
            start_line = i + 1;
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Add final chunk
    if !current_content.is_empty() {
        chunks.push(SearchChunk {
            id: format!("{}:{}", path.display(), chunk_index),
            source_type: "documentation".to_string(),
            source_path: path.to_string_lossy().to_string(),
            chunk_index,
            content: format!("## {}\n\n{}", current_section, current_content),
            start_line: Some(start_line),
            end_line: Some(content.lines().count()),
        });
    }

    chunks
}

pub fn chunk_text_file(path: &Path, content: &str) -> Vec<SearchChunk> {
    // Use source file chunking for other text files
    chunk_source_file(path, content)
}
