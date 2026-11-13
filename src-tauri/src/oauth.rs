use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;

use crate::providers::{CredentialedProviderRegistration, ProviderAuthKind};

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum OauthChallenge {
    Browser {
        authorize_url: String,
        browser_launched: bool,
    },
    DeviceCode {
        verification_url: String,
        user_code: String,
    },
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
        let browser_app = app.clone();
        start_registered_oauth_flow(app, "chatgpt", "ChatGPT", move |on_prompt| {
            let authenticator = Arc::new(crate::provider_client::chatgpt_authenticator());
            let browser_app = browser_app.clone();
            Ok(move || {
                let authenticator = Arc::clone(&authenticator);
                let on_prompt = Arc::clone(&on_prompt);
                let browser_app = browser_app.clone();
                async move {
                    // Use browser PKCE directly; some workspaces block device-code sign-in.
                    authenticator
                        .sign_in_with_browser(&rig::rig_reqwest::shared(), move |prompt| {
                            let browser_launched = prompt.browser_launched
                                || browser_app
                                    .opener()
                                    .open_url(&prompt.authorize_url, None::<String>)
                                    .is_ok();
                            on_prompt(OauthChallenge::Browser {
                                authorize_url: prompt.authorize_url,
                                browser_launched,
                            });
                        })
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                }
            })
        })
        .await
    })
}

fn start_grok(
    app: tauri::AppHandle,
) -> Pin<Box<dyn Future<Output = Result<OauthChallenge, String>> + Send>> {
    Box::pin(async move {
        start_registered_oauth_flow(app, "grok", "Grok", |on_prompt| {
            Ok(move || {
                let on_prompt = Arc::clone(&on_prompt);
                async move {
                    crate::grok_oauth::login(
                        &crate::grok_oauth::ReqwestPoster,
                        &crate::grok_oauth::Endpoints::default(),
                        &crate::keychain::grok_auth_file_path(),
                        |url, code| {
                            on_prompt(OauthChallenge::DeviceCode {
                                verification_url: url,
                                user_code: code,
                            })
                        },
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
        start_registered_oauth_flow(app, "github_copilot", "GitHub Copilot", |on_prompt| {
            let handler = rig::providers::copilot::auth::DeviceCodeHandler::new(move |prompt| {
                on_prompt(OauthChallenge::DeviceCode {
                    verification_url: prompt.verification_uri,
                    user_code: prompt.user_code,
                });
            });
            let authenticator =
                Arc::new(crate::provider_client::copilot_authenticator(Some(handler)));
            Ok(move || {
                let authenticator = Arc::clone(&authenticator);
                async move {
                    authenticator
                        .auth_context(&rig::rig_reqwest::shared())
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                }
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
    Build: FnOnce(Arc<dyn Fn(OauthChallenge) + Send + Sync>) -> Result<Authorize, String>,
    Authorize: Fn() -> AuthFut + Send + 'static,
    AuthFut: Future<Output = Result<(), String>> + Send + 'static,
{
    let provider = oauth_provider(provider_id)
        .ok_or_else(|| format!("Provider {provider_id} does not support OAuth"))?;
    start_interactive_oauth_flow(app, provider, log_label, build).await
}

async fn start_interactive_oauth_flow<Build, Authorize, AuthFut>(
    app: tauri::AppHandle,
    provider: &'static CredentialedProviderRegistration,
    log_label: &'static str,
    build: Build,
) -> Result<OauthChallenge, String>
where
    Build: FnOnce(Arc<dyn Fn(OauthChallenge) + Send + Sync>) -> Result<Authorize, String>,
    Authorize: Fn() -> AuthFut + Send + 'static,
    AuthFut: Future<Output = Result<(), String>> + Send + 'static,
{
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    };

    let challenge_shown = Arc::new(AtomicBool::new(false));
    let (challenge_sender, challenge_receiver) = tokio::sync::oneshot::channel();
    let challenge_sender = Arc::new(Mutex::new(Some(challenge_sender)));
    let on_prompt: Arc<dyn Fn(OauthChallenge) + Send + Sync> = {
        let challenge_sender = Arc::clone(&challenge_sender);
        let challenge_shown = Arc::clone(&challenge_shown);
        Arc::new(move |challenge| {
            challenge_shown.store(true, Ordering::SeqCst);
            if let Some(sender) = challenge_sender.lock().unwrap().take() {
                let _ = sender.send(Ok(challenge));
            }
        })
    };

    let authorize = build(Arc::clone(&on_prompt))?;
    let app_clone = app.clone();
    let challenge_sender_for_task = Arc::clone(&challenge_sender);
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
                        eprintln!("{log_label} OAuth failed after sign-in was started: {e}");
                        emit_oauth_complete(&app_clone, provider, false);
                        break;
                    }
                    retries += 1;
                    if retries >= max_retries {
                        eprintln!("{log_label} OAuth failed after {retries} attempts: {e}");
                        if let Some(sender) = challenge_sender_for_task.lock().unwrap().take() {
                            let _ = sender.send(Err(e));
                        }
                        emit_oauth_complete(&app_clone, provider, false);
                        break;
                    }
                    eprintln!("{log_label} OAuth attempt {retries} failed: {e}, retrying...");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    });

    let challenge = tokio::time::timeout(std::time::Duration::from_secs(30), challenge_receiver)
        .await
        .map_err(|_| "OAuth flow timed out before receiving a sign-in prompt".to_string())?
        .map_err(|_| "OAuth flow ended before producing a sign-in prompt".to_string())??;

    if let OauthChallenge::DeviceCode {
        verification_url, ..
    } = &challenge
    {
        if let Err(e) = app.opener().open_url(verification_url, None::<String>) {
            eprintln!("Failed to open browser: {e}");
        }
    }

    Ok(challenge)
}

#[cfg(test)]
mod tests {
    use super::OauthChallenge;

    #[test]
    fn oauth_prompts_serialize_for_the_frontend() {
        assert_eq!(
            serde_json::to_value(OauthChallenge::Browser {
                authorize_url: "https://auth.example/sign-in".into(),
                browser_launched: true,
            })
            .unwrap(),
            serde_json::json!({
                "method": "browser",
                "authorize_url": "https://auth.example/sign-in",
                "browser_launched": true,
            })
        );
        assert_eq!(
            serde_json::to_value(OauthChallenge::DeviceCode {
                verification_url: "https://auth.example/device".into(),
                user_code: "CODE".into(),
            })
            .unwrap(),
            serde_json::json!({
                "method": "device_code",
                "verification_url": "https://auth.example/device",
                "user_code": "CODE",
            })
        );
    }
}
