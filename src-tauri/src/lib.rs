#![recursion_limit = "2048"]

mod app_config;
mod conversation;
pub mod corpus;
pub mod keychain;
mod llm;
mod models;

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

use app_config::{AppConfigError, AppConfigState, Workspace};
use clap::Parser;
use conversation::{ConversationState, ConversationStore};
use corpus::commands::{
    build_corpus, get_corpus_neighbors, get_corpus_status, get_corpus_summary, query_corpus,
    run_corpus_build,
};
use corpus::persistence::CorpusPersistence;
use futures::{stream, StreamExt};
use llm::{LlmError, LlmService, StreamEvent};
use models::ModelInfo;
use once_cell::sync::Lazy;
use rig::providers::chatgpt;
use serde::Serialize;
use std::{collections::HashMap, path::PathBuf, time::Duration};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

static CORPUS_BUILD_LOCK: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));

#[derive(Parser, Debug)]
#[command(name = "gospel", about = "Gospel AI coding assistant")]
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

#[derive(Serialize, Clone)]
struct OauthChallenge {
    verification_url: String,
    user_code: String,
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

        let api_key = if provider == "chatgpt" {
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
    match provider {
        "openai" => "OpenAI".to_string(),
        "chatgpt" => "ChatGPT Plus/Pro".to_string(),
        "anthropic" => "Anthropic".to_string(),
        "gemini" => "Gemini".to_string(),
        "groq" => "Groq".to_string(),
        "mistral" => "Mistral".to_string(),
        _ => provider.to_string(),
    }
}

fn provider_auth_type(provider: &str) -> &'static str {
    match provider {
        "chatgpt" => "oauth",
        _ => "api_key",
    }
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

    if provider == "chatgpt" {
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
    if provider == "chatgpt" {
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

#[tauri::command]
async fn complete_streaming(
    app: tauri::AppHandle,
    app_config: tauri::State<'_, AppConfigState>,
    conversation_state: tauri::State<'_, ConversationState>,
    provider: String,
    prompt: String,
    model: String,
    session_id: Option<String>,
) -> Result<(), llm::LlmErrorDto> {
    let workspace_path = match &app_config.store {
        Some(store) => store
            .get_workspace_path()
            .ok()
            .flatten()
            .map(std::path::PathBuf::from),
        None => None,
    };
    eprintln!(
        "[CORPUS-AUTO] complete_streaming workspace path: {}",
        workspace_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".to_string())
    );

    if let Some(path) = &workspace_path {
        if let Err(e) = ensure_workspace_corpus(&app, path).await {
            let dto = LlmError::ProviderError(e).to_dto();
            let _ = app.emit("llm-error", dto.clone());
            return Err(dto);
        }
    }

    let api_key = if provider == "chatgpt" {
        String::new()
    } else {
        keychain::retrieve(&provider).map_err(|_| LlmError::ApiKeyMissing.to_dto())?
    };

    let chat_history = match &session_id {
        Some(sid) => {
            let mut store = conversation_state.store.lock().unwrap();
            store.get_history(sid)
        }
        None => vec![],
    };

    let app_clone = app.clone();
    let result = llm::stream_completion(
        &provider,
        &prompt,
        &model,
        &api_key,
        workspace_path,
        chat_history,
        move |event| match event {
            StreamEvent::Text(token) => {
                let _ = app_clone.emit("llm-token", token);
            }
            StreamEvent::ToolCall { name, arguments } => {
                let _ = app_clone.emit(
                    "llm-tool-call",
                    serde_json::json!({ "name": name, "arguments": arguments }),
                );
            }
            StreamEvent::ToolResult { name, result } => {
                let _ = app_clone.emit(
                    "llm-tool-result",
                    serde_json::json!({ "name": name, "result": result }),
                );
            }
        },
    )
    .await;

    match result {
        Ok(stream_result) => {
            if let (Some(sid), Some(history)) = (&session_id, stream_result.history) {
                let mut store = conversation_state.store.lock().unwrap();
                store.store_history(sid, history);
            }
            let _ = app.emit("llm-done", stream_result.full_response);
            Ok(())
        }
        Err(e) => {
            let _ = app.emit("llm-error", e.to_dto());
            Err(e.to_dto())
        }
    }
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
async fn start_chatgpt_oauth(app: tauri::AppHandle) -> Result<OauthChallenge, String> {
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
                    let _ = app_clone.emit("chatgpt-auth-complete", true);
                }
                Err(e) => {
                    retries += 1;
                    if retries >= max_retries {
                        eprintln!("ChatGPT OAuth failed after {} attempts: {}", retries, e);
                        let _ = app_clone.emit("chatgpt-auth-complete", false);
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
fn is_chatgpt_authenticated() -> ApiKeyStatus {
    ApiKeyStatus {
        configured: keychain::has_chatgpt_oauth_session(),
    }
}

#[tauri::command]
fn logout_chatgpt() -> Result<(), String> {
    keychain::delete_chatgpt_auth_file().map_err(|e| e.to_string())?;
    let _ = keychain::delete("chatgpt");
    Ok(())
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
fn list_workspaces(app_config: tauri::State<'_, AppConfigState>) -> Result<Vec<Workspace>, String> {
    match &app_config.store {
        Some(store) => store.list_workspaces().map_err(|e| e.to_string()),
        None => Err(app_config
            .init_warning
            .clone()
            .unwrap_or_else(|| "App config store is unavailable".to_string())),
    }
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
    id: String,
) -> Result<(), String> {
    match &app_config.store {
        Some(store) => store.remove_workspace(&id).map_err(|e| e.to_string()),
        None => Err(app_config
            .init_warning
            .clone()
            .unwrap_or_else(|| "App config store is unavailable".to_string())),
    }
}

fn spawn_corpus_auto_build(app: tauri::AppHandle, workspace_path: PathBuf, delay: Duration) {
    eprintln!(
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
    eprintln!("[CORPUS-AUTO] scheduling startup corpus check after 500ms");
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;

        let workspace_path = {
            let app_config = app.state::<AppConfigState>();
            match &app_config.store {
                Some(store) => match store.get_active_workspace() {
                    Ok(Some(workspace)) => {
                        eprintln!(
                            "[CORPUS-AUTO] startup active workspace: {} ({})",
                            workspace.name, workspace.path
                        );
                        Some(PathBuf::from(workspace.path))
                    }
                    Ok(None) => {
                        eprintln!("[CORPUS-AUTO] startup check skipped: no active workspace");
                        None
                    }
                    Err(e) => {
                        eprintln!(
                            "[CORPUS-AUTO] could not read active workspace for startup check: {}",
                            e
                        );
                        None
                    }
                },
                None => {
                    eprintln!("[CORPUS-AUTO] startup check skipped: app config store unavailable");
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
            let _ = app.emit(
                "corpus-auto-build-complete",
                CorpusAutoBuildComplete {
                    success: true,
                    symbol_count,
                },
            );
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!(
                "[CORPUS-AUTO] corpus auto-build failed for {}: {}",
                workspace_path.display(),
                e
            );
            let _ = app.emit(
                "corpus-auto-build-complete",
                CorpusAutoBuildComplete {
                    success: false,
                    symbol_count: 0,
                },
            );
        }
    }
}

async fn ensure_workspace_corpus(
    app: &tauri::AppHandle,
    workspace_path: &PathBuf,
) -> Result<Option<usize>, String> {
    eprintln!(
        "[CORPUS-AUTO] ensure requested for {}",
        workspace_path.display()
    );
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
    run_corpus_build(app, workspace_path, None).await?;

    let symbol_count = CorpusPersistence::new(workspace_path)
        .and_then(|persistence| persistence.summary_sqlite())
        .map(|summary| summary.symbol_count)
        .unwrap_or(0);

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
    id: String,
) -> Result<(), String> {
    match &app_config.store {
        Some(store) => {
            store.set_active_workspace(&id).map_err(|e| e.to_string())?;
            if let Some(workspace) = store.get_active_workspace().map_err(|e| e.to_string())? {
                eprintln!(
                    "[CORPUS-AUTO] active workspace set to {} ({})",
                    workspace.name, workspace.path
                );
                spawn_corpus_auto_build(app, PathBuf::from(workspace.path), Duration::ZERO);
            }
            Ok(())
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
) -> Result<Option<Workspace>, String> {
    match &app_config.store {
        Some(store) => store.get_active_workspace().map_err(|e| e.to_string()),
        None => Err(app_config
            .init_warning
            .clone()
            .unwrap_or_else(|| "App config store is unavailable".to_string())),
    }
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

    tauri::Builder::default()
        .manage(app_config_state)
        .manage(ConversationState {
            store: std::sync::Mutex::new(ConversationStore::new()),
        })
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
            complete,
            complete_streaming,
            clear_conversation_history,
            test_connection,
            start_chatgpt_oauth,
            is_chatgpt_authenticated,
            logout_chatgpt,
            pick_workspace_directory,
            list_workspaces,
            add_workspace,
            remove_workspace,
            set_active_workspace,
            get_active_workspace,
            // Corpus commands
            build_corpus,
            get_corpus_status,
            get_corpus_summary,
            query_corpus,
            get_corpus_neighbors,
        ])
        .setup(|app| {
            spawn_startup_corpus_auto_build(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
