pub mod keychain;
mod llm;
mod models;

use llm::{LlmError, LlmService};
use models::ModelInfo;
use serde::Serialize;
use tauri::Emitter;

#[derive(Serialize)]
struct ApiKeyStatus {
    configured: bool,
}

#[derive(Serialize)]
struct ProviderStatus {
    provider: String,
    configured: bool,
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
fn get_api_key_status(provider: String) -> ApiKeyStatus {
    ApiKeyStatus {
        configured: keychain::has_key(&provider),
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
            configured: keychain::has_key(p),
        })
        .collect()
}

#[tauri::command]
fn get_available_models() -> Vec<ModelInfo> {
    models::ModelRegistry::get_available_models(|p| keychain::has_key(p))
}

#[tauri::command]
async fn complete(provider: String, prompt: String, model: String) -> Result<String, llm::LlmErrorDto> {
    let api_key = keychain::retrieve(&provider).map_err(|_| LlmError::ApiKeyMissing.to_dto())?;
    LlmService::completion(&provider, &prompt, &model, &api_key)
        .await
        .map_err(|e| e.to_dto())
}

#[tauri::command]
async fn test_connection(provider: String, model: String) -> Result<bool, String> {
    let api_key = keychain::retrieve(&provider).map_err(|e| e.to_string())?;
    let response = LlmService::completion(&provider, "Say 'pong' and nothing else.", &model, &api_key).await;
    match response {
        Ok(_) => Ok(true),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn complete_streaming(
    app: tauri::AppHandle,
    provider: String,
    prompt: String,
    model: String,
) -> Result<(), llm::LlmErrorDto> {
    let api_key = keychain::retrieve(&provider).map_err(|_| LlmError::ApiKeyMissing.to_dto())?;

    let app_clone = app.clone();
    let result = llm::stream_completion(&provider, &prompt, &model, &api_key, move |token| {
        let _ = app_clone.emit("llm-token", token);
    })
    .await;

    match result {
        Ok(full_response) => {
            let _ = app.emit("llm-done", full_response);
            Ok(())
        }
        Err(e) => {
            let _ = app.emit("llm-error", e.to_dto());
            Err(e.to_dto())
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            set_api_key,
            get_api_key_status,
            get_models,
            get_configured_providers,
            get_available_models,
            complete,
            complete_streaming,
            test_connection,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
