use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;

use crate::providers::{CredentialedProviderRegistration, ProviderAuthKind};

#[derive(Serialize, Clone)]
pub struct OauthChallenge {
    pub verification_url: String,
    pub user_code: String,
}

#[derive(Serialize, Clone)]
pub struct OauthCompletion {
    pub provider: String,
    pub success: bool,
}

fn oauth_provider(id: &str) -> Option<&'static CredentialedProviderRegistration> {
    crate::providers::provider(id).filter(|entry| entry.auth_kind == ProviderAuthKind::Oauth)
}

pub async fn start_provider_oauth(
    app: tauri::AppHandle,
    provider: &str,
) -> Result<OauthChallenge, String> {
    let entry = oauth_provider(provider)
        .ok_or_else(|| format!("Provider {} does not support OAuth", provider))?;
    match entry.id {
        "chatgpt" => start_chatgpt(app).await,
        "github_copilot" => start_github_copilot(app).await,
        "grok" => start_grok(app).await,
        other => Err(format!("Provider {} does not support OAuth", other)),
    }
}

fn start_chatgpt(
    app: tauri::AppHandle,
) -> Pin<Box<dyn Future<Output = Result<OauthChallenge, String>> + Send>> {
    Box::pin(async move {
        start_registered_oauth_flow(app, "chatgpt", "ChatGPT", |on_device_code| {
            let client = Arc::new(
                rig::providers::chatgpt::Client::builder()
                    .oauth()
                    .on_device_code(move |prompt| {
                        on_device_code(prompt.verification_uri.clone(), prompt.user_code.clone());
                    })
                    .build()
                    .map_err(|e| e.to_string())?,
            );
            Ok(move || {
                let client = Arc::clone(&client);
                async move { client.authorize().await.map_err(|e| e.to_string()) }
            })
        })
        .await
    })
}

fn start_grok(
    app: tauri::AppHandle,
) -> Pin<Box<dyn Future<Output = Result<OauthChallenge, String>> + Send>> {
    Box::pin(async move {
        start_registered_oauth_flow(app, "grok", "Grok", |on_device_code| {
            Ok(move || {
                let on_device_code = Arc::clone(&on_device_code);
                async move {
                    crate::grok_oauth::login(
                        &crate::grok_oauth::ReqwestPoster,
                        &crate::grok_oauth::Endpoints::default(),
                        &crate::keychain::grok_auth_file_path(),
                        |url, code| on_device_code(url, code),
                    )
                    .await
                }
            })
        })
        .await
    })
}

fn start_github_copilot(
    app: tauri::AppHandle,
) -> Pin<Box<dyn Future<Output = Result<OauthChallenge, String>> + Send>> {
    Box::pin(async move {
        start_registered_oauth_flow(app, "github_copilot", "GitHub Copilot", |on_device_code| {
            let client = Arc::new(
                rig::providers::copilot::Client::builder()
                    .oauth()
                    .token_dir(crate::keychain::github_copilot_token_dir())
                    .on_device_code(move |prompt| {
                        on_device_code(prompt.verification_uri.clone(), prompt.user_code.clone());
                    })
                    .build()
                    .map_err(|e| e.to_string())?,
            );
            Ok(move || {
                let client = Arc::clone(&client);
                async move { client.authorize().await.map_err(|e| e.to_string()) }
            })
        })
        .await
    })
}

fn emit_oauth_complete(
    app: &tauri::AppHandle,
    provider: &CredentialedProviderRegistration,
    success: bool,
) {
    if let Some(hooks) = provider.oauth.as_ref() {
        let _ = app.emit(hooks.auth_complete_event, success);
    }
    let _ = app.emit(
        "provider-auth-complete",
        OauthCompletion {
            provider: provider.id.to_string(),
            success,
        },
    );
}

async fn start_registered_oauth_flow<Build, Authorize, AuthFut>(
    app: tauri::AppHandle,
    provider_id: &'static str,
    log_label: &'static str,
    build: Build,
) -> Result<OauthChallenge, String>
where
    Build: FnOnce(Arc<dyn Fn(String, String) + Send + Sync>) -> Result<Authorize, String>,
    Authorize: Fn() -> AuthFut + Send + 'static,
    AuthFut: Future<Output = Result<(), String>> + Send + 'static,
{
    let provider = oauth_provider(provider_id)
        .ok_or_else(|| format!("Provider {provider_id} does not support OAuth"))?;
    start_device_code_oauth_flow(app, provider, log_label, build).await
}

async fn start_device_code_oauth_flow<Build, Authorize, AuthFut>(
    app: tauri::AppHandle,
    provider: &'static CredentialedProviderRegistration,
    log_label: &'static str,
    build: Build,
) -> Result<OauthChallenge, String>
where
    Build: FnOnce(Arc<dyn Fn(String, String) + Send + Sync>) -> Result<Authorize, String>,
    Authorize: Fn() -> AuthFut + Send + 'static,
    AuthFut: Future<Output = Result<(), String>> + Send + 'static,
{
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    };
    use tokio::sync::Notify;

    let challenge = Arc::new(Mutex::new(None::<OauthChallenge>));
    let challenge_shown = Arc::new(AtomicBool::new(false));
    let notify = Arc::new(Notify::new());
    let on_device_code: Arc<dyn Fn(String, String) + Send + Sync> = {
        let challenge = Arc::clone(&challenge);
        let challenge_shown = Arc::clone(&challenge_shown);
        let notify = Arc::clone(&notify);
        Arc::new(move |verification_url, user_code| {
            *challenge.lock().unwrap() = Some(OauthChallenge {
                verification_url,
                user_code,
            });
            challenge_shown.store(true, Ordering::SeqCst);
            notify.notify_one();
        })
    };

    let authorize = build(Arc::clone(&on_device_code))?;
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut retries = 0;
        let max_retries = 3;

        loop {
            match authorize().await {
                Ok(()) => {
                    emit_oauth_complete(&app_clone, provider, true);
                    break;
                }
                Err(e) => {
                    if challenge_shown.load(Ordering::SeqCst) {
                        eprintln!("{log_label} OAuth failed after device code was displayed: {e}");
                        emit_oauth_complete(&app_clone, provider, false);
                        break;
                    }
                    retries += 1;
                    if retries >= max_retries {
                        eprintln!("{log_label} OAuth failed after {retries} attempts: {e}");
                        emit_oauth_complete(&app_clone, provider, false);
                        break;
                    }
                    eprintln!("{log_label} OAuth attempt {retries} failed: {e}, retrying...");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
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
            eprintln!("Failed to open browser: {e}");
        }
        Ok(challenge)
    } else {
        Err("Failed to initiate OAuth flow".to_string())
    }
}
