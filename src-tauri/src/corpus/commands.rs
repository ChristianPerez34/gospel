//! Tauri commands for Corpus functionality

use crate::corpus::{
    dto::{NeighborDto, NodeDto},
    extractor::extract_directory,
    persistence::{CorpusManifest, CorpusPersistence},
    Corpus,
};
use crate::{AppConfigState, CORPUS_BUILD_LOCK};
use serde::Serialize;
use std::path::{Path, PathBuf};
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

    run_corpus_build(&app, &workspace_path, ignore_patterns).await
}

/// Build a corpus for a workspace path.
pub async fn run_corpus_build(
    app: &tauri::AppHandle,
    workspace_path: &Path,
    ignore_patterns: Option<String>,
) -> Result<CorpusStatus, String> {
    tracing::debug!(
        "[CORPUS-AUTO] run_corpus_build called for {}",
        workspace_path.display()
    );
    if !workspace_path.exists() {
        return Err(format!(
            "Workspace path does not exist: {:?}",
            workspace_path
        ));
    }

    let _guard = CORPUS_BUILD_LOCK.lock().await;
    run_corpus_build_inner(app, workspace_path, ignore_patterns).await
}

/// Core corpus build implementation. Caller MUST hold CORPUS_BUILD_LOCK
/// (this is not reentrant). Used by both `run_corpus_build` (forced builds)
/// and `ensure_workspace_corpus` (conditional builds).
pub(crate) async fn run_corpus_build_inner(
    app: &tauri::AppHandle,
    workspace_path: &Path,
    ignore_patterns: Option<String>,
) -> Result<CorpusStatus, String> {
    // Parse ignore patterns
    let ignore: Vec<&str> = ignore_patterns
        .as_deref()
        .unwrap_or("target,node_modules,.git,dist,build")
        .split(',')
        .map(|s| s.trim())
        .collect();
    tracing::debug!(
        "[CORPUS-AUTO] run_corpus_build ignore patterns: {}",
        ignore.join(",")
    );

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

    match extract_directory(&mut corpus, workspace_path, &ignore) {
        Ok(()) => {
            let summary = corpus.summary();
            tracing::debug!(
                "[CORPUS-AUTO] extraction complete for {}: {} files, {} symbols",
                workspace_path.display(),
                summary.file_count,
                summary.symbol_count
            );

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
            let persistence = CorpusPersistence::new(workspace_path)
                .map_err(|e| format!("Failed to create persistence manager: {}", e))?;

            persistence
                .save(&corpus, workspace_path)
                .map_err(|e| format!("Failed to save corpus: {}", e))?;
            tracing::debug!(
                "[CORPUS-AUTO] persisted corpus for {} at {}",
                workspace_path.display(),
                persistence.corpus_dir().display()
            );

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
            tracing::warn!(
                "[CORPUS-AUTO] extraction failed for {}: {}",
                workspace_path.display(),
                e
            );
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

    let summary = persistence
        .summary_sqlite()
        .map_err(|e| format!("Failed to query corpus: {}", e))?;

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

    let node = persistence
        .get_node_dto(&node_id)
        .map_err(|e| format!("Failed to query corpus: {}", e))?
        .ok_or_else(|| format!("Node not found: {}", node_id))?;

    Ok(node)
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

    let min_conf = match min_confidence
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .as_deref()
    {
        Some("high") => crate::corpus::Confidence::High,
        Some("medium") => crate::corpus::Confidence::Medium,
        Some("low") => crate::corpus::Confidence::Low,
        Some(v) => {
            return Err(format!(
                "Invalid min_confidence value: '{}'; expected 'high', 'medium', or 'low'",
                v
            ))
        }
        None => crate::corpus::Confidence::Low,
    };

    let dtos = persistence
        .get_neighbor_dtos(&node_id, Some(min_conf))
        .map_err(|e| format!("Failed to query corpus neighbors: {}", e))?;

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
