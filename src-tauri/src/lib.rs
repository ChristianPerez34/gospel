pub mod keychain;
mod llm;
mod models;

use llm::{LlmError, LlmService};
use models::ModelInfo;
use rig::providers::chatgpt;
use serde::Serialize;
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;

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
    if provider == "chatgpt" {
        LlmService::completion(&provider, &prompt, &model, "")
            .await
            .map_err(|e| e.to_dto())
    } else {
        let api_key = keychain::retrieve(&provider).map_err(|_| LlmError::ApiKeyMissing.to_dto())?;
        LlmService::completion(&provider, &prompt, &model, &api_key)
            .await
            .map_err(|e| e.to_dto())
    }
}

#[tauri::command]
async fn test_connection(provider: String, model: String) -> Result<bool, String> {
    if provider == "chatgpt" {
        let response = LlmService::completion(&provider, "Say 'pong' and nothing else.", &model, "").await;
        match response {
            Ok(_) => Ok(true),
            Err(e) => Err(e.to_string()),
        }
    } else {
        let api_key = keychain::retrieve(&provider).map_err(|e| e.to_string())?;
        let response = LlmService::completion(&provider, "Say 'pong' and nothing else.", &model, &api_key).await;
        match response {
            Ok(_) => Ok(true),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[tauri::command]
async fn complete_streaming(
    app: tauri::AppHandle,
    provider: String,
    prompt: String,
    model: String,
) -> Result<(), llm::LlmErrorDto> {
    let api_key = if provider == "chatgpt" {
        String::new()
    } else {
        keychain::retrieve(&provider).map_err(|_| LlmError::ApiKeyMissing.to_dto())?
    };

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
                        eprintln!("ChatGPT OAuth attempt {} failed: {}, retrying...", retries, e);
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
        if let Err(e) = app.opener().open_url(&challenge.verification_url, None::<String>) {
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
        configured: keychain::has_key("chatgpt"),
    }
}

#[tauri::command]
fn logout_chatgpt() -> Result<(), String> {
    keychain::delete("chatgpt").map_err(|e| e.to_string())
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
            start_chatgpt_oauth,
            is_chatgpt_authenticated,
            logout_chatgpt,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
