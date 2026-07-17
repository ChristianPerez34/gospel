//! Corpus persistence - save and load corpus to/from disk

use crate::corpus::dto::{NeighborDto, NodeDto};
use crate::corpus::{Confidence, Corpus, CorpusSummary, Node, NodeId, NodeType};
use rusqlite::{params, Connection};
use serde_json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Standard corpus directory layout
pub const CORPUS_DIR_NAME: &str = ".gospel/corpus";
pub const GRAPH_JSON_FILE: &str = "graph.json";
pub const SQLITE_DB_FILE: &str = "corpus.db";
pub const MANIFEST_FILE: &str = "manifest.json";

/// Corpus manifest with metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CorpusManifest {
    pub version: String,
    pub created_at: String,
    pub updated_at: String,
    pub workspace_path: String,
    pub node_count: usize,
    pub relationship_count: usize,
}

impl CorpusManifest {
    pub fn new(workspace_path: &str, node_count: usize, relationship_count: usize) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            version: "1.0.0".to_string(),
            created_at: now.clone(),
            updated_at: now,
            workspace_path: workspace_path.to_string(),
            node_count,
            relationship_count,
        }
    }
}

/// Persistence manager for corpus data
pub struct CorpusPersistence {
    corpus_dir: PathBuf,
}

impl CorpusPersistence {
    /// Create a new persistence manager for the given workspace.
    ///
    /// Rejects storage paths that resolve outside the canonical workspace root,
    /// including an attacker-planted symlink at `.gospel`.
    pub fn new(workspace_path: &Path) -> Result<Self, PersistenceError> {
        let workspace_canonical = std::fs::canonicalize(workspace_path)?;
        let gospel_dir = workspace_path.join(".gospel");
        if let Some(canonical_gospel) = crate::corpus::symlink_guard::canonical(&gospel_dir) {
            if !crate::corpus::symlink_guard::is_within(&workspace_canonical, &canonical_gospel) {
                return Err(PersistenceError::IoError(std::io::Error::other(format!(
                    "Corpus parent directory {} escapes the workspace",
                    canonical_gospel.display()
                ))));
            }
        }

        let corpus_dir = workspace_path.join(CORPUS_DIR_NAME);
        if let Some(canonical_corpus) = crate::corpus::symlink_guard::canonical(&corpus_dir) {
            if !crate::corpus::symlink_guard::is_within(&workspace_canonical, &canonical_corpus) {
                return Err(PersistenceError::IoError(std::io::Error::other(format!(
                    "Corpus directory {} escapes the workspace",
                    canonical_corpus.display()
                ))));
            }
        }

        Ok(Self { corpus_dir })
    }

    /// Save corpus to disk
    pub fn save(&self, corpus: &Corpus, workspace_path: &Path) -> Result<(), PersistenceError> {
        // Create corpus directory
        std::fs::create_dir_all(&self.corpus_dir)?;

        // Save graph as JSON
        let graph_path = self.corpus_dir.join(GRAPH_JSON_FILE);
        let json = serde_json::to_string_pretty(corpus)?;
        std::fs::write(&graph_path, json)?;

        // Create/update SQLite database for queries
        self.create_database(corpus)?;

        // Save manifest
        let manifest = CorpusManifest::new(
            &workspace_path.to_string_lossy(),
            corpus.nodes.len(),
            corpus.relationships.len(),
        );
        let manifest_path = self.corpus_dir.join(MANIFEST_FILE);
        let manifest_json = serde_json::to_string_pretty(&manifest)?;
        std::fs::write(&manifest_path, manifest_json)?;

        Ok(())
    }

    /// Load corpus from disk
    pub fn load(&self) -> Result<Corpus, PersistenceError> {
        let graph_path = self.corpus_dir.join(GRAPH_JSON_FILE);
        if !graph_path.exists() {
            return Err(PersistenceError::NotFound(
                "Corpus graph.json not found".to_string(),
            ));
        }

        let json = std::fs::read_to_string(&graph_path)?;
        let mut corpus: Corpus = serde_json::from_str(&json)?;

        // Rebuild indexes
        corpus.file_index = HashMap::new();
        corpus.symbol_index = HashMap::new();

        for (id, node) in &corpus.nodes {
            match &node.node_type {
                NodeType::File { path, .. } => {
                    corpus.file_index.insert(path.clone(), id.clone());
                }
                NodeType::Symbol { name, .. } => {
                    corpus
                        .symbol_index
                        .entry(name.clone())
                        .or_default()
                        .push(id.clone());
                }
                NodeType::Concept { .. } => {}
            }
        }

        Ok(corpus)
    }

    /// Check if corpus exists
    pub fn exists(&self) -> bool {
        self.corpus_dir.join(GRAPH_JSON_FILE).exists()
    }

    /// Get corpus directory path
    pub fn corpus_dir(&self) -> &Path {
        &self.corpus_dir
    }

    /// Load manifest
    pub fn load_manifest(&self) -> Result<CorpusManifest, PersistenceError> {
        let manifest_path = self.corpus_dir.join(MANIFEST_FILE);
        if !manifest_path.exists() {
            return Err(PersistenceError::NotFound(
                "Corpus manifest not found".to_string(),
            ));
        }

        let json = std::fs::read_to_string(&manifest_path)?;
        let manifest: CorpusManifest = serde_json::from_str(&json)?;
        Ok(manifest)
    }

    fn create_database(&self, corpus: &Corpus) -> Result<(), PersistenceError> {
        let db_path = self.corpus_dir.join(SQLITE_DB_FILE);
        let conn = Connection::open(&db_path)?;

        // Create tables
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS nodes (
                id TEXT PRIMARY KEY,
                node_type TEXT NOT NULL,
                node_data TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS relationships (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                relationship_type TEXT NOT NULL,
                confidence TEXT NOT NULL,
                metadata TEXT,
                FOREIGN KEY (from_id) REFERENCES nodes(id),
                FOREIGN KEY (to_id) REFERENCES nodes(id)
            );
            CREATE INDEX IF NOT EXISTS idx_relationships_from ON relationships(from_id);
            CREATE INDEX IF NOT EXISTS idx_relationships_to ON relationships(to_id);
            CREATE INDEX IF NOT EXISTS idx_nodes_type ON nodes(node_type);",
        )?;

        // Clear existing data
        conn.execute("DELETE FROM relationships", [])?;
        conn.execute("DELETE FROM nodes", [])?;

        // Insert nodes
        let mut node_stmt =
            conn.prepare("INSERT INTO nodes (id, node_type, node_data) VALUES (?1, ?2, ?3)")?;
        for (id, node) in &corpus.nodes {
            let node_type = match &node.node_type {
                NodeType::File { .. } => "file",
                NodeType::Symbol { .. } => "symbol",
                NodeType::Concept { .. } => "concept",
            };
            let node_data = serde_json::to_string(&node.node_type)?;
            node_stmt.execute(params![id, node_type, node_data])?;
        }

        // Insert relationships
        let mut rel_stmt = conn.prepare(
            "INSERT INTO relationships (from_id, to_id, relationship_type, confidence, metadata) 
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for rel in &corpus.relationships {
            let rel_type = format!("{:?}", rel.relationship_type);
            let confidence = format!("{:?}", rel.confidence);
            let metadata = if rel.metadata.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&rel.metadata)?)
            };
            rel_stmt.execute(params![
                rel.from_id,
                rel.to_id,
                rel_type,
                confidence,
                metadata
            ])?;
        }

        Ok(())
    }

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
        let rows = stmt.query_map(params![pattern], |row| {
            let id: String = row.get(0)?;
            let data: String = row.get(1)?;
            let (name, kind) = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                let name = v
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or(&id)
                    .to_string();
                let kind = v
                    .get("symbol_kind")
                    .or(v.get("type"))
                    .and_then(|k| k.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                (name, kind)
            } else {
                (id.clone(), "unknown".to_string())
            };
            Ok(QueryResult { id, name, kind })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get neighbors of a node using SQL
    pub fn get_neighbors(&self, node_id: &NodeId) -> Result<Vec<NeighborResult>, PersistenceError> {
        let db_path = self.corpus_dir.join(SQLITE_DB_FILE);
        let conn = Connection::open(&db_path)?;

        let sql = r#"
            SELECT 
                CASE WHEN r.from_id = ?1 THEN r.to_id ELSE r.from_id END as neighbor_id,
                n.node_data,
                r.relationship_type,
                r.confidence
            FROM relationships r
            JOIN nodes n ON (CASE WHEN r.from_id = ?1 THEN r.to_id ELSE r.from_id END) = n.id
            WHERE r.from_id = ?1 OR r.to_id = ?1
        "#;

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![node_id], |row| {
            Ok(NeighborResult {
                neighbor_id: row.get(0)?,
                node_data: row.get(1)?,
                relationship_type: row.get(2)?,
                confidence: row.get(3)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }

        Ok(results)
    }

    /// Reconstruct a Node from a database row's node_data JSON
    fn node_from_row(id: String, node_data: String) -> Result<Node, PersistenceError> {
        let node_type: NodeType = serde_json::from_str(&node_data)?;
        Ok(Node {
            id,
            node_type,
            metadata: HashMap::new(),
        })
    }

    /// Get corpus summary using SQLite
    pub fn summary_sqlite(&self) -> Result<CorpusSummary, PersistenceError> {
        let db_path = self.corpus_dir.join(SQLITE_DB_FILE);
        let conn = Connection::open(&db_path)?;

        let mut file_count = 0usize;
        let mut symbol_count = 0usize;
        let mut concept_count = 0usize;

        let mut stmt = conn.prepare("SELECT node_type, COUNT(*) FROM nodes GROUP BY node_type")?;
        let rows = stmt.query_map([], |row| {
            let nt: String = row.get(0)?;
            let cnt: i64 = row.get(1)?;
            Ok((nt, cnt as usize))
        })?;
        for row in rows {
            let (nt, cnt) = row?;
            match nt.as_str() {
                "file" => file_count = cnt,
                "symbol" => symbol_count = cnt,
                "concept" => concept_count = cnt,
                _ => {}
            }
        }

        let relationship_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM relationships", [], |row| row.get(0))?;

        let mut stmt = conn.prepare(
            "SELECT relationship_type, COUNT(*) FROM relationships GROUP BY relationship_type",
        )?;
        let rows = stmt.query_map([], |row| {
            let rt: String = row.get(0)?;
            let cnt: i64 = row.get(1)?;
            Ok((rt, cnt as usize))
        })?;
        let mut relationship_counts = HashMap::new();
        for row in rows {
            let (rt, cnt) = row?;
            relationship_counts.insert(rt, cnt);
        }

        let mut top_symbols = Vec::new();
        let sql = r#"
            SELECT n.node_data, COUNT(*) as ref_count
            FROM relationships r
            JOIN nodes n ON r.to_id = n.id
            WHERE r.relationship_type != 'Contains' AND n.node_type = 'symbol'
            GROUP BY n.id
            ORDER BY ref_count DESC
            LIMIT 10
        "#;
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| {
            let data: String = row.get(0)?;
            let cnt: i64 = row.get(1)?;
            Ok((data, cnt as usize))
        })?;
        for row in rows {
            let (data, cnt) = row?;
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(name) = v.get("name").and_then(|n| n.as_str()) {
                    top_symbols.push((name.to_string(), cnt));
                }
            }
        }

        Ok(CorpusSummary {
            file_count,
            symbol_count,
            concept_count,
            relationship_count: relationship_count as usize,
            relationship_counts,
            top_symbols,
        })
    }

    /// Find a node by ID using SQLite, return as NodeDto
    pub fn get_node_dto(&self, id: &str) -> Result<Option<NodeDto>, PersistenceError> {
        let db_path = self.corpus_dir.join(SQLITE_DB_FILE);
        let conn = Connection::open(&db_path)?;
        let mut stmt = conn.prepare("SELECT id, node_data FROM nodes WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id], |row| {
            let node_id: String = row.get(0)?;
            let data: String = row.get(1)?;
            Ok((node_id, data))
        })?;
        match rows.next() {
            Some(Ok((node_id, data))) => {
                let node = Self::node_from_row(node_id, data)?;
                Ok(Some(NodeDto::from_node(&node)))
            }
            _ => Ok(None),
        }
    }

    /// Find symbols by exact name using SQLite
    pub fn get_symbols_by_name_dto(&self, name: &str) -> Result<Vec<NodeDto>, PersistenceError> {
        let db_path = self.corpus_dir.join(SQLITE_DB_FILE);
        let conn = Connection::open(&db_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, node_data FROM nodes WHERE node_type = 'symbol' AND json_extract(node_data, '$.name') = ?1",
        )?;
        let rows = stmt.query_map(params![name], |row| {
            let node_id: String = row.get(0)?;
            let data: String = row.get(1)?;
            Ok((node_id, data))
        })?;
        let mut results = Vec::new();
        for row in rows {
            let (node_id, data) = row?;
            let node = Self::node_from_row(node_id, data)?;
            results.push(NodeDto::from_node(&node));
        }
        Ok(results)
    }

    /// Find a file by path using SQLite
    pub fn get_file_by_path_dto(&self, path: &str) -> Result<Option<NodeDto>, PersistenceError> {
        let db_path = self.corpus_dir.join(SQLITE_DB_FILE);
        let conn = Connection::open(&db_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, node_data FROM nodes WHERE node_type = 'file' AND json_extract(node_data, '$.path') = ?1",
        )?;
        let mut rows = stmt.query_map(params![path], |row| {
            let node_id: String = row.get(0)?;
            let data: String = row.get(1)?;
            Ok((node_id, data))
        })?;
        match rows.next() {
            Some(Ok((node_id, data))) => {
                let node = Self::node_from_row(node_id, data)?;
                Ok(Some(NodeDto::from_node(&node)))
            }
            _ => Ok(None),
        }
    }

    /// Resolve a node by ID, symbol name, or file path using SQLite
    pub fn resolve_node_dto(&self, identifier: &str) -> Result<Option<NodeDto>, PersistenceError> {
        let db_path = self.corpus_dir.join(SQLITE_DB_FILE);
        if !db_path.exists() {
            return Ok(None);
        }
        if let Some(dto) = self.get_node_dto(identifier)? {
            return Ok(Some(dto));
        }
        let symbols = self.get_symbols_by_name_dto(identifier)?;
        if let Some(dto) = symbols.into_iter().next() {
            return Ok(Some(dto));
        }
        self.get_file_by_path_dto(identifier)
    }

    /// Count neighbors of a node using SQLite
    pub fn count_neighbors(&self, node_id: &str) -> Result<usize, PersistenceError> {
        let db_path = self.corpus_dir.join(SQLITE_DB_FILE);
        let conn = Connection::open(&db_path)?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM relationships WHERE from_id = ?1 OR to_id = ?1",
            params![node_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Get neighbor DTOs from SQLite, optionally filtered by minimum confidence
    pub fn get_neighbor_dtos(
        &self,
        node_id: &str,
        min_confidence: Option<Confidence>,
    ) -> Result<Vec<NeighborDto>, PersistenceError> {
        let db_path = self.corpus_dir.join(SQLITE_DB_FILE);
        let conn = Connection::open(&db_path)?;

        let confidence_clause = match min_confidence {
            None | Some(Confidence::Low) => String::new(),
            Some(Confidence::Medium) => "AND r.confidence IN ('High', 'Medium')".to_string(),
            Some(Confidence::High) => "AND r.confidence IN ('High')".to_string(),
        };

        let sql = format!(
            r#"
            SELECT 
                CASE WHEN r.from_id = ?1 THEN r.to_id ELSE r.from_id END as neighbor_id,
                n.node_data,
                r.from_id,
                r.relationship_type,
                r.confidence
            FROM relationships r
            JOIN nodes n ON (CASE WHEN r.from_id = ?1 THEN r.to_id ELSE r.from_id END) = n.id
            WHERE (r.from_id = ?1 OR r.to_id = ?1)
            {}
            "#,
            confidence_clause
        );

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![node_id], |row| {
            let neighbor_id: String = row.get(0)?;
            let node_data: String = row.get(1)?;
            let from_id: String = row.get(2)?;
            let rel_type: String = row.get(3)?;
            let confidence: String = row.get(4)?;
            Ok((neighbor_id, node_data, from_id, rel_type, confidence))
        })?;

        let mut results = Vec::new();
        for row in rows {
            let (neighbor_id, node_data, from_id, rel_type, confidence) = row?;
            let node = Self::node_from_row(neighbor_id, node_data)?;
            let direction = if from_id == node_id {
                "outgoing"
            } else {
                "incoming"
            };
            let node_name = node.name();
            let node_kind = node.kind_str().to_string();
            results.push(NeighborDto {
                node_id: node.id,
                node_name,
                node_kind,
                relationship_type: rel_type,
                confidence: confidence.to_lowercase(),
                direction: direction.to_string(),
            });
        }

        Ok(results)
    }
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub id: NodeId,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct NeighborResult {
    pub neighbor_id: NodeId,
    pub node_data: String,
    pub relationship_type: String,
    pub confidence: String,
}

#[derive(Error, Debug)]
pub enum PersistenceError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("Database error: {0}")]
    DatabaseError(#[from] rusqlite::Error),
    #[error("Not found: {0}")]
    NotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{Confidence, Node, Relationship, RelationshipType};
    use tempfile::TempDir;

    fn create_test_corpus() -> Corpus {
        let mut corpus = Corpus::new();

        let file_node = Node::new(NodeType::File {
            path: "src/main.rs".to_string(),
            language: "rust".to_string(),
            line_count: 100,
        });
        let file_id = corpus.add_node(file_node);

        let symbol_node = Node::new(NodeType::Symbol {
            name: "main".to_string(),
            symbol_kind: crate::corpus::SymbolKind::Function,
            file_id: file_id.clone(),
            start_line: 0,
            end_line: 50,
            documentation: Some("Main function".to_string()),
        });
        let symbol_id = corpus.add_node(symbol_node);

        corpus.add_relationship(Relationship::new(
            file_id,
            symbol_id,
            RelationshipType::Contains,
            Confidence::High,
        ));

        corpus
    }

    #[test]
    fn test_save_and_load_corpus() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_path = temp_dir.path();

        let persistence = CorpusPersistence::new(workspace_path).unwrap();
        let corpus = create_test_corpus();

        // Save
        let result = persistence.save(&corpus, workspace_path);
        assert!(result.is_ok());

        // Check files exist
        assert!(persistence.corpus_dir().exists());
        assert!(persistence.corpus_dir().join(GRAPH_JSON_FILE).exists());
        assert!(persistence.corpus_dir().join(SQLITE_DB_FILE).exists());
        assert!(persistence.corpus_dir().join(MANIFEST_FILE).exists());

        // Load
        let loaded = persistence.load().unwrap();
        assert_eq!(loaded.nodes.len(), corpus.nodes.len());
        assert_eq!(loaded.relationships.len(), corpus.relationships.len());
    }

    #[test]
    fn test_corpus_exists() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_path = temp_dir.path();

        let persistence = CorpusPersistence::new(workspace_path).unwrap();
        assert!(!persistence.exists());

        let corpus = create_test_corpus();
        persistence.save(&corpus, workspace_path).unwrap();
        assert!(persistence.exists());
    }

    #[test]
    fn test_load_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_path = temp_dir.path();

        let persistence = CorpusPersistence::new(workspace_path).unwrap();
        let corpus = create_test_corpus();
        persistence.save(&corpus, workspace_path).unwrap();

        let manifest = persistence.load_manifest().unwrap();
        assert_eq!(manifest.node_count, 2);
        assert_eq!(manifest.relationship_count, 1);
        assert_eq!(manifest.version, "1.0.0");
    }

    #[cfg(unix)]
    #[test]
    fn corpus_persistence_rejects_symlinked_gospel_dir() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("corpus");
        std::fs::create_dir_all(&target).unwrap();

        let gospel = dir.path().join(".gospel");
        symlink(&target, &gospel).unwrap();

        let result = CorpusPersistence::new(dir.path());
        assert!(result.is_err(), "symlinked .gospel should be rejected");
    }
}
