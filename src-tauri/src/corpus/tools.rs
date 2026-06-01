//! Rig tools for Corpus functionality

use crate::corpus::dto::{NeighborDto, NodeDto};
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
    #[error("Invalid min_confidence value: '{0}'; expected 'high', 'medium', or 'low'")]
    InvalidConfidence(String),
}

/// Arguments for corpus_summary tool
#[derive(Debug, Deserialize)]
pub struct CorpusSummaryArgs {}

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
pub struct CorpusSummaryTool {
    pub workspace_path: PathBuf,
}

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
                "properties": {},
                "required": []
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _ = args;
        let workspace_path = self.workspace_path.clone();

        let persistence = CorpusPersistence::new(&workspace_path)
            .map_err(|e| CorpusToolError::CorpusAccessError(e.to_string()))?;

        if !persistence.exists() {
            tracing::debug!(
                "[CORPUS-AUTO] corpus_summary missing corpus at {}",
                workspace_path.display()
            );
            return Ok(CorpusSummaryOutput {
                exists: false,
                file_count: 0,
                symbol_count: 0,
                relationship_count: 0,
                top_symbols: vec![],
                message: format!(
                    "No corpus exists at {:?}. Use the 'Build Corpus' command to create one.",
                    workspace_path
                ),
            });
        }

        let summary = persistence
            .summary_sqlite()
            .map_err(|e| CorpusToolError::CorpusLoadError(e.to_string()))?;
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
                summary
                    .top_symbols
                    .iter()
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
pub struct CorpusQueryTool {
    pub workspace_path: PathBuf,
}

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
                    }
                },
                "required": ["identifier"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let workspace_path = self.workspace_path.clone();

        let persistence = CorpusPersistence::new(&workspace_path)
            .map_err(|e| CorpusToolError::CorpusAccessError(e.to_string()))?;

        if !persistence.exists() {
            tracing::debug!(
                "[CORPUS-AUTO] corpus_query missing corpus at {}",
                workspace_path.display()
            );
            return Ok(CorpusQueryOutput {
                found: false,
                node: None,
                neighbor_count: 0,
                message: format!("No corpus exists at {:?}. Build one first.", workspace_path),
            });
        }

        let (node_dto, neighbor_count) = match persistence
            .resolve_node_dto(&args.identifier)
            .map_err(|e| CorpusToolError::CorpusLoadError(e.to_string()))?
        {
            Some(dto) => {
                let count = persistence
                    .count_neighbors(&dto.id)
                    .map_err(|e| CorpusToolError::CorpusLoadError(e.to_string()))?;
                (Some(dto), count)
            }
            None => (None, 0),
        };

        match node_dto {
            Some(node_dto) => Ok(CorpusQueryOutput {
                found: true,
                node: Some(node_dto),
                neighbor_count,
                message: format!(
                    "Found '{}' with {} connections",
                    args.identifier, neighbor_count
                ),
            }),
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
}

/// Tool to get relationships for a code element
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusNeighborsTool {
    pub workspace_path: PathBuf,
}

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
                    }
                },
                "required": ["identifier"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let workspace_path = self.workspace_path.clone();

        let persistence = CorpusPersistence::new(&workspace_path)
            .map_err(|e| CorpusToolError::CorpusAccessError(e.to_string()))?;

        if !persistence.exists() {
            tracing::debug!(
                "[CORPUS-AUTO] corpus_neighbors missing corpus at {}",
                workspace_path.display()
            );
            return Ok(vec![]);
        }

        let min_conf = match args
            .min_confidence
            .as_deref()
            .map(|s| s.trim().to_lowercase())
            .as_deref()
        {
            Some("high") => crate::corpus::Confidence::High,
            Some("medium") => crate::corpus::Confidence::Medium,
            Some("low") => crate::corpus::Confidence::Low,
            Some(v) => return Err(CorpusToolError::InvalidConfidence(v.to_string())),
            None => crate::corpus::Confidence::Low,
        };

        let node_id = match persistence
            .resolve_node_dto(&args.identifier)
            .map_err(|e| CorpusToolError::CorpusLoadError(e.to_string()))?
        {
            Some(dto) => dto.id,
            None => return Ok(vec![]),
        };

        let dtos = persistence
            .get_neighbor_dtos(&node_id, Some(min_conf))
            .map_err(|e| CorpusToolError::CorpusLoadError(e.to_string()))?;

        Ok(dtos)
    }
}

pub fn create_corpus_summary_tool(workspace_path: PathBuf) -> CorpusSummaryTool {
    CorpusSummaryTool { workspace_path }
}

pub fn create_corpus_query_tool(workspace_path: PathBuf) -> CorpusQueryTool {
    CorpusQueryTool { workspace_path }
}

pub fn create_corpus_neighbors_tool(workspace_path: PathBuf) -> CorpusNeighborsTool {
    CorpusNeighborsTool { workspace_path }
}

/// System prompt to add when corpus tools are available
pub const CORPUS_SYSTEM_PROMPT: &str = r#"

## Codebase Knowledge

You have access to a **corpus** - a structured knowledge graph of the active workspace with files, symbols, and their relationships.

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
- Corpus provides structural knowledge for the active workspace; use live file reads for source-of-truth code content

"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_prompt_mentions_live_reads_as_source_of_truth() {
        assert!(CORPUS_SYSTEM_PROMPT.contains("source-of-truth"));
    }
}
