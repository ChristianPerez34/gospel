//! Tauri commands for Corpus functionality

use crate::corpus::{
    dto::{NeighborDto, NodeDto},
    extractor::extract_directory,
    persistence::{CorpusManifest, CorpusPersistence},
    Corpus,
};
use crate::AppConfigState;
use serde::Serialize;
use std::path::PathBuf;
use tauri::Emitter;

/// Corpus build progress event
#[derive(Clone, Serialize)]
pub struct CorpusProgress {
    pub phase: String,
    pub message: String,
    pub current: usize,
    pub total: Option<usize>,
}

/// Corpus status response
#[derive(Clone, Serialize)]
pub struct CorpusStatus {
    pub exists: bool,
    pub manifest: Option<CorpusManifest>,
    pub corpus_dir: Option<String>,
}

/// Build a corpus for the active workspace
#[tauri::command]
pub async fn build_corpus(
    app: tauri::AppHandle,
    app_config: tauri::State<'_, AppConfigState>,
    ignore_patterns: Option<String>,
) -> Result<CorpusStatus, String> {
    // Get active workspace
    let workspace = match &app_config.store {
        Some(store) => store
            .get_active_workspace()
            .map_err(|e| format!("Failed to get active workspace: {}", e))?,
        None => return Err("App config store unavailable".to_string()),
    };

    let workspace = workspace.ok_or("No active workspace selected")?;
    let workspace_path = PathBuf::from(workspace.path);

    if !workspace_path.exists() {
        return Err(format!("Workspace path does not exist: {:?}", workspace_path));
    }

    // Parse ignore patterns
    let ignore: Vec<&str> = ignore_patterns
        .as_deref()
        .unwrap_or("target,node_modules,.git,dist,build")
        .split(',')
        .map(|s| s.trim())
        .collect();

    // Emit progress: starting
    let _ = app.emit(
        "corpus-progress",
        CorpusProgress {
            phase: "collecting".to_string(),
            message: "Collecting files...".to_string(),
            current: 0,
            total: None,
        },
    );

    // Build corpus
    let mut corpus = Corpus::new();

    match extract_directory(&mut corpus, &workspace_path, &ignore) {
        Ok(()) => {
            let summary = corpus.summary();

            // Emit progress: saving
            let _ = app.emit(
                "corpus-progress",
                CorpusProgress {
                    phase: "saving".to_string(),
                    message: format!(
                        "Saving {} files and {} symbols...",
                        summary.file_count, summary.symbol_count
                    ),
                    current: summary.file_count + summary.symbol_count,
                    total: None,
                },
            );

            // Save corpus
            let persistence = CorpusPersistence::new(&workspace_path)
                .map_err(|e| format!("Failed to create persistence manager: {}", e))?;

            persistence
                .save(&corpus, &workspace_path)
                .map_err(|e| format!("Failed to save corpus: {}", e))?;

            // Emit progress: complete
            let _ = app.emit(
                "corpus-progress",
                CorpusProgress {
                    phase: "complete".to_string(),
                    message: "Corpus built successfully!".to_string(),
                    current: summary.file_count + summary.symbol_count,
                    total: None,
                },
            );

            // Return status
            Ok(CorpusStatus {
                exists: true,
                manifest: persistence.load_manifest().ok(),
                corpus_dir: persistence.corpus_dir().to_str().map(|s| s.to_string()),
            })
        }
        Err(e) => {
            let _ = app.emit(
                "corpus-progress",
                CorpusProgress {
                    phase: "error".to_string(),
                    message: format!("Extraction failed: {}", e),
                    current: 0,
                    total: None,
                },
            );
            Err(format!("Failed to extract corpus: {}", e))
        }
    }
}

/// Get corpus status for the active workspace
#[tauri::command]
pub fn get_corpus_status(
    app_config: tauri::State<'_, AppConfigState>,
) -> Result<CorpusStatus, String> {
    let workspace = match &app_config.store {
        Some(store) => store
            .get_active_workspace()
            .map_err(|e| format!("Failed to get active workspace: {}", e))?,
        None => return Err("App config store unavailable".to_string()),
    };

    let workspace = workspace.ok_or("No active workspace selected")?;
    let workspace_path = PathBuf::from(workspace.path);
    let persistence = CorpusPersistence::new(&workspace_path)
        .map_err(|e| format!("Failed to create persistence manager: {}", e))?;

    let exists = persistence.exists();
    let manifest = if exists {
        persistence.load_manifest().ok()
    } else {
        None
    };
    let corpus_dir = if exists {
        persistence.corpus_dir().to_str().map(|s| s.to_string())
    } else {
        None
    };

    Ok(CorpusStatus {
        exists,
        manifest,
        corpus_dir,
    })
}

/// Get corpus summary for the active workspace
#[tauri::command]
pub fn get_corpus_summary(
    app_config: tauri::State<'_, AppConfigState>,
) -> Result<CorpusSummaryDto, String> {
    let workspace = match &app_config.store {
        Some(store) => store
            .get_active_workspace()
            .map_err(|e| format!("Failed to get active workspace: {}", e))?,
        None => return Err("App config store unavailable".to_string()),
    };

    let workspace = workspace.ok_or("No active workspace selected")?;
    let workspace_path = PathBuf::from(workspace.path);
    let persistence = CorpusPersistence::new(&workspace_path)
        .map_err(|e| format!("Failed to create persistence manager: {}", e))?;

    if !persistence.exists() {
        return Err("No corpus exists for this workspace".to_string());
    }

    let corpus = persistence
        .load()
        .map_err(|e| format!("Failed to load corpus: {}", e))?;

    let summary = corpus.summary();

    Ok(CorpusSummaryDto {
        file_count: summary.file_count,
        symbol_count: summary.symbol_count,
        concept_count: summary.concept_count,
        relationship_count: summary.relationship_count,
        relationship_counts: summary.relationship_counts,
        top_symbols: summary.top_symbols,
    })
}

/// Query corpus for the active workspace
#[tauri::command]
pub fn query_corpus(
    app_config: tauri::State<'_, AppConfigState>,
    node_id: String,
) -> Result<NodeDto, String> {
    let workspace = match &app_config.store {
        Some(store) => store
            .get_active_workspace()
            .map_err(|e| format!("Failed to get active workspace: {}", e))?,
        None => return Err("App config store unavailable".to_string()),
    };

    let workspace = workspace.ok_or("No active workspace selected")?;
    let workspace_path = PathBuf::from(workspace.path);
    let persistence = CorpusPersistence::new(&workspace_path)
        .map_err(|e| format!("Failed to create persistence manager: {}", e))?;

    if !persistence.exists() {
        return Err("No corpus exists for this workspace".to_string());
    }

    let corpus = persistence
        .load()
        .map_err(|e| format!("Failed to load corpus: {}", e))?;

    let node = corpus
        .get_node(&node_id)
        .ok_or_else(|| format!("Node not found: {}", node_id))?;

    Ok(NodeDto::from_node(node))
}

/// Get neighbors of a node in the corpus
#[tauri::command]
pub fn get_corpus_neighbors(
    app_config: tauri::State<'_, AppConfigState>,
    node_id: String,
    min_confidence: Option<String>,
) -> Result<Vec<NeighborDto>, String> {
    let workspace = match &app_config.store {
        Some(store) => store
            .get_active_workspace()
            .map_err(|e| format!("Failed to get active workspace: {}", e))?,
        None => return Err("App config store unavailable".to_string()),
    };

    let workspace = workspace.ok_or("No active workspace selected")?;
    let workspace_path = PathBuf::from(workspace.path);
    let persistence = CorpusPersistence::new(&workspace_path)
        .map_err(|e| format!("Failed to create persistence manager: {}", e))?;

    if !persistence.exists() {
        return Err("No corpus exists for this workspace".to_string());
    }

    let corpus = persistence
        .load()
        .map_err(|e| format!("Failed to load corpus: {}", e))?;

    let min_conf = match min_confidence.as_deref().map(|s| s.trim().to_lowercase()).as_deref() {
        Some("high") => crate::corpus::Confidence::High,
        Some("medium") => crate::corpus::Confidence::Medium,
        Some("low") => crate::corpus::Confidence::Low,
        Some(v) => return Err(format!("Invalid min_confidence value: '{}'; expected 'high', 'medium', or 'low'", v)),
        None => crate::corpus::Confidence::Low,
    };

    let neighbors = corpus.get_neighbors(&node_id);
    let dtos: Vec<NeighborDto> = neighbors
        .into_iter()
        .filter(|(rel, _)| rel.confidence >= min_conf)
        .map(|(rel, node)| NeighborDto::from_relationship(rel, node, &node_id))
        .collect();

    Ok(dtos)
}

/// DTO for corpus summary
#[derive(Clone, Serialize)]
pub struct CorpusSummaryDto {
    pub file_count: usize,
    pub symbol_count: usize,
    pub concept_count: usize,
    pub relationship_count: usize,
    pub relationship_counts: std::collections::HashMap<String, usize>,
    pub top_symbols: Vec<(String, usize)>,
}

// NodeDto and NeighborDto are defined in dto.rs for shared use
