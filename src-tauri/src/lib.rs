#![recursion_limit = "2048"]

use std::sync::Arc;

mod app_config;
pub mod approval_broker;
pub mod context_search;
mod conversation;
pub mod corpus;
mod harness_profile;
mod json_utils;
pub mod keychain;
mod llm;
pub mod mcp;
mod models;
mod provider_client;
mod review;
pub mod session_mode;
pub mod session_store;
mod session_turn;
mod shell_tools;
pub mod skills;
pub mod subprocess_output;
mod text_utils;
pub mod trace;
pub mod verification;
mod workspace_tools;

#[cfg(not(test))]
mod model_fetch;

#[cfg(test)]
mod model_fetch {
    use crate::models::{ModelInfoWithFreshness, ModelRegistry};

    pub async fn fetch_models_for_provider(
        provider: &str,
        _api_key: Option<&str>,
        _force_refresh: bool,
    ) -> ModelInfoWithFreshness {
        ModelInfoWithFreshness {
            models: ModelRegistry::hardcoded_models_for(provider),
            is_fresh: true,
            error_kind: None,
            error_detail: None,
        }
    }
}

use app_config::{AppConfigError, AppConfigState, AppConfigStore, Workspace};
use approval_broker::{
    ApprovalBroker, ApprovalDecision, ApprovalEventEmitter, ApprovalRequest, ApprovalResolution,
};
use clap::Parser;
use conversation::{ConversationState, ConversationStore};
use corpus::commands::{
    build_corpus, context_search, get_corpus_neighbors, get_corpus_status, get_corpus_summary,
    query_corpus, run_corpus_build_inner,
};
use corpus::persistence::CorpusPersistence;
use futures::{stream, StreamExt};
use llm::{LlmError, LlmService};
use models::{ModelInfo, ModelRegistry};
use once_cell::sync::Lazy;
use rig::providers::{chatgpt, copilot};
use serde::Serialize;
use session_store::{
    ArchiveMaintenanceResult, ArchivePolicy, ArchiveStats, ArchivedSessionRecord, SessionDetail,
    SessionRecord, SessionStore, SessionStoreState,
};
use shell_tools::{CommandApproval, CommandApprovalFuture, CommandApprovalRequest, CommandRisk};
use skills::SkillSummary;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use trace::TraceState;
use workspace_tools::{
    ExternalPathApproval, ExternalPathApprovalFuture, ExternalPathApprovalRequest, PathKind,
};

static CORPUS_BUILD_LOCK: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));
static REJECTION_STORE_LOCK: Lazy<tokio::sync::Mutex<()>> =
    Lazy::new(|| tokio::sync::Mutex::new(()));
const CORPUS_AUTO_BUILD_COMPLETE_EVENT: &str = "corpus-auto-build-complete";

pub struct SkillCache {
    pub loader: skills::SkillLoader,
}

impl SkillCache {
    fn new() -> Self {
        Self {
            loader: skills::SkillLoader::new(),
        }
    }
}

/// Tauri-managed state wrapping the single [`ApprovalBroker`] that owns every
/// pending agent-action approval. The broker outlives any individual command
/// or stream so the frontend can resolve requests issued during streaming.
pub struct ApprovalBrokerState {
    inner: Mutex<Option<Arc<ApprovalBroker>>>,
}

/// Tauri-managed state holding the in-flight streaming turn abort handles keyed
/// by session id. Used by `cancel_streaming` to abort a runaway or user-stopped
/// turn. A run that completes cleanly removes its own handle.
pub struct StreamingRunHandles {
    inner: Mutex<HashMap<String, futures::future::AbortHandle>>,
}

impl StreamingRunHandles {
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Insert an abort handle for the given session id, replacing any prior
    /// handle (which is aborted to avoid orphan tasks).
    fn insert(&self, session_id: &str, handle: futures::future::AbortHandle) {
        if let Some(prev) = self
            .inner
            .lock()
            .expect("streaming handles poisoned")
            .insert(session_id.to_string(), handle)
        {
            prev.abort();
        }
    }

    /// Remove and abort the handle for the given session id. Idempotent: returns
    /// silently if no handle is registered.
    fn cancel(&self, session_id: &str) {
        if let Some(handle) = self
            .inner
            .lock()
            .expect("streaming handles poisoned")
            .remove(session_id)
        {
            handle.abort();
        }
    }

    /// Remove the handle for the given session id without aborting (used when a
    /// run completes cleanly so the map doesn't grow without bound).
    fn remove(&self, session_id: &str) {
        self.inner
            .lock()
            .expect("streaming handles poisoned")
            .remove(session_id);
    }
}

impl ApprovalBrokerState {
    fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Install the broker once the Tauri app handle is available (called from
    /// the `setup` hook). Replaces any prior broker; in practice there is
    /// only ever one because Tauri state is constructed once per process.
    fn install(&self, broker: Arc<ApprovalBroker>) {
        let mut guard = self.inner.lock().expect("approval broker state poisoned");
        *guard = Some(broker);
    }

    /// Borrow the broker, returning an error if it has not been installed yet
    /// (e.g. if a command fires before `setup` runs).
    pub fn broker(&self) -> Result<Arc<ApprovalBroker>, String> {
        self.inner
            .lock()
            .expect("approval broker state poisoned")
            .clone()
            .ok_or_else(|| "Approval broker is unavailable".to_string())
    }
}

/// Tauri emitter that forwards broker events to the webview as
/// `approval-requested` / `approval-resolved`. The broker is intentionally
/// decoupled from Tauri; this adapter is the only place that knows about it.
struct TauriApprovalEventEmitter {
    app: tauri::AppHandle,
}

impl ApprovalEventEmitter for TauriApprovalEventEmitter {
    fn emit_requested(&self, request: &ApprovalRequest) {
        if let Err(err) = self
            .app
            .emit(approval_broker::APPROVAL_REQUESTED_EVENT, request)
        {
            tracing::warn!(error = %err, "failed to emit approval-requested event");
        }
    }

    fn emit_resolved(&self, resolution: &ApprovalResolution) {
        if let Err(err) = self
            .app
            .emit(approval_broker::APPROVAL_RESOLVED_EVENT, resolution)
        {
            tracing::warn!(error = %err, "failed to emit approval-resolved event");
        }
    }
}

#[derive(Clone)]
struct BrokerExternalPathApproval {
    broker: Arc<ApprovalBroker>,
}

impl ExternalPathApproval for BrokerExternalPathApproval {
    fn request_approval<'a>(
        &'a self,
        request: ExternalPathApprovalRequest,
    ) -> ExternalPathApprovalFuture<'a> {
        let broker = self.broker.clone();
        let reason = match request.kind {
            PathKind::File => "This file is outside the active workspace.",
            PathKind::Directory => "This directory is outside the active workspace.",
            PathKind::Symlink | PathKind::Other => "This path is outside the active workspace.",
        };
        Box::pin(async move {
            broker
                .request(ApprovalRequest::external_path(
                    request.tool_name,
                    request.path,
                    reason,
                ))
                .await
        })
    }
}

#[derive(Clone)]
struct BrokerCommandApproval {
    broker: Arc<ApprovalBroker>,
}

impl CommandApproval for BrokerCommandApproval {
    fn request_approval<'a>(
        &'a self,
        request: CommandApprovalRequest,
    ) -> CommandApprovalFuture<'a> {
        let broker = self.broker.clone();
        Box::pin(async move {
            let destructive = matches!(request.risk, CommandRisk::Destructive);
            broker
                .request(ApprovalRequest::command(
                    request.tool_name,
                    request.command_label,
                    request.reason,
                    destructive,
                ))
                .await
        })
    }
}

#[derive(Parser, Debug)]
#[command(name = "gospel", about = "Gospel AI coding assistant")]
#[command(version)]
struct Cli {
    #[arg(short = 'd', long = "dir")]
    dir: Option<String>,
}

#[derive(Serialize)]
struct ApiKeyStatus {
    configured: bool,
}

#[derive(Serialize)]
struct ProviderStatus {
    provider: String,
    configured: bool,
}

#[derive(Serialize, Clone)]
struct ProviderAvailability {
    provider: String,
    display_name: String,
    auth_type: String,
    credentialed: bool,
    visible: bool,
    model_fetch_status: String,
    model_count: usize,
    error_kind: Option<String>,
    error_detail: Option<String>,
}

#[derive(Serialize, Clone)]
struct ModelAvailabilitySnapshot {
    providers: Vec<ProviderAvailability>,
    available_models: Vec<ModelInfo>,
    empty_reason: Option<String>,
    warnings: Vec<String>,
}

#[derive(Serialize, Clone)]
struct CorpusAutoBuildComplete {
    success: bool,
    symbol_count: usize,
}

fn corpus_auto_build_complete_payload(
    success: bool,
    symbol_count: usize,
) -> CorpusAutoBuildComplete {
    CorpusAutoBuildComplete {
        success,
        symbol_count,
    }
}

pub(crate) fn corpus_auto_build_failure_payload() -> CorpusAutoBuildComplete {
    corpus_auto_build_complete_payload(false, 0)
}

pub(crate) fn emit_corpus_auto_build_complete<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    payload: CorpusAutoBuildComplete,
) {
    let _ = app.emit(CORPUS_AUTO_BUILD_COMPLETE_EVENT, payload);
}

pub(crate) fn validate_active_workspace_path(path: &Path) -> Result<(), String> {
    if !path
        .try_exists()
        .map_err(|e| format!("Failed to inspect workspace path {}: {}", path.display(), e))?
    {
        return Err(format!("Workspace path does not exist: {}", path.display()));
    }

    if !path.is_dir() {
        return Err(format!(
            "Workspace path is not a directory: {}",
            path.display()
        ));
    }

    Ok(())
}

#[derive(Serialize, Clone)]
struct OauthChallenge {
    verification_url: String,
    user_code: String,
}

#[derive(Serialize, Clone)]
struct OauthCompletion {
    provider: String,
    success: bool,
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn set_api_key(provider: String, api_key: String) -> Result<(), String> {
    keychain::store(&provider, &api_key).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_api_key(provider: String) -> Result<(), String> {
    keychain::delete(&provider).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_api_key_status(provider: String) -> ApiKeyStatus {
    ApiKeyStatus {
        configured: keychain::provider_has_credentials(&provider),
    }
}

#[tauri::command]
fn get_models(provider: String) -> Vec<String> {
    models::ModelRegistry::models_for_provider(&provider)
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[tauri::command]
fn get_configured_providers() -> Vec<ProviderStatus> {
    models::ModelRegistry::all_providers()
        .iter()
        .map(|&p| ProviderStatus {
            provider: p.to_string(),
            configured: keychain::provider_has_credentials(p),
        })
        .collect()
}

#[tauri::command]
async fn get_available_models(
    app_config: tauri::State<'_, AppConfigState>,
) -> Result<Vec<ModelInfo>, String> {
    let (visibility, warnings) = read_visibility_snapshot(&app_config);
    Ok(build_model_availability(visibility, warnings, false)
        .await
        .available_models)
}

#[tauri::command]
async fn get_model_availability(
    app_config: tauri::State<'_, AppConfigState>,
    force_refresh: Option<bool>,
) -> Result<ModelAvailabilitySnapshot, String> {
    let (visibility, warnings) = read_visibility_snapshot(&app_config);
    Ok(build_model_availability(visibility, warnings, force_refresh.unwrap_or(false)).await)
}

#[tauri::command]
fn set_provider_visibility(
    app_config: tauri::State<'_, AppConfigState>,
    provider: String,
    visible: bool,
) -> Result<(), String> {
    match &app_config.store {
        Some(store) => store
            .set_provider_visibility(&provider, visible)
            .map_err(|e| e.to_string()),
        None => Err(app_config
            .init_warning
            .clone()
            .unwrap_or_else(|| "App config store is unavailable".to_string())),
    }
}

fn app_config_store(app_config: &AppConfigState) -> Result<&AppConfigStore, String> {
    app_config.store.as_ref().ok_or_else(|| {
        app_config
            .init_warning
            .clone()
            .unwrap_or_else(|| "App config store is unavailable".to_string())
    })
}

#[tauri::command]
fn list_mcp_servers(
    app_config: tauri::State<'_, AppConfigState>,
) -> Result<Vec<mcp::McpServer>, String> {
    app_config_store(app_config.inner())?
        .list_mcp_servers()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn create_mcp_server(
    app_config: tauri::State<'_, AppConfigState>,
    request: mcp::CreateMcpServerRequest,
) -> Result<mcp::McpServer, String> {
    app_config_store(app_config.inner())?
        .create_mcp_server(request)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn update_mcp_server(
    app_config: tauri::State<'_, AppConfigState>,
    id: String,
    request: mcp::UpdateMcpServerRequest,
) -> Result<mcp::McpServer, String> {
    app_config_store(app_config.inner())?
        .update_mcp_server(&id, request)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_mcp_server(
    app_config: tauri::State<'_, AppConfigState>,
    id: String,
) -> Result<(), String> {
    app_config_store(app_config.inner())?
        .delete_mcp_server(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_mcp_server_enabled(
    app_config: tauri::State<'_, AppConfigState>,
    kind: String,
    id: String,
    enabled: bool,
) -> Result<mcp::McpServer, String> {
    app_config_store(app_config.inner())?
        .set_mcp_server_enabled(&kind, &id, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn trust_mcp_server(
    app_config: tauri::State<'_, AppConfigState>,
    id: String,
) -> Result<mcp::McpServer, String> {
    app_config_store(app_config.inner())?
        .trust_mcp_server(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn revoke_trust_mcp_server(
    app_config: tauri::State<'_, AppConfigState>,
    id: String,
) -> Result<mcp::McpServer, String> {
    app_config_store(app_config.inner())?
        .revoke_trust_mcp_server(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn refresh_mcp_server(
    app_config: tauri::State<'_, AppConfigState>,
    kind: String,
    id: String,
) -> Result<mcp::McpServer, String> {
    let store = app_config_store(app_config.inner())?;
    let active_workspace_path = store.get_workspace_path().ok().flatten().map(PathBuf::from);
    store
        .refresh_mcp_server(&kind, &id, active_workspace_path.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn preview_import_mcp_servers(
    app_config: tauri::State<'_, AppConfigState>,
    source_path: String,
) -> Result<mcp::McpImportPreview, String> {
    app_config_store(app_config.inner())?
        .preview_import_mcp_servers(&source_path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn apply_import_mcp_servers(
    app_config: tauri::State<'_, AppConfigState>,
    request: mcp::McpApplyImportRequest,
) -> Result<mcp::McpApplyImportResult, String> {
    app_config_store(app_config.inner())?
        .apply_import_mcp_servers(request)
        .map_err(|e| e.to_string())
}

async fn build_model_availability(
    visibility_by_provider: HashMap<String, bool>,
    warnings: Vec<String>,
    force_refresh: bool,
) -> ModelAvailabilitySnapshot {
    let provider_ids = models::ModelRegistry::all_providers();
    let mut providers: Vec<Option<ProviderAvailability>> = vec![None; provider_ids.len()];
    let mut fetch_inputs = Vec::new();
    let mut all_models = Vec::new();

    for (index, &provider) in provider_ids.iter().enumerate() {
        let credentialed = keychain::provider_has_credentials(provider);
        let visible = visibility_by_provider
            .get(provider)
            .copied()
            .unwrap_or(true);
        let model_count = 0;

        if !credentialed {
            providers[index] = Some(provider_availability(
                provider,
                credentialed,
                visible,
                "not_credentialed".to_string(),
                model_count,
                None,
                None,
            ));
            continue;
        }

        if !visible {
            providers[index] = Some(provider_availability(
                provider,
                credentialed,
                visible,
                "hidden".to_string(),
                model_count,
                None,
                None,
            ));
            continue;
        }

        let api_key = if ModelRegistry::is_oauth_provider(provider) {
            None
        } else {
            match keychain::retrieve(provider) {
                Ok(key) => Some(key),
                Err(_) => {
                    providers[index] = Some(provider_availability(
                        provider,
                        credentialed,
                        visible,
                        "failed".to_string(),
                        model_count,
                        Some("credentials_unavailable".to_string()),
                        Some("Saved provider credentials could not be read.".to_string()),
                    ));
                    continue;
                }
            }
        };

        fetch_inputs.push((index, provider.to_string(), api_key));
    }

    let mut fetched = stream::iter(fetch_inputs)
        .map(|(index, provider, api_key)| async move {
            let result = model_fetch::fetch_models_for_provider(
                &provider,
                api_key.as_deref(),
                force_refresh,
            )
            .await;
            let mut provider_models = result.models;
            provider_models.sort_by(|a, b| a.model.cmp(&b.model));
            let model_count = provider_models.len();
            let model_fetch_status =
                provider_model_fetch_status(model_count, result.is_fresh).to_string();
            let availability = provider_availability(
                &provider,
                true,
                true,
                model_fetch_status,
                model_count,
                result.error_kind,
                result.error_detail,
            );
            (index, availability, provider_models)
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;

    fetched.sort_by_key(|(index, _, _)| *index);
    for (index, availability, provider_models) in fetched {
        providers[index] = Some(availability);
        all_models.extend(provider_models);
    }

    let providers: Vec<ProviderAvailability> = providers.into_iter().flatten().collect();
    let empty_reason = availability_empty_reason(&providers, all_models.len());

    ModelAvailabilitySnapshot {
        providers,
        available_models: all_models,
        empty_reason,
        warnings,
    }
}

fn availability_empty_reason(
    providers: &[ProviderAvailability],
    available_model_count: usize,
) -> Option<String> {
    if available_model_count > 0 {
        return None;
    }

    let credentialed: Vec<&ProviderAvailability> =
        providers.iter().filter(|p| p.credentialed).collect();
    if credentialed.is_empty() {
        return Some("no_credentialed_providers".to_string());
    }

    let visible: Vec<&ProviderAvailability> =
        credentialed.into_iter().filter(|p| p.visible).collect();
    if visible.is_empty() {
        return Some("all_credentialed_providers_hidden".to_string());
    }

    if visible.iter().any(|p| p.model_fetch_status == "failed") {
        return Some("model_fetch_failed".to_string());
    }

    Some("no_visible_provider_models".to_string())
}

fn provider_model_fetch_status(model_count: usize, is_fresh: bool) -> &'static str {
    match (model_count, is_fresh) {
        (0, true) => "empty",
        (0, false) => "failed",
        (_, true) => "loaded",
        (_, false) => "stale",
    }
}

fn read_visibility_snapshot(state: &AppConfigState) -> (HashMap<String, bool>, Vec<String>) {
    let mut visibility_by_provider = HashMap::new();
    let mut warnings = Vec::new();

    if let Some(warning) = state.init_warning.clone() {
        warnings.push(warning);
    }

    if let Some(store) = &state.store {
        for &provider in models::ModelRegistry::all_providers() {
            match store.provider_visibility(provider) {
                Ok(visible) => {
                    visibility_by_provider.insert(provider.to_string(), visible);
                }
                Err(e) => warnings.push(format!(
                    "Failed to read provider visibility for {provider}; using default visible: {e}"
                )),
            }
        }
    }

    (visibility_by_provider, warnings)
}

fn provider_availability(
    provider: &str,
    credentialed: bool,
    visible: bool,
    model_fetch_status: String,
    model_count: usize,
    error_kind: Option<String>,
    error_detail: Option<String>,
) -> ProviderAvailability {
    ProviderAvailability {
        provider: provider.to_string(),
        display_name: provider_display_name(provider),
        auth_type: provider_auth_type(provider).to_string(),
        credentialed,
        visible,
        model_fetch_status,
        model_count,
        error_kind,
        error_detail,
    }
}

fn provider_display_name(provider: &str) -> String {
    let name = ModelRegistry::provider_display_name(provider);
    if name == "Unknown Provider" {
        provider.to_string()
    } else {
        name.to_string()
    }
}

fn provider_auth_type(provider: &str) -> &'static str {
    ModelRegistry::provider_auth_type(provider)
}

#[cfg(test)]
mod availability_tests {
    use super::*;

    #[test]
    fn provider_status_distinguishes_loaded_stale_empty_and_failed() {
        assert_eq!(provider_model_fetch_status(2, true), "loaded");
        assert_eq!(provider_model_fetch_status(2, false), "stale");
        assert_eq!(provider_model_fetch_status(0, true), "empty");
        assert_eq!(provider_model_fetch_status(0, false), "failed");
    }

    #[test]
    fn empty_reason_prefers_credentials_then_visibility_then_failures() {
        assert_eq!(
            availability_empty_reason(&[], 0).as_deref(),
            Some("no_credentialed_providers")
        );
        let hidden = vec![provider_availability(
            "openai",
            true,
            false,
            "hidden".to_string(),
            0,
            None,
            None,
        )];
        assert_eq!(
            availability_empty_reason(&hidden, 0).as_deref(),
            Some("all_credentialed_providers_hidden")
        );
        let failed = vec![provider_availability(
            "openai",
            true,
            true,
            "failed".to_string(),
            0,
            None,
            None,
        )];
        assert_eq!(
            availability_empty_reason(&failed, 0).as_deref(),
            Some("model_fetch_failed")
        );
        let empty = vec![provider_availability(
            "openai",
            true,
            true,
            "empty".to_string(),
            0,
            None,
            None,
        )];
        assert_eq!(
            availability_empty_reason(&empty, 0).as_deref(),
            Some("no_visible_provider_models")
        );
        assert_eq!(availability_empty_reason(&empty, 1), None);
    }
}

#[cfg(test)]
mod corpus_auto_build_event_tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn failure_event_contract_uses_existing_frontend_payload() {
        let payload = serde_json::to_value(corpus_auto_build_failure_payload()).unwrap();

        assert_eq!(
            CORPUS_AUTO_BUILD_COMPLETE_EVENT,
            "corpus-auto-build-complete"
        );
        assert_eq!(
            payload,
            serde_json::json!({
                "success": false,
                "symbol_count": 0,
            })
        );
    }

    #[test]
    fn active_workspace_path_must_exist_and_be_directory() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("missing");
        let file = dir.path().join("workspace-file");
        fs::write(&file, b"not a directory").unwrap();

        assert!(validate_active_workspace_path(dir.path()).is_ok());
        assert!(validate_active_workspace_path(&missing).is_err());
        assert!(validate_active_workspace_path(&file).is_err());
    }
}

#[tauri::command]
async fn complete(
    app: tauri::AppHandle,
    app_config: tauri::State<'_, AppConfigState>,
    provider: String,
    prompt: String,
    model: String,
) -> Result<String, llm::LlmErrorDto> {
    let workspace_path = match &app_config.store {
        Some(store) => store.get_workspace_path().ok().flatten().map(PathBuf::from),
        None => None,
    };
    eprintln!(
        "[CORPUS-AUTO] complete workspace path: {}",
        workspace_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".to_string())
    );

    if let Some(path) = &workspace_path {
        ensure_workspace_corpus(&app, path)
            .await
            .map_err(|e| LlmError::ProviderError(e).to_dto())?;
    }

    let full_prompt = match workspace_path {
        Some(path) => format!("[Workspace: {}]\n\n{}", path.display(), prompt),
        None => prompt,
    };

    if ModelRegistry::is_oauth_provider(&provider) {
        LlmService::completion(&provider, &full_prompt, &model, "")
            .await
            .map_err(|e| e.to_dto())
    } else {
        let api_key =
            keychain::retrieve(&provider).map_err(|_| LlmError::ApiKeyMissing.to_dto())?;
        LlmService::completion(&provider, &full_prompt, &model, &api_key)
            .await
            .map_err(|e| e.to_dto())
    }
}

#[tauri::command]
async fn test_connection(provider: String, model: String) -> Result<bool, String> {
    if ModelRegistry::is_oauth_provider(&provider) {
        let response =
            LlmService::completion(&provider, "Say 'pong' and nothing else.", &model, "").await;
        match response {
            Ok(_) => Ok(true),
            Err(e) => Err(e.to_string()),
        }
    } else {
        let api_key = keychain::retrieve(&provider).map_err(|e| e.to_string())?;
        let response =
            LlmService::completion(&provider, "Say 'pong' and nothing else.", &model, &api_key)
                .await;
        match response {
            Ok(_) => Ok(true),
            Err(e) => Err(e.to_string()),
        }
    }
}

fn active_review_workspace_path(app_config: &AppConfigState) -> Result<PathBuf, String> {
    let workspace = match &app_config.store {
        Some(store) => store
            .get_active_workspace()
            .map_err(|e| format!("Failed to get active workspace: {}", e))?,
        None => {
            return Err(app_config
                .init_warning
                .clone()
                .unwrap_or_else(|| "App config store is unavailable".to_string()))
        }
    }
    .ok_or_else(|| "No active workspace selected".to_string())?;
    let workspace_path = PathBuf::from(workspace.path);
    validate_active_workspace_path(&workspace_path)?;
    Ok(workspace_path)
}

fn review_api_key(provider: &str) -> Result<String, String> {
    if ModelRegistry::is_oauth_provider(provider) {
        Ok(String::new())
    } else {
        keychain::retrieve(provider).map_err(|_| format!("API key not configured for {}", provider))
    }
}

#[tauri::command]
async fn gospel_review(
    app: tauri::AppHandle,
    app_config: tauri::State<'_, AppConfigState>,
    config: review::ReviewConfig,
) -> Result<review::ReviewResult, String> {
    let workspace_path = active_review_workspace_path(&app_config)?;
    let api_key = review_api_key(&config.provider)?;
    let emitter = Arc::new(TauriReviewProgressEmitter { app: app.clone() });
    review::run_review(config, workspace_path, api_key, emitter).await
}

#[tauri::command]
async fn gospel_multi_review(
    app: tauri::AppHandle,
    app_config: tauri::State<'_, AppConfigState>,
    provider: String,
    model: String,
    mode: String,
    pr_number: Option<u64>,
    focuses: Option<Vec<review::ReviewFocus>>,
) -> Result<review::multi::MultiReviewResult, String> {
    let workspace_path = active_review_workspace_path(&app_config)?;
    let api_key = review_api_key(&provider)?;
    let focus_list = focuses.unwrap_or_else(|| review::multi::ALL_FOCUSES.to_vec());
    let emitter = Arc::new(TauriReviewProgressEmitter { app: app.clone() });
    review::multi::run_multi_focus_review(
        provider,
        model,
        mode,
        pr_number,
        &focus_list,
        workspace_path,
        api_key,
        emitter,
    )
    .await
}

/// Tauri-backed [`review::ReviewProgressEmitter`] that forwards every event to
/// the `review-progress` webview event. Mirrors the
/// `TauriSessionTurnAdapters` pattern: the trait lives in `review::progress`
/// (decoupled from Tauri), the real impl lives here next to the command.
struct TauriReviewProgressEmitter<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> review::ReviewProgressEmitter for TauriReviewProgressEmitter<R> {
    fn emit_progress(&self, event: review::ReviewProgressEvent) {
        if let Err(err) = self.app.emit("review-progress", event) {
            tracing::warn!(error = %err, "failed to emit review-progress event");
        }
    }
}

struct TauriSessionTurnAdapters<'a> {
    app: &'a tauri::AppHandle,
    app_config: &'a AppConfigState,
    conversation_state: &'a ConversationState,
    session_store_state: &'a SessionStoreState,
    trace_state: &'a TraceState,
    skill_cache: &'a SkillCache,
}

impl session_turn::SessionTurnWorkspace for TauriSessionTurnAdapters<'_> {
    fn active_workspace_selection(&self) -> Option<session_turn::ActiveWorkspaceSelection> {
        match &self.app_config.store {
            Some(store) => store
                .get_active_workspace()
                .ok()
                .flatten()
                .map(|workspace| session_turn::ActiveWorkspaceSelection {
                    id: workspace.id,
                    path: PathBuf::from(workspace.path),
                }),
            None => None,
        }
    }

    fn validate_workspace_path(&self, workspace_path: &Path) -> Result<(), String> {
        validate_active_workspace_path(workspace_path)
    }

    fn ensure_workspace_corpus<'b>(
        &'b self,
        workspace_path: &'b Path,
    ) -> session_turn::SessionTurnFuture<'b, Result<Option<usize>, String>> {
        Box::pin(async move { ensure_workspace_corpus(self.app, workspace_path).await })
    }

    fn emit_corpus_auto_build_failure(&self) {
        emit_corpus_auto_build_complete(self.app, corpus_auto_build_failure_payload());
    }
}

impl session_turn::SessionTurnCredentials for TauriSessionTurnAdapters<'_> {
    fn api_key(&self, provider: &str) -> Result<String, LlmError> {
        if ModelRegistry::is_oauth_provider(provider) {
            Ok(String::new())
        } else {
            keychain::retrieve(provider).map_err(|_| LlmError::ApiKeyMissing)
        }
    }
}

impl session_turn::SessionTurnSessions for TauriSessionTurnAdapters<'_> {
    fn validate_workspace_binding(
        &self,
        session_id: &str,
        active_workspace_id: Option<&str>,
    ) -> Result<(), String> {
        match &self.session_store_state.store {
            Some(store) => store
                .validate_workspace_binding(session_id, active_workspace_id)
                .map_err(|e| e.to_string()),
            None => Err("session store unavailable".to_string()),
        }
    }

    fn session_mode(&self, session_id: &str) -> Result<String, String> {
        match &self.session_store_state.store {
            Some(store) => store
                .get_session(session_id)
                .map_err(|e| e.to_string())?
                .map(|session| session.mode)
                .ok_or_else(|| format!("Session not found: {}", session_id)),
            None => Err("session store unavailable".to_string()),
        }
    }

    fn unresolved_notes(&self, session_id: &str) -> Vec<session_store::SessionNote> {
        match &self.session_store_state.store {
            Some(store) => store.list_unresolved_notes(session_id).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    fn failure_snapshot(&self, session_id: &str) -> Option<session_turn::SessionFailureSnapshot> {
        let store = self.session_store_state.store.as_ref()?;
        let detail = store.get_session(session_id).ok().flatten()?;
        Some(session_turn::SessionFailureSnapshot {
            display_transcript: detail.display_transcript,
            model_history: detail.model_history,
        })
    }

    fn persist_turn(
        &self,
        session_id: &str,
        display_transcript: &str,
        model_history: Option<&str>,
    ) -> Result<(), String> {
        match &self.session_store_state.store {
            Some(store) => store
                .persist_turn(session_id, display_transcript, model_history)
                .map_err(|e| e.to_string()),
            None => Err("session store unavailable".to_string()),
        }
    }

    fn update_model_selection(
        &self,
        session_id: &str,
        provider: &str,
        model: &str,
        variant: Option<&str>,
    ) -> Result<(), String> {
        match &self.session_store_state.store {
            Some(store) => store
                .update_model_selection(session_id, provider, model, variant)
                .map_err(|e| e.to_string()),
            None => Err("session store unavailable".to_string()),
        }
    }

    fn update_status(&self, session_id: &str, status: &str) -> Result<(), String> {
        match &self.session_store_state.store {
            Some(store) => store
                .update_status(session_id, status)
                .map_err(|e| e.to_string()),
            None => Err("session store unavailable".to_string()),
        }
    }
}

impl session_turn::SessionTurnConversation for TauriSessionTurnAdapters<'_> {
    fn chat_history(&self, session_id: Option<&str>) -> Vec<rig::completion::message::Message> {
        match session_id {
            Some(sid) => {
                let mut store = self.conversation_state.store.lock().unwrap();
                store.get_history(sid)
            }
            None => Vec::new(),
        }
    }

    fn store_history(&self, session_id: &str, history: Vec<rig::completion::message::Message>) {
        let mut store = self.conversation_state.store.lock().unwrap();
        store.store_history(session_id, history);
    }
}

impl session_turn::SessionTurnSkills for TauriSessionTurnAdapters<'_> {
    fn load_skills(&self, workspace_path: Option<&Path>) -> Vec<skills::Skill> {
        self.skill_cache.loader.load(workspace_path)
    }
}

impl session_turn::SessionTurnLlm for TauriSessionTurnAdapters<'_> {
    fn stream_completion<'b>(
        &'b self,
        request: session_turn::SessionTurnStreamRequest<'b>,
        on_event: Box<dyn FnMut(session_turn::SessionTurnEvent) + Send + 'b>,
    ) -> session_turn::SessionTurnFuture<'b, Result<llm::StreamCompletionResult, LlmError>> {
        let mut on_event = on_event;
        Box::pin(async move {
            let broker = self
                .app
                .state::<ApprovalBrokerState>()
                .broker()
                .expect("approval broker is installed during app setup");
            let external_path_approval: Arc<dyn ExternalPathApproval> =
                Arc::new(BrokerExternalPathApproval {
                    broker: broker.clone(),
                });
            let command_approval: Arc<dyn CommandApproval> = Arc::new(BrokerCommandApproval {
                broker: broker.clone(),
            });
            let mut skill_script_tool = request.skill_script_tool;
            if let Some(tool) = &mut skill_script_tool {
                tool.command_approval = Some(command_approval.clone());
            }

            llm::stream_completion(
                request.provider,
                request.prompt,
                request.model,
                request.variant,
                request.api_key,
                request.delegate_provider,
                request.delegate_model,
                request.delegate_api_key,
                request.workspace,
                Some(external_path_approval),
                Some(command_approval),
                request.chat_history,
                request.matched_skills_section,
                request.invoked_skill_section,
                skill_script_tool,
                move |event| on_event(session_turn::SessionTurnEvent::from(event)),
            )
            .await
        })
    }
}

impl session_turn::SessionTurnEvents for TauriSessionTurnAdapters<'_> {
    fn emit_stream_event(
        &self,
        session_id: &str,
        role: &str,
        run_id: &str,
        event: &session_turn::SessionTurnEvent,
    ) {
        if let Some(trace_event) = session_turn::trace_event_for_session_turn_event(
            event,
            session_id,
            role,
            trace::current_timestamp(),
        ) {
            self.trace_state.write_event(&trace_event);
        }

        let ui_event = session_turn::ui_event_payload(event, run_id);
        let _ = self.app.emit(ui_event.name, ui_event.payload);
    }

    fn trace_done(
        &self,
        session_id: &str,
        role: &str,
        response_length: usize,
        prompt_tokens: usize,
        response_tokens: usize,
        tool_calls: usize,
    ) {
        self.trace_state.write_event(&trace::TraceEvent::Done {
            session_id: session_id.to_string(),
            role: role.to_string(),
            response_length,
            prompt_tokens,
            response_tokens,
            tool_calls,
            timestamp: trace::current_timestamp(),
        });
    }

    fn trace_error(&self, session_id: &str, role: &str, error: &LlmError) {
        self.trace_state.write_event(&trace::TraceEvent::Error {
            session_id: session_id.to_string(),
            role: role.to_string(),
            error_code: error.to_dto().code,
            error_message: error.to_dto().message,
            timestamp: trace::current_timestamp(),
        });
    }

    fn emit_done(
        &self,
        run_id: &str,
        response: &str,
        prompt_tokens: usize,
        response_tokens: usize,
        tool_calls: usize,
    ) {
        let payload = serde_json::json!({
            "runId": run_id,
            "response": response,
            "prompt_tokens": prompt_tokens,
            "response_tokens": response_tokens,
            "tool_calls": tool_calls,
        });
        let _ = self.app.emit("llm-done", payload);
    }

    fn emit_error(&self, run_id: &str, error: &LlmError) {
        let dto = error.to_dto();
        let payload = serde_json::json!({
            "runId": run_id,
            "code": dto.code,
            "message": dto.message,
        });
        let _ = self.app.emit("llm-error", payload);
    }
}

impl session_turn::SessionTurnVerification for TauriSessionTurnAdapters<'_> {
    fn schedule_verification(&self, job: session_turn::VerificationJobRequest) {
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let result = verification::run_verification(
                &job.provider,
                &job.model,
                &job.api_key,
                &job.workspace,
                &job.response_to_verify,
                &job.user_prompt,
            )
            .await;

            if let Some(ref sid) = job.session_id {
                let session_store_state = app.state::<SessionStoreState>();
                if let Some(ref session_store) = session_store_state.store {
                    let unresolved_notes =
                        session_store.list_unresolved_notes(sid).unwrap_or_default();
                    for action in
                        session_turn::verification_note_actions(&result, &unresolved_notes)
                    {
                        match action {
                            session_turn::VerificationNoteAction::Create { note_type, content } => {
                                let _ = session_store.create_note(sid, &note_type, &content, None);
                            }
                            session_turn::VerificationNoteAction::Resolve { note_id } => {
                                let _ = session_store.resolve_note(&note_id);
                            }
                        }
                    }
                }
            }

            let _ = app.emit(
                "llm-verification",
                serde_json::json!({
                    "sessionId": job.session_id,
                    "status": result.status,
                    "concerns": result.concerns,
                    "summary": result.summary,
                }),
            );
        });
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn complete_streaming(
    app: tauri::AppHandle,
    app_config: tauri::State<'_, AppConfigState>,
    conversation_state: tauri::State<'_, ConversationState>,
    session_store_state: tauri::State<'_, SessionStoreState>,
    trace_state: tauri::State<'_, TraceState>,
    skill_cache: tauri::State<'_, SkillCache>,
    streaming_handles: tauri::State<'_, StreamingRunHandles>,
    provider: String,
    prompt: String,
    model: String,
    variant: Option<String>,
    session_id: Option<String>,
    invoked_skill: Option<session_turn::InvokedSkillRequest>,
) -> Result<String, llm::LlmErrorDto> {
    let (delegate_provider, delegate_model, delegate_api_key) =
        resolve_delegate_completion_config(app_config.inner(), &provider, &model);

    let adapters = TauriSessionTurnAdapters {
        app: &app,
        app_config: app_config.inner(),
        conversation_state: conversation_state.inner(),
        session_store_state: session_store_state.inner(),
        trace_state: trace_state.inner(),
        skill_cache: skill_cache.inner(),
    };

    // Spawn the turn so `cancel_streaming` can abort its task. When a session
    // id is present, register the handle keyed by it so cancel can find it;
    // local-only turns (no session id) cannot be cancelled but still run to
    // completion. The command awaits the spawned task so the IPC resolves when
    // the turn finishes (errors surface via `llm-error` events regardless).
    let session_key = session_id.clone();
    let turn_future = session_turn::run_streaming_turn(
        session_turn::StreamingTurnDependencies {
            workspace: &adapters,
            credentials: &adapters,
            sessions: &adapters,
            conversation: &adapters,
            skills: &adapters,
            llm: &adapters,
            events: &adapters,
            verification: &adapters,
        },
        session_turn::StreamingTurnRequest {
            provider,
            prompt,
            model,
            variant,
            delegate_provider,
            delegate_model,
            delegate_api_key,
            session_id,
            invoked_skill,
        },
    );

    let (abort_handle, abort_registration) = futures::future::AbortHandle::new_pair();
    if let Some(sid) = &session_key {
        streaming_handles.insert(sid, abort_handle);
    }

    let abortable = futures::future::Abortable::new(turn_future, abort_registration);
    let result = match abortable.await {
        Ok(turn_result) => turn_result,
        // Aborted by `cancel_streaming`. Surface a controlled-stop error so
        // the frontend finalize path treats this as a user-initiated stop.
        Err(_) => Err(llm::LlmError::ControlledStop(
            "Stream cancelled by user.".to_string(),
        )
        .to_dto()),
    };

    if let Some(sid) = &session_key {
        streaming_handles.remove(sid);
    }
    result
}

/// Cancel the in-flight streaming turn for the given session id. Idempotent:
/// returns `Ok(())` if no turn is in flight. Only backend-tracked turns (those
/// with a session id) can be cancelled.
#[tauri::command]
fn cancel_streaming(
    streaming_handles: tauri::State<'_, StreamingRunHandles>,
    session_id: String,
) -> Result<(), String> {
    streaming_handles.cancel(&session_id);
    Ok(())
}

fn resolve_delegate_completion_config(
    app_config: &AppConfigState,
    provider: &str,
    model: &str,
) -> (String, String, String) {
    let fallback_provider = provider.to_string();
    let fallback_model = model.to_string();

    let stored_provider = app_config
        .store
        .as_ref()
        .and_then(|store| store.get_config_value("delegate_provider").ok().flatten());
    let configured_provider = stored_provider.filter(|configured| {
        ModelRegistry::all_providers().contains(&configured.as_str())
    });
    let delegate_provider = configured_provider.unwrap_or(fallback_provider);

    let stored_model = app_config
        .store
        .as_ref()
        .and_then(|store| store.get_config_value("delegate_model").ok().flatten());
    let supported_models = ModelRegistry::models_for_provider(&delegate_provider);
    let delegate_model = stored_model
        .filter(|configured| supported_models.contains(&configured.as_str()))
        .unwrap_or(fallback_model);

    let delegate_api_key = if ModelRegistry::is_oauth_provider(&delegate_provider) {
        String::new()
    } else {
        keychain::retrieve(&delegate_provider).unwrap_or_default()
    };

    (delegate_provider, delegate_model, delegate_api_key)
}

#[tauri::command]
fn clear_conversation_history(
    conversation_state: tauri::State<'_, ConversationState>,
    session_id: String,
) -> Result<(), String> {
    let mut store = conversation_state.store.lock().unwrap();
    store.clear(&session_id);
    Ok(())
}

#[tauri::command]
fn export_conversation(
    conversation_state: tauri::State<'_, ConversationState>,
    session_id: String,
) -> Result<String, String> {
    let mut store = conversation_state.store.lock().unwrap();
    let history = store.get_history(&session_id);
    if history.is_empty() {
        return Err("Conversation not found".to_string());
    }
    serde_json::to_string_pretty(&history).map_err(|e| e.to_string())
}

fn emit_oauth_complete(
    app: &tauri::AppHandle,
    provider: &'static str,
    provider_event: &'static str,
    success: bool,
) {
    let _ = app.emit(provider_event, success);
    let _ = app.emit(
        "provider-auth-complete",
        OauthCompletion {
            provider: provider.to_string(),
            success,
        },
    );
}

async fn start_chatgpt_oauth_flow(
    app: tauri::AppHandle,
    provider_event: &'static str,
) -> Result<OauthChallenge, String> {
    use std::sync::{Arc, Mutex};
    use tokio::sync::Notify;

    let challenge = Arc::new(Mutex::new(None));
    let challenge_clone = challenge.clone();
    let notify = Arc::new(Notify::new());
    let notify_clone = notify.clone();

    let client = chatgpt::Client::builder()
        .oauth()
        .on_device_code(move |prompt| {
            let mut guard = challenge_clone.lock().unwrap();
            *guard = Some(OauthChallenge {
                verification_url: prompt.verification_uri.clone(),
                user_code: prompt.user_code.clone(),
            });
            notify_clone.notify_one();
        })
        .build()
        .map_err(|e| e.to_string())?;

    // Start authorization in background — this blocks polling for the token
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut retries = 0;
        let max_retries = 3;
        let mut success = false;

        while retries < max_retries && !success {
            match client.authorize().await {
                Ok(()) => {
                    success = true;
                    emit_oauth_complete(&app_clone, "chatgpt", provider_event, true);
                }
                Err(e) => {
                    retries += 1;
                    if retries >= max_retries {
                        eprintln!("ChatGPT OAuth failed after {} attempts: {}", retries, e);
                        emit_oauth_complete(&app_clone, "chatgpt", provider_event, false);
                    } else {
                        eprintln!(
                            "ChatGPT OAuth attempt {} failed: {}, retrying...",
                            retries, e
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
        }
    });

    // Wait for the on_device_code callback to fire (with 30s timeout)
    match tokio::time::timeout(std::time::Duration::from_secs(30), notify.notified()).await {
        Ok(()) => {}
        Err(_) => return Err("OAuth flow timed out before receiving device code".to_string()),
    }

    let maybe_challenge = { challenge.lock().unwrap().take() };

    if let Some(challenge) = maybe_challenge {
        // Open browser for user to authenticate
        if let Err(e) = app
            .opener()
            .open_url(&challenge.verification_url, None::<String>)
        {
            eprintln!("Failed to open browser: {}", e);
        }
        Ok(challenge)
    } else {
        Err("Failed to initiate OAuth flow".to_string())
    }
}

#[tauri::command]
async fn start_chatgpt_oauth(app: tauri::AppHandle) -> Result<OauthChallenge, String> {
    start_chatgpt_oauth_flow(app, "chatgpt-auth-complete").await
}

async fn start_github_copilot_oauth_flow(
    app: tauri::AppHandle,
    provider_event: &'static str,
) -> Result<OauthChallenge, String> {
    use std::sync::{Arc, Mutex};
    use tokio::sync::Notify;

    let challenge = Arc::new(Mutex::new(None));
    let challenge_clone = challenge.clone();
    let notify = Arc::new(Notify::new());
    let notify_clone = notify.clone();

    let client = copilot::Client::builder()
        .oauth()
        .token_dir(keychain::github_copilot_token_dir())
        .on_device_code(move |prompt| {
            let mut guard = challenge_clone.lock().unwrap();
            *guard = Some(OauthChallenge {
                verification_url: prompt.verification_uri.clone(),
                user_code: prompt.user_code.clone(),
            });
            notify_clone.notify_one();
        })
        .build()
        .map_err(|e| e.to_string())?;

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut retries = 0;
        let max_retries = 3;
        let mut success = false;

        while retries < max_retries && !success {
            match client.authorize().await {
                Ok(()) => {
                    success = true;
                    emit_oauth_complete(&app_clone, "github_copilot", provider_event, true);
                }
                Err(e) => {
                    retries += 1;
                    if retries >= max_retries {
                        eprintln!(
                            "GitHub Copilot OAuth failed after {} attempts: {}",
                            retries, e
                        );
                        emit_oauth_complete(&app_clone, "github_copilot", provider_event, false);
                    } else {
                        eprintln!(
                            "GitHub Copilot OAuth attempt {} failed: {}, retrying...",
                            retries, e
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
        }
    });

    match tokio::time::timeout(std::time::Duration::from_secs(30), notify.notified()).await {
        Ok(()) => {}
        Err(_) => return Err("OAuth flow timed out before receiving device code".to_string()),
    }

    let maybe_challenge = { challenge.lock().unwrap().take() };

    if let Some(challenge) = maybe_challenge {
        if let Err(e) = app
            .opener()
            .open_url(&challenge.verification_url, None::<String>)
        {
            eprintln!("Failed to open browser: {}", e);
        }
        Ok(challenge)
    } else {
        Err("Failed to initiate OAuth flow".to_string())
    }
}

#[tauri::command]
async fn start_github_copilot_oauth(app: tauri::AppHandle) -> Result<OauthChallenge, String> {
    start_github_copilot_oauth_flow(app, "github-copilot-auth-complete").await
}

#[tauri::command]
async fn start_provider_oauth(
    app: tauri::AppHandle,
    provider: String,
) -> Result<OauthChallenge, String> {
    match provider.as_str() {
        "chatgpt" => start_chatgpt_oauth_flow(app, "chatgpt-auth-complete").await,
        "github_copilot" => {
            start_github_copilot_oauth_flow(app, "github-copilot-auth-complete").await
        }
        other => Err(format!("Provider {} does not support OAuth", other)),
    }
}

#[tauri::command]
fn is_chatgpt_authenticated() -> ApiKeyStatus {
    ApiKeyStatus {
        configured: keychain::has_chatgpt_oauth_session(),
    }
}

#[tauri::command]
fn is_github_copilot_authenticated() -> ApiKeyStatus {
    ApiKeyStatus {
        configured: keychain::has_github_copilot_oauth_session(),
    }
}

#[tauri::command]
fn is_provider_authenticated(provider: String) -> ApiKeyStatus {
    ApiKeyStatus {
        configured: keychain::provider_has_credentials(&provider),
    }
}

#[tauri::command]
fn logout_chatgpt() -> Result<(), String> {
    keychain::delete_chatgpt_auth_file().map_err(|e| e.to_string())?;
    let _ = keychain::delete("chatgpt");
    Ok(())
}

#[tauri::command]
fn logout_github_copilot() -> Result<(), String> {
    keychain::delete_github_copilot_auth_files().map_err(|e| e.to_string())?;
    let _ = keychain::delete("github_copilot");
    Ok(())
}

#[tauri::command]
fn logout_provider_oauth(provider: String) -> Result<(), String> {
    match provider.as_str() {
        "chatgpt" => logout_chatgpt(),
        "github_copilot" => logout_github_copilot(),
        other => Err(format!("Provider {} does not support OAuth", other)),
    }
}

#[tauri::command]
async fn pick_workspace_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let folder = app
        .dialog()
        .file()
        .set_title("Select workspace directory")
        .blocking_pick_folder();
    match folder {
        Some(path) => Ok(Some(path.to_string())),
        None => Ok(None),
    }
}

#[tauri::command]
fn resolve_approval_request(
    state: tauri::State<'_, ApprovalBrokerState>,
    id: String,
    decision: ApprovalDecision,
) -> Result<bool, String> {
    let broker = state.broker()?;
    Ok(broker.resolve(&id, decision))
}

fn hydrate_workspace_session_counts(
    workspaces: &mut [Workspace],
    session_store: &SessionStoreState,
) -> Result<(), String> {
    let Some(store) = &session_store.store else {
        for workspace in workspaces {
            workspace.session_count = 0;
        }
        return Ok(());
    };
    let counts = store
        .workspace_session_counts()
        .map_err(|e| e.to_string())?;
    for workspace in workspaces {
        workspace.session_count = *counts.get(&workspace.id).unwrap_or(&0);
    }
    Ok(())
}

fn list_workspaces_response(
    app_config: &AppConfigState,
    session_store: &SessionStoreState,
) -> Result<Vec<Workspace>, String> {
    match &app_config.store {
        Some(store) => {
            let mut workspaces = store.list_workspaces().map_err(|e| e.to_string())?;
            hydrate_workspace_session_counts(&mut workspaces, session_store)?;
            Ok(workspaces)
        }
        None => Err(app_config
            .init_warning
            .clone()
            .unwrap_or_else(|| "App config store is unavailable".to_string())),
    }
}

#[tauri::command]
fn list_workspaces(
    app_config: tauri::State<'_, AppConfigState>,
    session_store: tauri::State<'_, SessionStoreState>,
) -> Result<Vec<Workspace>, String> {
    list_workspaces_response(app_config.inner(), session_store.inner())
}

#[tauri::command]
fn add_workspace(
    app_config: tauri::State<'_, AppConfigState>,
    path: String,
) -> Result<Workspace, String> {
    match &app_config.store {
        Some(store) => store.add_workspace(&path).map_err(|e| e.to_string()),
        None => Err(app_config
            .init_warning
            .clone()
            .unwrap_or_else(|| "App config store is unavailable".to_string())),
    }
}

#[tauri::command]
fn remove_workspace(
    app_config: tauri::State<'_, AppConfigState>,
    skill_cache: tauri::State<'_, SkillCache>,
    id: String,
) -> Result<(), String> {
    match &app_config.store {
        Some(store) => {
            if let Ok(workspaces) = store.list_workspaces() {
                if let Some(ws) = workspaces.into_iter().find(|w| w.id == id) {
                    skill_cache.loader.invalidate(&PathBuf::from(&ws.path));
                }
            }
            store.remove_workspace(&id).map_err(|e| e.to_string())
        }
        None => Err(app_config
            .init_warning
            .clone()
            .unwrap_or_else(|| "App config store is unavailable".to_string())),
    }
}

fn spawn_corpus_auto_build(app: tauri::AppHandle, workspace_path: PathBuf, delay: Duration) {
    tracing::debug!(
        "[CORPUS-AUTO] scheduling workspace-switch build for {} after {:?}",
        workspace_path.display(),
        delay
    );
    tauri::async_runtime::spawn(async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }

        run_corpus_auto_build(app, workspace_path).await;
    });
}

fn spawn_startup_corpus_auto_build(app: tauri::AppHandle) {
    tracing::debug!("[CORPUS-AUTO] scheduling startup corpus check after 500ms");
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;

        let workspace_path = {
            let app_config = app.state::<AppConfigState>();
            match &app_config.store {
                Some(store) => match store.get_active_workspace() {
                    Ok(Some(workspace)) => {
                        tracing::debug!(
                            "[CORPUS-AUTO] startup active workspace: {} ({})",
                            workspace.name,
                            workspace.path
                        );
                        Some(PathBuf::from(workspace.path))
                    }
                    Ok(None) => {
                        tracing::debug!("[CORPUS-AUTO] startup check skipped: no active workspace");
                        None
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[CORPUS-AUTO] could not read active workspace for startup check: {}",
                            e
                        );
                        None
                    }
                },
                None => {
                    tracing::debug!(
                        "[CORPUS-AUTO] startup check skipped: app config store unavailable"
                    );
                    None
                }
            }
        };

        if let Some(workspace_path) = workspace_path {
            run_corpus_auto_build(app, workspace_path).await;
        }
    });
}

async fn run_corpus_auto_build(app: tauri::AppHandle, workspace_path: PathBuf) {
    match ensure_workspace_corpus(&app, &workspace_path).await {
        Ok(Some(symbol_count)) => {
            emit_corpus_auto_build_complete(
                &app,
                corpus_auto_build_complete_payload(true, symbol_count),
            );
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!(
                "[CORPUS-AUTO] corpus auto-build failed for {}: {}",
                workspace_path.display(),
                e
            );
            emit_corpus_auto_build_complete(&app, corpus_auto_build_failure_payload());
        }
    }
}

pub(crate) async fn ensure_workspace_corpus(
    app: &tauri::AppHandle,
    workspace_path: &Path,
) -> Result<Option<usize>, String> {
    eprintln!(
        "[CORPUS-AUTO] ensure requested for {}",
        workspace_path.display()
    );

    validate_active_workspace_path(workspace_path)?;

    let _guard = CORPUS_BUILD_LOCK.lock().await;
    let persistence = CorpusPersistence::new(workspace_path)
        .map_err(|e| format!("Failed to create persistence manager: {}", e))?;

    if persistence.exists() {
        eprintln!(
            "[CORPUS-AUTO] corpus already exists for {}",
            workspace_path.display()
        );
        return Ok(None);
    }

    eprintln!(
        "[CORPUS-AUTO] corpus missing; building for {}",
        workspace_path.display()
    );
    // Use the inner (lock already held by us) to avoid re-acquiring the
    // non-reentrant CORPUS_BUILD_LOCK and deadlocking.
    run_corpus_build_inner(app, workspace_path, None).await?;

    let persistence = CorpusPersistence::new(workspace_path)
        .map_err(|e| format!("Failed to create persistence manager: {}", e))?;
    let symbol_count = persistence
        .summary_sqlite()
        .map_err(|e| format!("Failed to query corpus summary: {}", e))?
        .symbol_count;

    eprintln!(
        "[CORPUS-AUTO] corpus build complete for {} with {} symbols",
        workspace_path.display(),
        symbol_count
    );
    Ok(Some(symbol_count))
}

#[tauri::command]
fn set_active_workspace(
    app: tauri::AppHandle,
    app_config: tauri::State<'_, AppConfigState>,
    skill_cache: tauri::State<'_, SkillCache>,
    id: String,
) -> Result<(), String> {
    match &app_config.store {
        Some(store) => {
            let old_path = store.get_workspace_path().ok().flatten();
            store.set_active_workspace(&id).map_err(|e| e.to_string())?;

            if let Some(ref old) = old_path {
                skill_cache.loader.invalidate(&PathBuf::from(old));
            }

            match store.get_active_workspace().map_err(|e| e.to_string()) {
                Ok(Some(workspace)) => {
                    tracing::debug!(
                        "[CORPUS-AUTO] active workspace set to {} ({})",
                        workspace.name,
                        workspace.path
                    );
                    skill_cache
                        .loader
                        .invalidate(&PathBuf::from(&workspace.path));
                    spawn_corpus_auto_build(app, PathBuf::from(workspace.path), Duration::ZERO);
                }
                Ok(None) => {
                    tracing::debug!(
                        "[CORPUS-AUTO] set active workspace succeeded but no workspace is active"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "[CORPUS-AUTO] set active workspace succeeded but could not read it back: {}",
                        e
                    );
                }
            }
            Ok(())
        }
        None => Err(app_config
            .init_warning
            .clone()
            .unwrap_or_else(|| "App config store is unavailable".to_string())),
    }
}

fn get_active_workspace_response(
    app_config: &AppConfigState,
    session_store: &SessionStoreState,
) -> Result<Option<Workspace>, String> {
    match &app_config.store {
        Some(store) => {
            let mut workspace = store.get_active_workspace().map_err(|e| e.to_string())?;
            if let Some(workspace) = workspace.as_mut() {
                hydrate_workspace_session_counts(std::slice::from_mut(workspace), session_store)?;
            }
            Ok(workspace)
        }
        None => Err(app_config
            .init_warning
            .clone()
            .unwrap_or_else(|| "App config store is unavailable".to_string())),
    }
}

#[tauri::command]
fn get_active_workspace(
    app_config: tauri::State<'_, AppConfigState>,
    session_store: tauri::State<'_, SessionStoreState>,
) -> Result<Option<Workspace>, String> {
    get_active_workspace_response(app_config.inner(), session_store.inner())
}

#[tauri::command]
fn list_skills(
    app_config: tauri::State<'_, AppConfigState>,
    skill_cache: tauri::State<'_, SkillCache>,
) -> Result<Vec<SkillSummary>, String> {
    let workspace_path = match &app_config.store {
        Some(store) => store.get_workspace_path().ok().flatten().map(PathBuf::from),
        None => None,
    };

    let skills = skill_cache.loader.load(workspace_path.as_deref());
    Ok(skills.iter().map(skills::SkillSummary::from).collect())
}

#[tauri::command]
fn reload_skills(
    app_config: tauri::State<'_, AppConfigState>,
    skill_cache: tauri::State<'_, SkillCache>,
) -> Result<Vec<SkillSummary>, String> {
    let workspace_path = match &app_config.store {
        Some(store) => store.get_workspace_path().ok().flatten().map(PathBuf::from),
        None => None,
    };

    if let Some(ref path) = workspace_path {
        skill_cache.loader.invalidate(path);
    }

    let skills = skill_cache.loader.load(workspace_path.as_deref());
    Ok(skills.iter().map(skills::SkillSummary::from).collect())
}

#[tauri::command]
fn create_session(
    session_store: tauri::State<'_, SessionStoreState>,
    title: String,
    provider: String,
    model: String,
    variant: Option<String>,
    workspace_id: Option<String>,
    mode: Option<String>,
) -> Result<SessionRecord, String> {
    let mode = mode.unwrap_or_else(|| session_mode::SESSION_MODE_BUILD.to_string());
    match &session_store.store {
        Some(store) => store
            .create_session_with_selection(
                &title,
                &provider,
                &model,
                variant.as_deref(),
                workspace_id.as_deref(),
                &mode,
            )
            .map_err(|e| e.to_string()),
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn update_session_model_selection(
    session_store: tauri::State<'_, SessionStoreState>,
    session_id: String,
    provider: String,
    model: String,
    variant: Option<String>,
) -> Result<(), String> {
    match &session_store.store {
        Some(store) => store
            .update_model_selection(&session_id, &provider, &model, variant.as_deref())
            .map_err(|e| e.to_string()),
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn update_session_mode(
    session_store: tauri::State<'_, SessionStoreState>,
    session_id: String,
    mode: String,
) -> Result<(), String> {
    match &session_store.store {
        Some(store) => store
            .update_session_mode(&session_id, &mode)
            .map_err(|e| e.to_string()),
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn get_session(
    session_store: tauri::State<'_, SessionStoreState>,
    conversation_state: tauri::State<'_, ConversationState>,
    app_config: tauri::State<'_, AppConfigState>,
    session_id: String,
) -> Result<SessionDetail, String> {
    let detail = match &session_store.store {
        Some(store) => {
            let detail = store
                .get_session(&session_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Session not found: {}", session_id))?;
            validate_session_access(store, &session_id, app_config.inner())?;
            detail
        }
        None => {
            return Err(session_store
                .init_warning
                .clone()
                .unwrap_or_else(|| "Session store is unavailable".to_string()));
        }
    };

    // Hydrate the in-memory conversation store with the persisted Model History
    // so a subsequent `complete_streaming` call can continue the conversation
    // with prior turns visible to the LLM, not just the UI transcript.
    if let Some(model_history_json) = detail.model_history.as_deref() {
        match serde_json::from_str::<Vec<rig::completion::message::Message>>(model_history_json) {
            Ok(messages) if !messages.is_empty() => {
                let mut store = conversation_state.store.lock().unwrap();
                store.store_history(&session_id, messages);
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    "Failed to parse model history for session {}: {}",
                    session_id,
                    e
                );
            }
        }
    }

    Ok(detail)
}

#[tauri::command]
fn list_sessions(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    workspace_id: Option<String>,
) -> Result<Vec<SessionRecord>, String> {
    let workspace_id = workspace_id.or_else(|| match &app_config.store {
        Some(store) => store
            .get_workspace_path()
            .ok()
            .flatten()
            .and_then(|_| store.get_active_workspace().ok().flatten().map(|ws| ws.id)),
        None => None,
    });

    match &session_store.store {
        Some(store) => {
            // Clean up stale drafts on list
            let _ = store.clean_stale_drafts();
            let _ = store.run_archive_maintenance(workspace_id.as_deref());
            store
                .list_sessions_for_workspace(workspace_id.as_deref())
                .map_err(|e| e.to_string())
        }
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn list_archived_sessions(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    workspace_id: Option<String>,
) -> Result<Vec<ArchivedSessionRecord>, String> {
    let workspace_id = workspace_id.or_else(|| match &app_config.store {
        Some(store) => store
            .get_workspace_path()
            .ok()
            .flatten()
            .and_then(|_| store.get_active_workspace().ok().flatten().map(|ws| ws.id)),
        None => None,
    });

    match &session_store.store {
        Some(store) => {
            let _ = store.run_archive_maintenance(workspace_id.as_deref());
            store
                .list_archived_sessions_for_workspace(workspace_id.as_deref())
                .map_err(|e| e.to_string())
        }
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn get_archive_policy(
    session_store: tauri::State<'_, SessionStoreState>,
    workspace_id: Option<String>,
) -> Result<ArchivePolicy, String> {
    match &session_store.store {
        Some(store) => store
            .get_archive_policy(workspace_id.as_deref())
            .map_err(|e| e.to_string()),
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn set_archive_policy(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    workspace_id: Option<String>,
    retention_days: i64,
    auto_archive_hours: i64,
) -> Result<ArchivePolicy, String> {
    validate_archive_policy_values(retention_days, auto_archive_hours)?;
    if let Some(ref workspace_id) = workspace_id {
        validate_workspace_id_access(workspace_id, app_config.inner())?;
    }

    match &session_store.store {
        Some(store) => store
            .set_archive_policy(workspace_id.as_deref(), retention_days, auto_archive_hours)
            .map_err(|e| e.to_string()),
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn clear_workspace_archive_policy(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    workspace_id: String,
) -> Result<ArchivePolicy, String> {
    validate_workspace_id_access(&workspace_id, app_config.inner())?;
    match &session_store.store {
        Some(store) => store
            .clear_workspace_archive_policy(&workspace_id)
            .map_err(|e| e.to_string()),
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn get_archive_stats(
    session_store: tauri::State<'_, SessionStoreState>,
    workspace_id: Option<String>,
) -> Result<ArchiveStats, String> {
    match &session_store.store {
        Some(store) => store
            .archive_stats(workspace_id.as_deref())
            .map_err(|e| e.to_string()),
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn run_archive_maintenance(
    session_store: tauri::State<'_, SessionStoreState>,
    workspace_id: Option<String>,
) -> Result<ArchiveMaintenanceResult, String> {
    match &session_store.store {
        Some(store) => store
            .run_archive_maintenance(workspace_id.as_deref())
            .map_err(|e| e.to_string()),
        None => Ok(ArchiveMaintenanceResult {
            archived_count: 0,
            deleted_count: 0,
        }),
    }
}

#[tauri::command]
fn archive_session(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    session_id: String,
) -> Result<ArchivedSessionRecord, String> {
    match &session_store.store {
        Some(store) => {
            validate_session_access(store, &session_id, app_config.inner())?;
            store
                .archive_session(&session_id)
                .map_err(|e| e.to_string())
        }
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn archive_sessions(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    session_ids: Vec<String>,
) -> Result<Vec<ArchivedSessionRecord>, String> {
    match &session_store.store {
        Some(store) => {
            for session_id in &session_ids {
                validate_session_access(store, session_id, app_config.inner())?;
            }
            store
                .archive_sessions_by_ids(&session_ids)
                .map_err(|e| e.to_string())
        }
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn restore_archived_session(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    session_id: String,
) -> Result<SessionRecord, String> {
    match &session_store.store {
        Some(store) => {
            validate_archived_session_access(store, &session_id, app_config.inner())?;
            store
                .restore_archived_session(&session_id)
                .map_err(|e| e.to_string())
        }
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn restore_archived_sessions(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    session_ids: Vec<String>,
) -> Result<Vec<SessionRecord>, String> {
    match &session_store.store {
        Some(store) => {
            for session_id in &session_ids {
                validate_archived_session_access(store, session_id, app_config.inner())?;
            }
            store
                .restore_archived_sessions_by_ids(&session_ids)
                .map_err(|e| e.to_string())
        }
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn delete_archived_session(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    session_id: String,
) -> Result<(), String> {
    match &session_store.store {
        Some(store) => {
            validate_archived_session_access(store, &session_id, app_config.inner())?;
            store
                .delete_archived_session(&session_id)
                .map_err(|e| e.to_string())
        }
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn delete_archived_sessions(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    session_ids: Vec<String>,
) -> Result<usize, String> {
    match &session_store.store {
        Some(store) => {
            for session_id in &session_ids {
                validate_archived_session_access(store, session_id, app_config.inner())?;
            }
            store
                .delete_archived_sessions_by_ids(&session_ids)
                .map_err(|e| e.to_string())
        }
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn delete_expired_archived_sessions(
    session_store: tauri::State<'_, SessionStoreState>,
    workspace_id: Option<String>,
) -> Result<usize, String> {
    match &session_store.store {
        Some(store) => {
            let policy = store
                .get_archive_policy(workspace_id.as_deref())
                .map_err(|e| e.to_string())?;
            store
                .delete_archived_sessions_older_than_days(
                    workspace_id.as_deref(),
                    policy.retention_days,
                )
                .map_err(|e| e.to_string())
        }
        None => Ok(0),
    }
}

#[tauri::command]
fn delete_session_cmd(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    session_id: String,
) -> Result<(), String> {
    match &session_store.store {
        Some(store) => {
            validate_session_access(store, &session_id, app_config.inner())?;
            store.delete_session(&session_id).map_err(|e| e.to_string())
        }
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn cleanup_stale_drafts(
    session_store: tauri::State<'_, SessionStoreState>,
) -> Result<usize, String> {
    match &session_store.store {
        Some(store) => store.clean_stale_drafts().map_err(|e| e.to_string()),
        None => Ok(0),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum ExportFormat {
    Transcript,
    Debug,
    Internal,
}

#[tauri::command]
fn export_session(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    session_id: String,
    format: ExportFormat,
) -> Result<String, String> {
    match &session_store.store {
        Some(store) => {
            let detail = store
                .get_session(&session_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Session not found: {}", session_id))?;
            validate_session_access(store, &session_id, app_config.inner())?;

            match format {
                ExportFormat::Transcript => {
                    // UI-safe: only Display Transcript, no Model History
                    Ok(detail.display_transcript)
                }
                ExportFormat::Debug => {
                    // Includes tool activity from Display Transcript
                    Ok(detail.display_transcript)
                }
                ExportFormat::Internal => {
                    // Full internal: Display Transcript + Model History
                    let export = serde_json::json!({
                        "session": {
                            "id": detail.id,
                            "title": detail.title,
                            "provider": detail.provider,
                            "model": detail.model,
                            "status": detail.status,
                            "workspace_id": detail.workspace_id,
                            "created_at": detail.created_at,
                            "updated_at": detail.updated_at,
                        },
                        "display_transcript": serde_json::from_str::<serde_json::Value>(&detail.display_transcript)
                            .unwrap_or(serde_json::json!([])),
                        "model_history": detail.model_history.as_ref()
                            .and_then(|h| serde_json::from_str::<serde_json::Value>(h).ok()),
                    });
                    serde_json::to_string_pretty(&export).map_err(|e| e.to_string())
                }
            }
        }
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn export_archived_sessions(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    session_ids: Vec<String>,
) -> Result<String, String> {
    match &session_store.store {
        Some(store) => {
            for session_id in &session_ids {
                validate_archived_session_access(store, session_id, app_config.inner())?;
            }
            store
                .export_archived_sessions(&session_ids)
                .map_err(|e| e.to_string())
        }
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn import_archived_sessions(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    payload: String,
    workspace_id: Option<String>,
) -> Result<Vec<ArchivedSessionRecord>, String> {
    if let Some(ref workspace_id) = workspace_id {
        validate_workspace_id_access(workspace_id, app_config.inner())?;
    }

    match &session_store.store {
        Some(store) => store
            .import_archived_sessions(&payload, workspace_id.as_deref())
            .map_err(|e| e.to_string()),
        None => Err(session_store
            .init_warning
            .clone()
            .unwrap_or_else(|| "Session store is unavailable".to_string())),
    }
}

#[tauri::command]
fn get_workspace_session_count(
    session_store: tauri::State<'_, SessionStoreState>,
    workspace_id: String,
) -> Result<i64, String> {
    match &session_store.store {
        Some(store) => store
            .workspace_session_count(&workspace_id)
            .map_err(|e| e.to_string()),
        None => Ok(0),
    }
}

#[tauri::command]
fn create_session_note(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    session_id: String,
    note_type: String,
    content: String,
    source_message_id: Option<String>,
) -> Result<session_store::SessionNote, String> {
    match &session_store.store {
        Some(store) => {
            validate_session_access(store, &session_id, app_config.inner())?;
            store
                .create_note(
                    &session_id,
                    &note_type,
                    &content,
                    source_message_id.as_deref(),
                )
                .map_err(|e| e.to_string())
        }
        None => Err("Session store not initialized".to_string()),
    }
}

#[tauri::command]
fn list_session_notes(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    session_id: String,
) -> Result<Vec<session_store::SessionNote>, String> {
    match &session_store.store {
        Some(store) => {
            validate_session_access(store, &session_id, app_config.inner())?;
            store
                .list_unresolved_notes(&session_id)
                .map_err(|e| e.to_string())
        }
        None => Ok(vec![]),
    }
}

#[tauri::command]
fn resolve_session_note(
    session_store: tauri::State<'_, SessionStoreState>,
    app_config: tauri::State<'_, AppConfigState>,
    note_id: String,
) -> Result<(), String> {
    match &session_store.store {
        Some(store) => {
            validate_note_access(store, &note_id, app_config.inner())?;
            store.resolve_note(&note_id).map_err(|e| e.to_string())
        }
        None => Err("Session store not initialized".to_string()),
    }
}

fn active_workspace_id(app_config: &AppConfigState) -> Option<String> {
    match &app_config.store {
        Some(store) => store.get_active_workspace().ok().flatten().map(|ws| ws.id),
        None => None,
    }
}

fn validate_archive_policy_values(
    retention_days: i64,
    auto_archive_hours: i64,
) -> Result<(), String> {
    let retention_allowed = [7, 30, 90, 365];
    let auto_archive_allowed = [0, 1, 24, 168, 720];
    if !retention_allowed.contains(&retention_days) {
        return Err("retention_days must be one of 7, 30, 90, or 365".to_string());
    }
    if !auto_archive_allowed.contains(&auto_archive_hours) {
        return Err("auto_archive_hours must be one of 0, 1, 24, 168, or 720".to_string());
    }
    Ok(())
}

fn validate_workspace_id_access(
    workspace_id: &str,
    app_config: &AppConfigState,
) -> Result<(), String> {
    let active_ws_id = active_workspace_id(app_config)
        .ok_or_else(|| "No active workspace is available".to_string())?;
    if workspace_id != active_ws_id {
        return Err(format!(
            "Workspace {} is not the active workspace {}",
            workspace_id, active_ws_id
        ));
    }
    Ok(())
}

fn validate_session_access(
    store: &SessionStore,
    session_id: &str,
    app_config: &AppConfigState,
) -> Result<(), String> {
    let active_ws_id = active_workspace_id(app_config);
    store
        .validate_workspace_binding(session_id, active_ws_id.as_deref())
        .map_err(|e| e.to_string())
}

fn validate_archived_session_access(
    store: &SessionStore,
    session_id: &str,
    app_config: &AppConfigState,
) -> Result<(), String> {
    let active_ws_id = active_workspace_id(app_config);
    store
        .validate_archived_workspace_binding(session_id, active_ws_id.as_deref())
        .map_err(|e| e.to_string())
}

fn validate_note_access(
    store: &SessionStore,
    note_id: &str,
    app_config: &AppConfigState,
) -> Result<(), String> {
    let session_id = store
        .note_session_id(note_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session note not found: {}", note_id))?;
    validate_session_access(store, &session_id, app_config)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cli = Cli::parse();

    let app_config_state = match app_config::AppConfigStore::new() {
        Ok(store) => {
            if let Some(ref dir_path) = cli.dir {
                let workspace_for_dir = match store.add_workspace(dir_path) {
                    Ok(ws) => Some(ws),
                    Err(AppConfigError::WorkspacePathExists(existing_path)) => {
                        match store.list_workspaces() {
                            Ok(workspaces) => workspaces
                                .into_iter()
                                .find(|workspace| workspace.path == existing_path),
                            Err(e) => {
                                eprintln!(
                                    "Warning: could not list workspaces for --dir activation: {}",
                                    e
                                );
                                None
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: could not add --dir workspace: {}", e);
                        None
                    }
                };

                if let Some(workspace) = workspace_for_dir {
                    if let Err(e) = store.set_active_workspace(&workspace.id) {
                        eprintln!("Warning: could not set --dir workspace as active: {}", e);
                    }
                }
            }
            AppConfigState {
                store: Some(store),
                init_warning: None,
            }
        }
        Err(e) => AppConfigState {
            store: None,
            init_warning: Some(format!(
                "App config store unavailable; provider visibility defaults to visible: {e}"
            )),
        },
    };

    let session_store_state = match session_store::SessionStore::new() {
        Ok(store) => SessionStoreState {
            store: Some(store),
            init_warning: None,
        },
        Err(e) => SessionStoreState {
            store: None,
            init_warning: Some(format!("Session store unavailable: {e}")),
        },
    };

    let mut trace_state = TraceState::new();
    trace_state.init();

    tauri::Builder::default()
        .manage(app_config_state)
        .manage(ConversationState {
            store: std::sync::Mutex::new(ConversationStore::new()),
        })
        .manage(session_store_state)
        .manage(trace_state)
        .manage(SkillCache::new())
        .manage(ApprovalBrokerState::new())
        .manage(StreamingRunHandles::new())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            set_api_key,
            delete_api_key,
            get_api_key_status,
            get_models,
            get_configured_providers,
            get_available_models,
            get_model_availability,
            set_provider_visibility,
            list_mcp_servers,
            create_mcp_server,
            update_mcp_server,
            delete_mcp_server,
            set_mcp_server_enabled,
            trust_mcp_server,
            revoke_trust_mcp_server,
            refresh_mcp_server,
            preview_import_mcp_servers,
            apply_import_mcp_servers,
            complete,
            complete_streaming,
            cancel_streaming,
            clear_conversation_history,
            export_conversation,
            test_connection,
            gospel_review,
            gospel_multi_review,
            start_chatgpt_oauth,
            start_github_copilot_oauth,
            start_provider_oauth,
            is_chatgpt_authenticated,
            is_github_copilot_authenticated,
            is_provider_authenticated,
            logout_chatgpt,
            logout_github_copilot,
            logout_provider_oauth,
            pick_workspace_directory,
            resolve_approval_request,
            list_workspaces,
            add_workspace,
            remove_workspace,
            set_active_workspace,
            get_active_workspace,
            list_skills,
            reload_skills,
            create_session,
            update_session_model_selection,
            update_session_mode,
            get_session,
            list_sessions,
            list_archived_sessions,
            get_archive_policy,
            set_archive_policy,
            clear_workspace_archive_policy,
            get_archive_stats,
            run_archive_maintenance,
            archive_session,
            archive_sessions,
            restore_archived_session,
            restore_archived_sessions,
            delete_archived_session,
            delete_archived_sessions,
            delete_expired_archived_sessions,
            delete_session_cmd,
            cleanup_stale_drafts,
            export_session,
            export_archived_sessions,
            import_archived_sessions,
            get_workspace_session_count,
            create_session_note,
            list_session_notes,
            resolve_session_note,
            // Corpus commands
            build_corpus,
            get_corpus_status,
            get_corpus_summary,
            query_corpus,
            get_corpus_neighbors,
            context_search,
            gospel_reject_review_comment,
            gospel_record_review_outcome,
        ])
        .setup(|app| {
            let emitter = Arc::new(TauriApprovalEventEmitter {
                app: app.handle().clone(),
            });
            let broker = Arc::new(ApprovalBroker::new(emitter));
            app.state::<ApprovalBrokerState>().install(broker);
            spawn_startup_corpus_auto_build(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
async fn gospel_reject_review_comment(
    app_config: tauri::State<'_, AppConfigState>,
    comment: review::ReviewComment,
) -> Result<(), String> {
    let workspace = match &app_config.store {
        Some(store) => store
            .get_active_workspace()
            .map_err(|e| format!("Failed to get active workspace: {}", e))?,
        None => return Err("App config store is unavailable".to_string()),
    }
    .ok_or_else(|| "No active workspace selected".to_string())?;

    let workspace_path = PathBuf::from(workspace.path);
    remember_rejected_review_comment(&workspace_path, &comment).await
}

async fn remember_rejected_review_comment(
    workspace_path: &Path,
    comment: &review::ReviewComment,
) -> Result<(), String> {
    let _guard = REJECTION_STORE_LOCK.lock().await;
    validate_active_workspace_path(workspace_path)?;
    let mut store = review::anti_pattern::AntiPatternStore::load(workspace_path)?;
    store.add_rejection(
        comment.focus,
        &comment.file,
        comment.line_start,
        comment.line_end,
        &comment.title,
    );
    store.save(workspace_path)
}

#[tauri::command]
async fn gospel_record_review_outcome(
    app_config: tauri::State<'_, AppConfigState>,
    run_id: String,
    comment_id: String,
    outcome: review::ReviewOutcome,
) -> Result<review::ReviewOutcomeOutput, String> {
    let workspace = match &app_config.store {
        Some(store) => store
            .get_active_workspace()
            .map_err(|e| format!("Failed to get active workspace: {}", e))?,
        None => return Err("App config store is unavailable".to_string()),
    }
    .ok_or_else(|| "No active workspace selected".to_string())?;

    let workspace_path = PathBuf::from(workspace.path);
    let _guard = REJECTION_STORE_LOCK.lock().await;
    validate_active_workspace_path(&workspace_path)?;
    review::outcome::record_review_outcome(&workspace_path, &run_id, &comment_id, outcome)
}

#[cfg(test)]
mod review_rejection_tests {
    use super::*;
    use tempfile::tempdir;

    fn review_comment(title: &str, evidence: &str) -> review::ReviewComment {
        review::ReviewComment {
            comment_id: String::new(),
            file: "src/main.rs".to_string(),
            line_start: 10,
            line_end: 12,
            severity: review::Severity::High,
            category: "injection".to_string(),
            focus: review::ReviewFocus::Security,
            focus_subcategory: None,
            cwe_id: Some("CWE-78".to_string()),
            cwe_name: Some("OS Command Injection".to_string()),
            title: title.to_string(),
            description: "User input reaches a shell command.".to_string(),
            rationale: Some("Rejecting this finding should persist.".to_string()),
            evidence: evidence.to_string(),
            suggestion: Some("Avoid shell execution.".to_string()),
            verification_plan: Some("Run the review again and verify it is filtered.".to_string()),
            signal_tier: review::SignalTier::Tier1,
        }
    }

    #[tokio::test]
    async fn rejected_review_comment_requires_existing_workspace() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("missing-workspace");
        let comment = review_comment("Unsanitized command", "Command::new(\"sh\")");

        let error = remember_rejected_review_comment(&missing, &comment)
            .await
            .unwrap_err();

        assert!(error.contains("Workspace path does not exist"));
        assert!(!missing.exists());
    }

    #[tokio::test]
    async fn rejected_review_comment_updates_preserve_existing_rejections() {
        let dir = tempdir().unwrap();
        let first = review_comment("Unsanitized command", "Command::new(\"sh\")");
        let second = review_comment("Leaky log", "println!(\"token={token}\")");

        remember_rejected_review_comment(dir.path(), &first)
            .await
            .unwrap();
        remember_rejected_review_comment(dir.path(), &second)
            .await
            .unwrap();

        let store = review::anti_pattern::AntiPatternStore::load(dir.path()).unwrap();
        assert!(store.is_rejected(
            first.focus,
            &first.file,
            first.line_start,
            first.line_end,
            &first.title
        ));
        assert!(store.is_rejected(
            second.focus,
            &second.file,
            second.line_start,
            second.line_end,
            &second.title
        ));
    }
}

#[cfg(test)]
mod workspace_response_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn list_and_active_workspace_responses_use_derived_session_counts() {
        let root = tempdir().unwrap();
        let first_path = root.path().join("first");
        let second_path = root.path().join("second");
        std::fs::create_dir_all(&first_path).unwrap();
        std::fs::create_dir_all(&second_path).unwrap();

        let app_store = crate::app_config::AppConfigStore::in_memory_for_test().unwrap();
        let first_workspace = app_store
            .add_workspace(first_path.to_str().unwrap())
            .unwrap();
        let second_workspace = app_store
            .add_workspace(second_path.to_str().unwrap())
            .unwrap();
        app_store
            .set_active_workspace(&first_workspace.id)
            .expect("active workspace can be set");

        let session_store = SessionStore::in_memory_for_test().unwrap();
        let active = session_store
            .create_session("Active", "openai", "gpt-4", Some(&first_workspace.id))
            .unwrap();
        let errored = session_store
            .create_session("Errored", "openai", "gpt-4", Some(&first_workspace.id))
            .unwrap();
        let draft = session_store
            .create_session("Draft", "openai", "gpt-4", Some(&first_workspace.id))
            .unwrap();

        session_store.update_status(&active.id, "active").unwrap();
        session_store.update_status(&errored.id, "error").unwrap();

        let app_state = AppConfigState {
            store: Some(app_store),
            init_warning: None,
        };
        let session_state = SessionStoreState {
            store: Some(session_store),
            init_warning: None,
        };

        let workspaces = list_workspaces_response(&app_state, &session_state).unwrap();
        let first = workspaces
            .iter()
            .find(|workspace| workspace.id == first_workspace.id)
            .unwrap();
        let second = workspaces
            .iter()
            .find(|workspace| workspace.id == second_workspace.id)
            .unwrap();

        assert_eq!(first.session_count, 2);
        assert_eq!(second.session_count, 0);

        let active_workspace = get_active_workspace_response(&app_state, &session_state)
            .unwrap()
            .unwrap();
        assert_eq!(active_workspace.id, first_workspace.id);
        assert_eq!(active_workspace.session_count, 2);
        assert_eq!(
            session_state
                .store
                .as_ref()
                .unwrap()
                .get_session(&draft.id)
                .unwrap()
                .unwrap()
                .status,
            "draft"
        );

        let missing_session_state = SessionStoreState {
            store: None,
            init_warning: None,
        };
        let fallback_workspaces =
            list_workspaces_response(&app_state, &missing_session_state).unwrap();
        assert!(fallback_workspaces
            .iter()
            .all(|workspace| workspace.session_count == 0));
    }
}

#[cfg(test)]
mod streaming_run_handles_tests {
    use super::*;

    #[test]
    fn cancel_with_no_in_flight_handle_is_idempotent() {
        let handles = StreamingRunHandles::new();
        // Cancelling a session that was never streamed must not error.
        handles.cancel("never-streamed");
        // The map stays empty.
        assert!(handles
            .inner
            .lock()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn remove_with_no_in_flight_handle_is_idempotent() {
        let handles = StreamingRunHandles::new();
        handles.remove("never-streamed");
        assert!(handles.inner.lock().unwrap().is_empty());
    }

    #[test]
    fn insert_then_cancel_aborts_and_clears() {
        let handles = StreamingRunHandles::new();
        let (abort_handle, _reg) = futures::future::AbortHandle::new_pair();
        handles.insert("session-1", abort_handle);
        assert!(handles.inner.lock().unwrap().contains_key("session-1"));
        handles.cancel("session-1");
        assert!(!handles.inner.lock().unwrap().contains_key("session-1"));
    }

    #[test]
    fn remove_after_insert_keeps_handle_unaborted() {
        let handles = StreamingRunHandles::new();
        let (abort_handle, _reg) = futures::future::AbortHandle::new_pair();
        handles.insert("session-1", abort_handle);
        handles.remove("session-1");
        assert!(!handles.inner.lock().unwrap().contains_key("session-1"));
    }
}
