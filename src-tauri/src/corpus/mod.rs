//! Corpus data model - nodes, relationships, and confidence tagging

pub mod commands;
pub mod extractor;
pub mod persistence;
pub mod tools;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Unique identifier for corpus nodes
pub type NodeId = String;

/// Confidence level for relationships
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Serialize, Deserialize)]
pub enum Confidence {
    /// Extracted from explicit syntax (imports, function calls, etc.)
    High,
    /// Inferred from naming patterns or proximity
    Medium,
    /// Speculative or weak association
    Low,
}

impl std::fmt::Display for Confidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Confidence::High => write!(f, "high"),
            Confidence::Medium => write!(f, "medium"),
            Confidence::Low => write!(f, "low"),
        }
    }
}

impl Confidence {
    pub fn as_f32(&self) -> f32 {
        match self {
            Confidence::High => 0.9,
            Confidence::Medium => 0.6,
            Confidence::Low => 0.3,
        }
    }
}

/// Node types in the corpus knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeType {
    /// A source code file
    File {
        path: String,
        language: String,
        line_count: usize,
    },
    /// A code symbol (function, struct, class, etc.)
    Symbol {
        name: String,
        symbol_kind: SymbolKind,
        file_id: NodeId,
        start_line: usize,
        end_line: usize,
        documentation: Option<String>,
    },
    /// A conceptual section from documentation
    Concept {
        name: String,
        source: String,
        summary: String,
        keywords: Vec<String>,
    },
}

/// Kinds of code symbols
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Class,
    Interface,
    TypeAlias,
    Constant,
    Variable,
    Module,
    Trait,
    Impl,
    Enum,
    Variant,
    Field,
}

impl SymbolKind {
    pub fn from_ts_symbol(kind: &str) -> Option<Self> {
        match kind {
            "function" | "arrow_function" => Some(SymbolKind::Function),
            "struct_item" | "struct" => Some(SymbolKind::Struct),
            "class" | "class_declaration" => Some(SymbolKind::Class),
            "interface" | "interface_declaration" => Some(SymbolKind::Interface),
            "type_alias" | "type_alias_declaration" => Some(SymbolKind::TypeAlias),
            "const" | "const_item" => Some(SymbolKind::Constant),
            "let" | "variable" | "variable_declarator" => Some(SymbolKind::Variable),
            "module" | "mod_item" => Some(SymbolKind::Module),
            "trait" | "trait_item" => Some(SymbolKind::Trait),
            "impl" | "impl_item" => Some(SymbolKind::Impl),
            "enum" | "enum_item" => Some(SymbolKind::Enum),
            "enum_variant" | "variant" => Some(SymbolKind::Variant),
            "field" | "field_declaration" => Some(SymbolKind::Field),
            "method" | "method_definition" => Some(SymbolKind::Method),
            _ => None,
        }
    }
}

/// Relationship types between nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "relationship")]
pub enum RelationshipType {
    /// File contains symbol
    Contains,
    /// Symbol imports/uses another symbol
    Uses,
    /// File imports another file
    Imports,
    /// Symbol defines a concept
    Defines,
    /// Documentation explains a symbol
    Explains,
    /// Generic reference (function call, type usage, etc.)
    References,
    /// Symbol extends/implements another
    Extends,
    /// Symbol is a child of another (nested classes, etc.)
    ChildOf,
}

/// A relationship between two nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub from_id: NodeId,
    pub to_id: NodeId,
    pub relationship_type: RelationshipType,
    pub confidence: Confidence,
    /// Optional metadata about the relationship
    pub metadata: HashMap<String, String>,
}

impl Relationship {
    pub fn new(
        from_id: NodeId,
        to_id: NodeId,
        relationship_type: RelationshipType,
        confidence: Confidence,
    ) -> Self {
        Self {
            from_id,
            to_id,
            relationship_type,
            confidence,
            metadata: HashMap::new(),
        }
    }
}

/// A node in the corpus knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub node_type: NodeType,
    /// Metadata that doesn't fit in the node type
    pub metadata: HashMap<String, String>,
}

impl Node {
    pub fn new(node_type: NodeType) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            node_type,
            metadata: HashMap::new(),
        }
    }

    pub fn name(&self) -> String {
        match &self.node_type {
            NodeType::File { path, .. } => path.clone(),
            NodeType::Symbol { name, .. } => name.clone(),
            NodeType::Concept { name, .. } => name.clone(),
        }
    }

    pub fn kind_str(&self) -> &'static str {
        match &self.node_type {
            NodeType::File { .. } => "file",
            NodeType::Symbol { symbol_kind, .. } => match symbol_kind {
                SymbolKind::Function => "function",
                SymbolKind::Method => "method",
                SymbolKind::Struct => "struct",
                SymbolKind::Class => "class",
                SymbolKind::Interface => "interface",
                SymbolKind::TypeAlias => "type_alias",
                SymbolKind::Constant => "constant",
                SymbolKind::Variable => "variable",
                SymbolKind::Module => "module",
                SymbolKind::Trait => "trait",
                SymbolKind::Impl => "impl",
                SymbolKind::Enum => "enum",
                SymbolKind::Variant => "variant",
                SymbolKind::Field => "field",
            },
            NodeType::Concept { .. } => "concept",
        }
    }
}

/// The complete corpus knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Corpus {
    pub nodes: HashMap<NodeId, Node>,
    pub relationships: Vec<Relationship>,
    /// Index: file_path -> node_id
    #[serde(skip)]
    pub file_index: HashMap<String, NodeId>,
    /// Index: symbol_name -> node_id
    #[serde(skip)]
    pub symbol_index: HashMap<String, Vec<NodeId>>,
}

impl Corpus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: Node) -> NodeId {
        let id = node.id.clone();
        
        // Update indexes
        match &node.node_type {
            NodeType::File { path, .. } => {
                self.file_index.insert(path.clone(), id.clone());
            }
            NodeType::Symbol { name, .. } => {
                self.symbol_index
                    .entry(name.clone())
                    .or_insert_with(Vec::new)
                    .push(id.clone());
            }
            NodeType::Concept { .. } => {}
        }

        self.nodes.insert(id.clone(), node);
        id
    }

    pub fn add_relationship(&mut self, relationship: Relationship) {
        self.relationships.push(relationship);
    }

    pub fn get_node(&self, id: &NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn get_file_by_path(&self, path: &str) -> Option<&Node> {
        self.file_index.get(path).and_then(|id| self.nodes.get(id))
    }

    pub fn get_symbols_by_name(&self, name: &str) -> Vec<&Node> {
        self.symbol_index
            .get(name)
            .map(|ids| ids.iter().filter_map(|id| self.nodes.get(id)).collect())
            .unwrap_or_default()
    }

    /// Get all relationships for a node (both incoming and outgoing)
    pub fn get_neighbors(&self, node_id: &NodeId) -> Vec<(&Relationship, &Node)> {
        self.relationships
            .iter()
            .filter_map(|rel| {
                if rel.from_id == *node_id {
                    self.nodes.get(&rel.to_id).map(|node| (rel, node))
                } else if rel.to_id == *node_id {
                    self.nodes.get(&rel.from_id).map(|node| (rel, node))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get outgoing relationships from a node
    pub fn get_outgoing(&self, node_id: &NodeId) -> Vec<(&Relationship, &Node)> {
        self.relationships
            .iter()
            .filter_map(|rel| {
                if rel.from_id == *node_id {
                    self.nodes.get(&rel.to_id).map(|node| (rel, node))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get incoming relationships to a node
    pub fn get_incoming(&self, node_id: &NodeId) -> Vec<(&Relationship, &Node)> {
        self.relationships
            .iter()
            .filter_map(|rel| {
                if rel.to_id == *node_id {
                    self.nodes.get(&rel.from_id).map(|node| (rel, node))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Build a summary of the corpus
    pub fn summary(&self) -> CorpusSummary {
        let file_count = self
            .nodes
            .values()
            .filter(|n| matches!(n.node_type, NodeType::File { .. }))
            .count();
        let symbol_count = self
            .nodes
            .values()
            .filter(|n| matches!(n.node_type, NodeType::Symbol { .. }))
            .count();
        let concept_count = self
            .nodes
            .values()
            .filter(|n| matches!(n.node_type, NodeType::Concept { .. }))
            .count();

        // Count relationships by type
        let mut relationship_counts: HashMap<String, usize> = HashMap::new();
        for rel in &self.relationships {
            let key = format!("{:?}", rel.relationship_type);
            *relationship_counts.entry(key).or_insert(0) += 1;
        }

        // Get top symbols by incoming references
        let mut symbol_refs: HashMap<String, usize> = HashMap::new();
        for rel in &self.relationships {
            if let Some(node) = self.nodes.get(&rel.to_id) {
                if let NodeType::Symbol { name, .. } = &node.node_type {
                    *symbol_refs.entry(name.clone()).or_insert(0) += 1;
                }
            }
        }

        let mut top_symbols: Vec<_> = symbol_refs.into_iter().collect();
        top_symbols.sort_by(|a, b| b.1.cmp(&a.1));
        top_symbols.truncate(10);

        CorpusSummary {
            file_count,
            symbol_count,
            concept_count,
            relationship_count: self.relationships.len(),
            relationship_counts,
            top_symbols,
        }
    }

    /// Filter relationships by minimum confidence
    pub fn filter_by_confidence(&self, min_confidence: Confidence) -> Vec<&Relationship> {
        self.relationships
            .iter()
            .filter(|rel| rel.confidence >= min_confidence)
            .collect()
    }
}

/// Summary statistics for a corpus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusSummary {
    pub file_count: usize,
    pub symbol_count: usize,
    pub concept_count: usize,
    pub relationship_count: usize,
    pub relationship_counts: HashMap<String, usize>,
    pub top_symbols: Vec<(String, usize)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_corpus_with_nodes() {
        let mut corpus = Corpus::new();

        let file_node = Node::new(NodeType::File {
            path: "src/main.rs".to_string(),
            language: "rust".to_string(),
            line_count: 100,
        });
        let file_id = corpus.add_node(file_node);

        let symbol_node = Node::new(NodeType::Symbol {
            name: "main".to_string(),
            symbol_kind: SymbolKind::Function,
            file_id: file_id.clone(),
            start_line: 0,
            end_line: 50,
            documentation: Some("Main entry point".to_string()),
        });
        let symbol_id = corpus.add_node(symbol_node);

        corpus.add_relationship(Relationship::new(
            file_id.clone(),
            symbol_id.clone(),
            RelationshipType::Contains,
            Confidence::High,
        ));

        assert_eq!(corpus.nodes.len(), 2);
        assert_eq!(corpus.relationships.len(), 1);
        assert!(corpus.get_file_by_path("src/main.rs").is_some());
        assert_eq!(corpus.get_symbols_by_name("main").len(), 1);
    }

    #[test]
    fn test_get_neighbors() {
        let mut corpus = Corpus::new();

        let file_node = Node::new(NodeType::File {
            path: "src/lib.rs".to_string(),
            language: "rust".to_string(),
            line_count: 200,
        });
        let file_id = corpus.add_node(file_node);

        let func1 = Node::new(NodeType::Symbol {
            name: "foo".to_string(),
            symbol_kind: SymbolKind::Function,
            file_id: file_id.clone(),
            start_line: 0,
            end_line: 20,
            documentation: None,
        });
        let func1_id = corpus.add_node(func1);

        let func2 = Node::new(NodeType::Symbol {
            name: "bar".to_string(),
            symbol_kind: SymbolKind::Function,
            file_id: file_id.clone(),
            start_line: 21,
            end_line: 40,
            documentation: None,
        });
        let func2_id = corpus.add_node(func2);

        corpus.add_relationship(Relationship::new(
            file_id.clone(),
            func1_id.clone(),
            RelationshipType::Contains,
            Confidence::High,
        ));
        corpus.add_relationship(Relationship::new(
            file_id.clone(),
            func2_id.clone(),
            RelationshipType::Contains,
            Confidence::High,
        ));
        corpus.add_relationship(Relationship::new(
            func1_id.clone(),
            func2_id.clone(),
            RelationshipType::Uses,
            Confidence::Medium,
        ));

        let neighbors = corpus.get_neighbors(&func1_id);
        assert_eq!(neighbors.len(), 2); // file contains func1, func1 uses func2
    }

    #[test]
    fn test_confidence_ordering() {
        assert!(Confidence::High > Confidence::Medium);
        assert!(Confidence::Medium > Confidence::Low);
        assert!(Confidence::High > Confidence::Low);
    }

    #[test]
    fn test_corpus_summary() {
        let mut corpus = Corpus::new();

        for i in 0..3 {
            let file_node = Node::new(NodeType::File {
                path: format!("src/file{}.rs", i),
                language: "rust".to_string(),
                line_count: 100,
            });
            let file_id = corpus.add_node(file_node);

            let symbol_node = Node::new(NodeType::Symbol {
                name: format!("function{}", i),
                symbol_kind: SymbolKind::Function,
                file_id: file_id.clone(),
                start_line: 0,
                end_line: 50,
                documentation: None,
            });
            let symbol_id = corpus.add_node(symbol_node);

            corpus.add_relationship(Relationship::new(
                file_id,
                symbol_id,
                RelationshipType::Contains,
                Confidence::High,
            ));
        }

        let summary = corpus.summary();
        assert_eq!(summary.file_count, 3);
        assert_eq!(summary.symbol_count, 3);
        assert_eq!(summary.relationship_count, 3);
        assert_eq!(summary.top_symbols.len(), 0); // No cross-references yet
    }
}
