//! Rig tools for Corpus functionality

use crate::corpus::persistence::CorpusPersistence;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CorpusToolError {
    #[error("Corpus not found")]
    CorpusNotFound,
    #[error("Corpus access error: {0}")]
    CorpusAccessError(String),
    #[error("Corpus load error: {0}")]
    CorpusLoadError(String),
    #[error("Node not found: {0}")]
    NodeNotFound(String),
}

/// Arguments for corpus_summary tool
#[derive(Debug, Deserialize)]
pub struct CorpusSummaryArgs {
    /// Optional workspace path (defaults to current directory if not provided)
    workspace_path: Option<String>,
}

/// Corpus summary tool output
#[derive(Debug, Serialize)]
pub struct CorpusSummaryOutput {
    pub exists: bool,
    pub file_count: usize,
    pub symbol_count: usize,
    pub relationship_count: usize,
    pub top_symbols: Vec<(String, usize)>,
    pub message: String,
}

/// Tool to get a summary of the codebase corpus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusSummaryTool;

impl Tool for CorpusSummaryTool {
    const NAME: &'static str = "corpus_summary";

    type Error = CorpusToolError;
    type Args = CorpusSummaryArgs;
    type Output = CorpusSummaryOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "corpus_summary".to_string(),
            description: "Get a summary of the codebase structure including files, symbols, and their relationships. Use this FIRST when asked about the codebase, its structure, or what it contains.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "workspace_path": {
                        "type": "string",
                        "description": "Optional workspace path. If not provided, uses the current directory."
                    }
                },
                "required": []
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let workspace_path = args.workspace_path
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let persistence = CorpusPersistence::new(&workspace_path)
            .map_err(|e| CorpusToolError::CorpusAccessError(e.to_string()))?;

        if !persistence.exists() {
            return Ok(CorpusSummaryOutput {
                exists: false,
                file_count: 0,
                symbol_count: 0,
                relationship_count: 0,
                top_symbols: vec![],
                message: "No corpus exists for this workspace. Use the 'Build Corpus' command to create one.".to_string(),
            });
        }

        let corpus = persistence.load()
            .map_err(|e| CorpusToolError::CorpusLoadError(e.to_string()))?;
        let summary = corpus.summary();
        let top_symbols_clone = summary.top_symbols.clone();

        Ok(CorpusSummaryOutput {
            exists: true,
            file_count: summary.file_count,
            symbol_count: summary.symbol_count,
            relationship_count: summary.relationship_count,
            top_symbols: top_symbols_clone,
            message: format!(
                "Corpus contains {} files, {} symbols, and {} relationships. Top symbols: {}",
                summary.file_count,
                summary.symbol_count,
                summary.relationship_count,
                summary.top_symbols.iter()
                    .take(5)
                    .map(|(name, count)| format!("{} ({} refs)", name, count))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })
    }
}

/// Arguments for corpus_query tool
#[derive(Debug, Deserialize)]
pub struct CorpusQueryArgs {
    /// Node ID or symbol/file name to query
    identifier: String,
    /// Optional workspace path
    workspace_path: Option<String>,
}

/// Node DTO for tool output
#[derive(Debug, Serialize, Clone)]
pub struct NodeDto {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub node_type: String,
    pub metadata: std::collections::HashMap<String, String>,
}

impl NodeDto {
    pub fn from_node(node: &crate::corpus::Node) -> Self {
        use crate::corpus::NodeType;

        let (node_type, mut metadata) = match &node.node_type {
            NodeType::File { path, language, line_count } => {
                let mut m = std::collections::HashMap::new();
                m.insert("path".to_string(), path.clone());
                m.insert("language".to_string(), language.clone());
                m.insert("line_count".to_string(), line_count.to_string());
                ("file".to_string(), m)
            }
            NodeType::Symbol { name, symbol_kind, file_id, start_line, end_line, documentation } => {
                let mut m = std::collections::HashMap::new();
                m.insert("symbol_kind".to_string(), format!("{:?}", symbol_kind));
                m.insert("file_id".to_string(), file_id.clone());
                m.insert("start_line".to_string(), start_line.to_string());
                m.insert("end_line".to_string(), end_line.to_string());
                if let Some(doc) = documentation {
                    m.insert("documentation".to_string(), doc.clone());
                }
                ("symbol".to_string(), m)
            }
            NodeType::Concept { name: _, source, summary, keywords } => {
                let mut m = std::collections::HashMap::new();
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

/// Query result output
#[derive(Debug, Serialize)]
pub struct CorpusQueryOutput {
    pub found: bool,
    pub node: Option<NodeDto>,
    pub neighbor_count: usize,
    pub message: String,
}

/// Tool to query details about a specific code element
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusQueryTool;

impl Tool for CorpusQueryTool {
    const NAME: &'static str = "corpus_query";

    type Error = CorpusToolError;
    type Args = CorpusQueryArgs;
    type Output = CorpusQueryOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "corpus_query".to_string(),
            description: "Query details about a specific file, function, class, or code element by name or ID. Returns metadata and connection count.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "identifier": {
                        "type": "string",
                        "description": "Node ID, symbol name, or file path to query (e.g., 'main', 'DataProcessor', 'src/main.rs')"
                    },
                    "workspace_path": {
                        "type": "string",
                        "description": "Optional workspace path"
                    }
                },
                "required": ["identifier"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let workspace_path: PathBuf = args.workspace_path
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let persistence = CorpusPersistence::new(&workspace_path)
            .map_err(|e| CorpusToolError::CorpusAccessError(e.to_string()))?;

        if !persistence.exists() {
            return Ok(CorpusQueryOutput {
                found: false,
                node: None,
                neighbor_count: 0,
                message: "No corpus exists. Build one first.".to_string(),
            });
        }

        let corpus = persistence.load()
            .map_err(|e| CorpusToolError::CorpusLoadError(e.to_string()))?;

        // Try to find by exact node ID first
        let node = corpus.get_node(&args.identifier);
        
        // If not found by ID, try symbol name
        let node = node.or_else(|| {
            corpus.get_symbols_by_name(&args.identifier).first().copied()
        });

        // If still not found, try file path
        let node = node.or_else(|| {
            corpus.get_file_by_path(&args.identifier)
        });

        match node {
            Some(node) => {
                let neighbors = corpus.get_neighbors(&node.id);
                let node_dto = NodeDto::from_node(node);
                
                Ok(CorpusQueryOutput {
                    found: true,
                    node: Some(node_dto),
                    neighbor_count: neighbors.len(),
                    message: format!("Found '{}' with {} connections", args.identifier, neighbors.len()),
                })
            }
            None => Ok(CorpusQueryOutput {
                found: false,
                node: None,
                neighbor_count: 0,
                message: format!("No element found matching '{}'", args.identifier),
            }),
        }
    }
}

/// Arguments for corpus_neighbors tool
#[derive(Debug, Deserialize)]
pub struct CorpusNeighborsArgs {
    /// Node ID or name to get neighbors for
    identifier: String,
    /// Minimum confidence: "high", "medium", or "low"
    min_confidence: Option<String>,
    /// Optional workspace path
    workspace_path: Option<String>,
}

/// Neighbor relationship DTO
#[derive(Debug, Serialize, Clone)]
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
        rel: &crate::corpus::Relationship,
        node: &crate::corpus::Node,
    ) -> Self {
        Self {
            node_id: node.id.clone(),
            node_name: node.name(),
            node_kind: node.kind_str().to_string(),
            relationship_type: format!("{:?}", rel.relationship_type),
            confidence: rel.confidence.to_string(),
            direction: if rel.from_id == node.id {
                "incoming".to_string()
            } else {
                "outgoing".to_string()
            },
        }
    }
}

/// Tool to get relationships for a code element
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusNeighborsTool;

impl Tool for CorpusNeighborsTool {
    const NAME: &'static str = "corpus_neighbors";

    type Error = CorpusToolError;
    type Args = CorpusNeighborsArgs;
    type Output = Vec<NeighborDto>;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "corpus_neighbors".to_string(),
            description: "Get all relationships for a code element. Shows what it uses, imports, defines, or connects to. Use to understand dependencies.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "identifier": {
                        "type": "string",
                        "description": "Node ID, symbol name, or file path"
                    },
                    "min_confidence": {
                        "type": "string",
                        "enum": ["high", "medium", "low"],
                        "description": "Minimum confidence (default: low)"
                    },
                    "workspace_path": {
                        "type": "string",
                        "description": "Optional workspace path"
                    }
                },
                "required": ["identifier"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let workspace_path: PathBuf = args.workspace_path
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let persistence = CorpusPersistence::new(&workspace_path)
            .map_err(|e| CorpusToolError::CorpusAccessError(e.to_string()))?;

        if !persistence.exists() {
            return Ok(vec![]);
        }

        let corpus = persistence.load()
            .map_err(|e| CorpusToolError::CorpusLoadError(e.to_string()))?;

        let min_conf = match args.min_confidence.as_deref() {
            Some("high") => crate::corpus::Confidence::High,
            Some("medium") => crate::corpus::Confidence::Medium,
            _ => crate::corpus::Confidence::Low,
        };

        // Find node
        let node = corpus.get_node(&args.identifier)
            .or_else(|| corpus.get_symbols_by_name(&args.identifier).first().copied())
            .or_else(|| corpus.get_file_by_path(&args.identifier));

        match node {
            Some(node) => {
                let neighbors = corpus.get_neighbors(&node.id);
                let dtos: Vec<NeighborDto> = neighbors
                    .into_iter()
                    .filter(|(rel, _)| rel.confidence >= min_conf)
                    .map(|(rel, node)| NeighborDto::from_relationship(rel, node))
                    .collect();
                Ok(dtos)
            }
            None => Ok(vec![]),
        }
    }
}

/// Helper to add corpus tools to an agent
/// Usage: 
/// ```rust
/// let agent = client.agent(model)
///     .preamble("Your preamble")
///     .with_corpus_tools()  // Adds all corpus tools
///     .build();
/// ```
pub trait WithCorpusTools<M, P> {
    fn with_corpus_tools(self) -> Self;
}

// Note: Due to rig's type system, tools must be added individually:
// .tool(CorpusSummaryTool).tool(CorpusQueryTool).tool(CorpusNeighborsTool)

/// Create just the summary tool (for lightweight usage)
pub fn create_corpus_summary_tool() -> CorpusSummaryTool {
    CorpusSummaryTool
}

/// Create the query tool
pub fn create_corpus_query_tool() -> CorpusQueryTool {
    CorpusQueryTool
}

/// Create the neighbors tool
pub fn create_corpus_neighbors_tool() -> CorpusNeighborsTool {
    CorpusNeighborsTool
}

/// System prompt to add when corpus tools are available
pub const CORPUS_SYSTEM_PROMPT: &str = r#"

## Codebase Knowledge

You have access to a **corpus** - a structured knowledge graph of the workspace's code with files, symbols, and their relationships.

### Available Tools

1. **corpus_summary** - Get codebase overview (files, symbols, relationships, top symbols)
2. **corpus_query** - Get details about a specific element by name or ID
3. **corpus_neighbors** - Get all relationships for an element (dependencies, connections)

### When to Use Corpus Tools

**ALWAYS use corpus tools when:**
- Asked "what is this codebase about?" or "how is this structured?"
- Asked about architecture, components, or organization
- Asked "where is X defined?" or "what does X do?"
- Asked about relationships between components
- Starting work in an unfamiliar codebase

**Workflow:**
1. Start with `corpus_summary` for orientation
2. Use `corpus_query` to look up specific elements
3. Use `corpus_neighbors` to understand dependencies

**Important:**
- Mention when you're using the corpus
- If corpus doesn't exist, guide user to build it via "Build Corpus" command
- Corpus provides structural knowledge; use file reads for detailed code content

"#;
