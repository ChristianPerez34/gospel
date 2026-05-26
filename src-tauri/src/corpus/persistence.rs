//! Corpus persistence - save and load corpus to/from disk

use crate::corpus::{Corpus, NodeId, NodeType};
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
    /// Create a new persistence manager for the given workspace
    pub fn new(workspace_path: &Path) -> Result<Self, PersistenceError> {
        let corpus_dir = workspace_path.join(CORPUS_DIR_NAME);
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
                    corpus.symbol_index
                        .entry(name.clone())
                        .or_insert_with(Vec::new)
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
        let mut node_stmt = conn.prepare("INSERT INTO nodes (id, node_type, node_data) VALUES (?1, ?2, ?3)")?;
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
            rel_stmt.execute(params![rel.from_id, rel.to_id, rel_type, confidence, metadata])?;
        }

        Ok(())
    }

    /// Query corpus using SQL
    pub fn query(&self, sql: &str, _params: &[&str]) -> Result<Vec<QueryResult>, PersistenceError> {
        // For now, just return empty - SQL query interface needs refinement
        // TODO: Implement proper parameterized queries
        Ok(Vec::new())
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
}
