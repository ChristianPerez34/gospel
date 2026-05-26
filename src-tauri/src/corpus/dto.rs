//! Shared DTOs for corpus serialization (used by both Tauri commands and rig tools)

use crate::corpus;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct NodeDto {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub node_type: String,
    pub metadata: HashMap<String, String>,
}

impl NodeDto {
    pub fn from_node(node: &corpus::Node) -> Self {
        use corpus::NodeType;

        let (node_type, mut metadata) = match &node.node_type {
            NodeType::File {
                path,
                language,
                line_count,
            } => {
                let mut m = HashMap::new();
                m.insert("path".to_string(), path.clone());
                m.insert("language".to_string(), language.clone());
                m.insert("line_count".to_string(), line_count.to_string());
                ("file".to_string(), m)
            }
            NodeType::Symbol {
                name: _,
                symbol_kind,
                file_id,
                start_line,
                end_line,
                documentation,
            } => {
                let mut m = HashMap::new();
                m.insert("symbol_kind".to_string(), format!("{:?}", symbol_kind));
                m.insert("file_id".to_string(), file_id.clone());
                m.insert("start_line".to_string(), start_line.to_string());
                m.insert("end_line".to_string(), end_line.to_string());
                if let Some(doc) = documentation {
                    m.insert("documentation".to_string(), doc.clone());
                }
                ("symbol".to_string(), m)
            }
            NodeType::Concept {
                name: _,
                source,
                summary,
                keywords,
            } => {
                let mut m = HashMap::new();
                m.insert("source".to_string(), source.clone());
                m.insert("summary".to_string(), summary.clone());
                m.insert("keywords".to_string(), keywords.join(", "));
                ("concept".to_string(), m)
            }
        };

        for (k, v) in &node.metadata {
            metadata.insert(k.clone(), v.clone());
        }

        Self {
            id: node.id.clone(),
            name: node.name(),
            kind: node.kind_str().to_string(),
            node_type,
            metadata,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NeighborDto {
    pub node_id: String,
    pub node_name: String,
    pub node_kind: String,
    pub relationship_type: String,
    pub confidence: String,
    pub direction: String,
}

impl NeighborDto {
    pub fn from_relationship(
        rel: &corpus::Relationship,
        node: &corpus::Node,
        queried_node_id: &str,
    ) -> Self {
        Self {
            node_id: node.id.clone(),
            node_name: node.name(),
            node_kind: node.kind_str().to_string(),
            relationship_type: format!("{:?}", rel.relationship_type),
            confidence: rel.confidence.to_string(),
            direction: if rel.from_id == queried_node_id {
                "outgoing".to_string()
            } else {
                "incoming".to_string()
            },
        }
    }
}
