use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;

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

pub type OauthStart =
    fn(tauri::AppHandle) -> Pin<Box<dyn Future<Output = Result<OauthChallenge, String>> + Send>>;

pub struct OauthProviderRegistration {
    pub id: &'static str,
    pub display_name: &'static str,
    pub start: OauthStart,
    pub has_session: fn() -> bool,
    pub delete_session: fn() -> Result<(), crate::keychain::KeychainError>,
}

pub const OAUTH_PROVIDERS: &[OauthProviderRegistration] = &[
    OauthProviderRegistration {
        id: "chatgpt",
        display_name: "ChatGPT Plus/Pro",
        start: start_chatgpt,
        has_session: crate::keychain::has_chatgpt_oauth_session,
        delete_session: crate::keychain::delete_chatgpt_auth_file,
    },
    OauthProviderRegistration {
        id: "github_copilot",
        display_name: "GitHub Copilot",
        start: start_github_copilot,
        has_session: crate::keychain::has_github_copilot_oauth_session,
        delete_session: crate::keychain::delete_github_copilot_auth_files,
    },
];

pub fn oauth_provider(id: &str) -> Option<&'static OauthProviderRegistration> {
    OAUTH_PROVIDERS.iter().find(|provider| provider.id == id)
}

pub fn oauth_provider_ids() -> Vec<&'static str> {
    OAUTH_PROVIDERS.iter().map(|provider| provider.id).collect()
}

pub async fn start_provider_oauth(
    app: tauri::AppHandle,
    provider: &str,
) -> Result<OauthChallenge, String> {
    let entry = oauth_provider(provider)
        .ok_or_else(|| format!("Provider {} does not support OAuth", provider))?;
    (entry.start)(app).await
}

fn start_chatgpt(
    app: tauri::AppHandle,
) -> Pin<Box<dyn Future<Output = Result<OauthChallenge, String>> + Send>> {
    Box::pin(start_chatgpt_oauth_flow(app, "chatgpt-auth-complete"))
}

fn start_github_copilot(
    app: tauri::AppHandle,
) -> Pin<Box<dyn Future<Output = Result<OauthChallenge, String>> + Send>> {
    Box::pin(start_github_copilot_oauth_flow(
        app,
        "github-copilot-auth-complete",
    ))
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

    let client = rig::providers::chatgpt::Client::builder()
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

    let client = rig::providers::copilot::Client::builder()
        .oauth()
        .token_dir(crate::keychain::github_copilot_token_dir())
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
