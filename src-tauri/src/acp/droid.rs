//! Factory Droid CLI integration: binary discovery, version checks,
//! authentication flows, and model catalog discovery over ACP.
//!
//! Invokes the user's locally installed Factory Droid CLI over its official
//! Agent Client Protocol interface so Factory subscriptions (`device-pairing`)
//! and optional `FACTORY_API_KEY` overrides run through Factory's supported
//! CLI without touching private auth files or endpoints.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, LazyLock, Mutex as StdMutex};
use std::time::Duration;

use agent_client_protocol::{Error, ErrorCode, UntypedMessage};
use serde_json::{json, Value};

use super::client::{agent_config, connect, initialize_request, with_timeout, NoOpAcpHandler, AcpSessionHandler};
use super::dialect::{
    extract_auth_challenge_from_text, extract_auth_challenge_from_value, DialectAuthStatus,
    DialectSessionSetup, ExtractedAuthChallenge,
};
use crate::models::{ModelInfo, ModelVariant};
use crate::oauth::OauthChallenge;
use rig::completion::Effort;

pub const FACTORY_DROID_PROVIDER_ID: &str = "factory_droid";
const AUTH_PROBE_TIMEOUT: Duration = Duration::from_secs(6);
const MODEL_FETCH_TIMEOUT: Duration = Duration::from_secs(12);
const AUTH_FLOW_TIMEOUT: Duration = Duration::from_secs(300);

/// Minimum supported Droid CLI version. Older versions predate the ACP
/// session/config surface Gospel relies on (session capabilities, config
/// options, auth status).
pub const MINIMUM_DROID_VERSION: (u64, u64, u64) = (1, 21, 0);

const AUTH_UNKNOWN: u8 = 0;
const AUTH_UNAUTHENTICATED: u8 = 1;
const AUTH_AUTHENTICATED: u8 = 2;

static DROID_AUTH_CACHE: AtomicU8 = AtomicU8::new(AUTH_UNKNOWN);
static DROID_EFFORT_OVERRIDES: LazyLock<StdMutex<HashMap<String, Vec<String>>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

pub(crate) struct DroidCuratedModel {
    pub id: &'static str,
    pub efforts: &'static [&'static str],
}

/// Curated model catalog from `droid exec --help` (v1.21.0), used as fallback
/// before an online `session/new` listing succeeds or if live listing fails.
pub(crate) const FACTORY_DROID_CURATED_MODELS: &[DroidCuratedModel] = &[
    DroidCuratedModel { id: "auto", efforts: &[] },
    DroidCuratedModel { id: "claude-opus-5-5", efforts: &["low", "medium", "high", "xhigh", "max"] },
    DroidCuratedModel { id: "claude-opus-5-5-fast", efforts: &["low", "medium", "high", "xhigh", "max"] },
    DroidCuratedModel { id: "claude-opus-5", efforts: &["off", "low", "medium", "high", "xhigh", "max"] },
    DroidCuratedModel { id: "claude-sonnet-5", efforts: &["off", "low", "medium", "high", "xhigh", "max"] },
    DroidCuratedModel { id: "claude-opus-4-8", efforts: &["off", "low", "medium", "high", "xhigh", "max"] },
    DroidCuratedModel { id: "claude-opus-4-7", efforts: &["off", "low", "medium", "high", "xhigh", "max"] },
    DroidCuratedModel { id: "claude-opus-4-6", efforts: &["off", "low", "medium", "high", "max"] },
    DroidCuratedModel { id: "claude-sonnet-4-6", efforts: &["off", "low", "medium", "high", "max"] },
    DroidCuratedModel { id: "claude-haiku-4-5-20251001", efforts: &["off", "low", "medium", "high"] },
    DroidCuratedModel { id: "claude-fable-5.1", efforts: &["off", "low", "medium", "high", "xhigh", "max"] },
    DroidCuratedModel { id: "claude-fable-5", efforts: &["off", "low", "medium", "high", "xhigh", "max"] },
    DroidCuratedModel { id: "gpt-5.5", efforts: &["low", "medium", "high", "xhigh"] },
    DroidCuratedModel { id: "gpt-5.5-pro", efforts: &["medium", "high", "xhigh"] },
    DroidCuratedModel { id: "gpt-5.4", efforts: &["low", "medium", "high", "xhigh"] },
    DroidCuratedModel { id: "gpt-5.4-mini", efforts: &["low", "medium", "high", "xhigh"] },
    DroidCuratedModel { id: "gpt-5.3-codex", efforts: &["low", "medium", "high", "xhigh"] },
    DroidCuratedModel { id: "gpt-5.2", efforts: &["off", "low", "medium", "high", "xhigh"] },
    DroidCuratedModel { id: "gemini-3.1-pro-preview", efforts: &["low", "medium", "high"] },
    DroidCuratedModel { id: "gemini-3.8-flash", efforts: &["low", "medium", "high"] },
    DroidCuratedModel { id: "gemini-3.7-flash", efforts: &["low", "medium", "high"] },
    DroidCuratedModel { id: "gemini-3.5-flash", efforts: &["minimal", "low", "medium", "high"] },
    DroidCuratedModel { id: "gemini-3-flash-preview", efforts: &["minimal", "low", "medium", "high"] },
    DroidCuratedModel { id: "grok-4.7", efforts: &["low", "medium", "high", "xhigh"] },
    DroidCuratedModel { id: "grok-4.6", efforts: &["low", "medium", "high", "xhigh"] },
    DroidCuratedModel { id: "grok-4.5", efforts: &["low", "medium", "high"] },
    DroidCuratedModel { id: "glm-5.3", efforts: &["low", "high", "max"] },
    DroidCuratedModel { id: "glm-5.3-flash", efforts: &["low", "high", "max"] },
    DroidCuratedModel { id: "glm-5.2", efforts: &["off", "high", "max"] },
    DroidCuratedModel { id: "kimi-k3", efforts: &["off", "low", "high", "max"] },
    DroidCuratedModel { id: "qwen3.8-max", efforts: &["low", "medium", "xhigh"] },
    DroidCuratedModel { id: "deepseek-v4.1-flash", efforts: &["off", "low", "high", "max"] },
    DroidCuratedModel { id: "minimax-m3", efforts: &["high"] },
    DroidCuratedModel { id: "muse-spark-1.3", efforts: &["low", "medium", "high", "xhigh"] },
    DroidCuratedModel { id: "mistral-medium-3.5", efforts: &["off", "high"] },
    DroidCuratedModel { id: "inkling", efforts: &["off", "minimal", "low", "medium", "high", "xhigh", "max"] },
    DroidCuratedModel { id: "nemotron-3-ultra", efforts: &["off", "high"] },
];

pub fn set_cached_authenticated(authenticated: Option<bool>) {
    let val = match authenticated {
        None => AUTH_UNKNOWN,
        Some(false) => AUTH_UNAUTHENTICATED,
        Some(true) => AUTH_AUTHENTICATED,
    };
    DROID_AUTH_CACHE.store(val, Ordering::SeqCst);
}

fn cached_authenticated() -> Option<bool> {
    match DROID_AUTH_CACHE.load(Ordering::SeqCst) {
        AUTH_AUTHENTICATED => Some(true),
        AUTH_UNAUTHENTICATED => Some(false),
        _ => None,
    }
}

pub fn has_api_key_override() -> bool {
    effective_api_key(None).is_some()
}

/// Resolves the effective Factory API key: explicit argument, then the
/// provider credential store, then the process environment.
pub fn effective_api_key(explicit: Option<&str>) -> Option<String> {
    if let Some(key) = explicit.map(str::trim).filter(|k| !k.is_empty()) {
        return Some(key.to_string());
    }
    if let Ok(key) = crate::keychain::retrieve(FACTORY_DROID_PROVIDER_ID) {
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if let Ok(key) = std::env::var("FACTORY_API_KEY") {
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

pub fn is_credentialed_sync() -> bool {
    if has_api_key_override() && resolve_droid_binary().is_some() {
        return true;
    }
    cached_authenticated() == Some(true)
}

pub fn resolve_droid_binary() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("GOSPEL_DROID_PATH") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    let binary_name = if cfg!(target_os = "windows") {
        "droid.exe"
    } else {
        "droid"
    };

    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(binary_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    let mut fallbacks = Vec::new();
    if let Some(home) = dirs::home_dir() {
        fallbacks.push(home.join(".local").join("bin").join(binary_name));
    }
    #[cfg(not(target_os = "windows"))]
    {
        fallbacks.push(PathBuf::from("/opt/homebrew/bin").join(binary_name));
        fallbacks.push(PathBuf::from("/usr/local/bin").join(binary_name));
    }
    fallbacks.into_iter().find(|path| path.is_file())
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let token = text
        .split_whitespace()
        .find(|tok| tok.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let mut parts = token.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts
        .next()
        .unwrap_or("0")
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()?;
    Some((major, minor, patch))
}

/// Runs `droid --version` and returns the parsed version, warning in the
/// error string when the binary is older than `MINIMUM_DROID_VERSION`.
pub async fn check_droid_version(binary: &std::path::Path) -> Result<(u64, u64, u64), String> {
    if cfg!(test) && std::env::var_os("GOSPEL_DROID_PATH").is_none() {
        return Ok(MINIMUM_DROID_VERSION);
    }
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(binary).arg("--version").output(),
    )
    .await
    .map_err(|_| "droid --version timed out".to_string())
    .and_then(|res| res.map_err(|e| format!("droid --version failed: {e}")))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let version = parse_version(&text)
        .ok_or_else(|| format!("Could not parse droid version from: {}", text.trim()))?;
    if version < MINIMUM_DROID_VERSION {
        return Err(format!(
            "Factory Droid {}.{}.{} is too old; Gospel requires at least {}.{}.{}. Upgrade with: curl -fsSL https://app.factory.ai/cli | sh",
            version.0, version.1, version.2, MINIMUM_DROID_VERSION.0, MINIMUM_DROID_VERSION.1, MINIMUM_DROID_VERSION.2
        ));
    }
    Ok(version)
}

fn droid_acp_args(cwd: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "exec".to_string(),
        "--output-format".to_string(),
        "acp".to_string(),
    ];
    if let Some(cwd) = cwd {
        args.push("--cwd".to_string());
        args.push(cwd.to_string());
    }
    args
}

fn api_key_envs(api_key: Option<&str>) -> Vec<(String, String)> {
    effective_api_key(api_key)
        .map(|key| vec![("FACTORY_API_KEY".to_string(), key)])
        .unwrap_or_default()
}

fn untyped(method: &str, params: &Value) -> Result<UntypedMessage, Error> {
    UntypedMessage::new(method, params).map_err(|e| Error::new(ErrorCode::INTERNAL_ERROR, e.to_string()))
}

/// Probes `droid exec --output-format acp` via `initialize` + `auth/status`
/// and refreshes the in-process credential cache.
pub async fn probe_auth_status() -> bool {
    let Some(binary) = resolve_droid_binary() else {
        set_cached_authenticated(Some(false));
        return false;
    };

    if has_api_key_override() {
        set_cached_authenticated(Some(true));
        return true;
    }

    // In unit tests without an explicit mock binary, do not spawn external user binaries.
    if cfg!(test) && std::env::var_os("GOSPEL_DROID_PATH").is_none() {
        return cached_authenticated() == Some(true);
    }

    if check_droid_version(&binary).await.is_err() {
        set_cached_authenticated(Some(false));
        return false;
    }

    let result = connect(
        agent_config(&binary, &droid_acp_args(None), &[]),
        Arc::new(NoOpAcpHandler),
        None,
        move |connection| async move {
            with_timeout(
                "initialize",
                AUTH_PROBE_TIMEOUT,
                connection.send_request(initialize_request(false, false)).block_task(),
            )
            .await?;
            let status_value = with_timeout(
                "auth/status",
                AUTH_PROBE_TIMEOUT,
                connection.send_request(untyped("auth/status", &json!({}))?).block_task(),
            )
            .await?;
            let status: DialectAuthStatus = serde_json::from_value(status_value).unwrap_or_default();
            Ok(status.is_authenticated())
        },
    )
    .await;

    let authenticated = result.unwrap_or(false);
    set_cached_authenticated(Some(authenticated));
    authenticated
}

struct AuthFlowHandler {
    emitted: StdMutex<bool>,
    on_challenge: Arc<dyn Fn(OauthChallenge) + Send + Sync>,
}

impl AuthFlowHandler {
    fn maybe_emit(&self, challenge: ExtractedAuthChallenge) {
        let mut emitted = self.emitted.lock().unwrap_or_else(|p| p.into_inner());
        if *emitted {
            return;
        }
        *emitted = true;
        let payload = match challenge {
            ExtractedAuthChallenge::DeviceCode {
                verification_url,
                user_code,
            } => OauthChallenge::DeviceCode {
                verification_url,
                user_code,
            },
            ExtractedAuthChallenge::Browser { authorize_url } => OauthChallenge::Browser {
                authorize_url,
                browser_launched: true,
            },
        };
        (self.on_challenge)(payload);
    }

    fn emit_fallback_if_needed(&self) {
        let mut emitted = self.emitted.lock().unwrap_or_else(|p| p.into_inner());
        if *emitted {
            return;
        }
        *emitted = true;
        (self.on_challenge)(OauthChallenge::Browser {
            authorize_url: "https://app.factory.ai/device".to_string(),
            browser_launched: true,
        });
    }
}

#[async_trait::async_trait]
impl AcpSessionHandler for AuthFlowHandler {
    async fn on_session_update(
        &self,
        notification: &agent_client_protocol::v1::schema::SessionNotification,
    ) {
        if let Ok(value) = serde_json::to_value(&notification.update) {
            if let Some(challenge) = extract_auth_challenge_from_value(&value) {
                self.maybe_emit(challenge);
            }
        }
    }
}

pub async fn authenticate_device_pairing<F>(on_challenge: F) -> Result<(), String>
where
    F: Fn(OauthChallenge) + Send + Sync + 'static,
{
    let binary = resolve_droid_binary().ok_or_else(|| {
        "Factory Droid CLI was not found. Install droid (curl -fsSL https://app.factory.ai/cli | sh) or set GOSPEL_DROID_PATH."
            .to_string()
    })?;
    check_droid_version(&binary).await?;

    let handler = Arc::new(AuthFlowHandler {
        emitted: StdMutex::new(false),
        on_challenge: Arc::new(on_challenge),
    });
    let stderr_handler = Arc::clone(&handler);
    let stderr_cb: super::client::StderrLineCallback = Arc::new(move |line| {
        if let Some(challenge) = extract_auth_challenge_from_text(line) {
            stderr_handler.maybe_emit(challenge);
        }
    });

    let result = connect(
        agent_config(&binary, &droid_acp_args(None), &[]),
        handler.clone(),
        Some(stderr_cb),
        move |connection| {
            let handler = handler.clone();
            async move {
                let init = with_timeout(
                    "initialize",
                    AUTH_PROBE_TIMEOUT,
                    connection.send_request(initialize_request(false, false)).block_task(),
                )
                .await?;

                if !init
                    .auth_methods
                    .iter()
                    .any(|method| method.id().to_string() == "device-pairing")
                {
                    return Err(Error::new(
                        ErrorCode::INTERNAL_ERROR,
                        "Installed Factory Droid CLI does not advertise device-pairing login.",
                    ));
                }

                // If Droid already holds a valid session, short-circuit cleanly.
                if let Ok(status_value) = with_timeout(
                    "auth/status",
                    AUTH_PROBE_TIMEOUT,
                    connection.send_request(untyped("auth/status", &json!({}))?).block_task(),
                )
                .await
                {
                    let status: DialectAuthStatus =
                        serde_json::from_value(status_value).unwrap_or_default();
                    if status.is_authenticated() {
                        handler.emit_fallback_if_needed();
                        return Ok(true);
                    }
                }

                // Fallback prompt after 2s in case Droid opens the browser
                // directly without emitting a challenge notification.
                let fallback_handler = Arc::clone(&handler);
                let fallback_task = tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    fallback_handler.emit_fallback_if_needed();
                });

                let auth_result = with_timeout(
                    "authenticate",
                    AUTH_FLOW_TIMEOUT,
                    connection.send_request(
                        untyped("authenticate", &json!({ "methodId": "device-pairing" }))?,
                    ).block_task(),
                )
                .await;

                fallback_task.abort();

                match auth_result {
                    Ok(value) => {
                        if let Some(challenge) = extract_auth_challenge_from_value(&value) {
                            handler.maybe_emit(challenge);
                        }
                        let confirmed = with_timeout(
                            "auth/status",
                            AUTH_PROBE_TIMEOUT,
                            connection.send_request(untyped("auth/status", &json!({}))?).block_task(),
                        )
                        .await
                        .map(|v| {
                            serde_json::from_value::<DialectAuthStatus>(v)
                                .unwrap_or_default()
                                .is_authenticated()
                        })
                        .unwrap_or(true);
                        if confirmed {
                            Ok(true)
                        } else {
                            Err(Error::new(
                                ErrorCode::INTERNAL_ERROR,
                                "Factory Droid sign-in did not complete.",
                            ))
                        }
                    }
                    Err(err) => Err(err),
                }
            }
        },
    )
    .await;

    match result {
        Ok(true) => {
            set_cached_authenticated(Some(true));
            Ok(())
        }
        Ok(false) => unreachable!(),
        Err(err) => {
            set_cached_authenticated(Some(false));
            Err(format!("Factory Droid sign-in failed: {err}"))
        }
    }
}

pub async fn logout() -> Result<(), String> {
    set_cached_authenticated(Some(false));
    let Some(binary) = resolve_droid_binary() else {
        return Ok(());
    };
    if cfg!(test) && std::env::var_os("GOSPEL_DROID_PATH").is_none() {
        return Ok(());
    }

    let result = connect(
        agent_config(&binary, &droid_acp_args(None), &[]),
        Arc::new(NoOpAcpHandler),
        None,
        move |connection| async move {
            let _ = with_timeout(
                "initialize",
                AUTH_PROBE_TIMEOUT,
                connection.send_request(initialize_request(false, false)).block_task(),
            )
            .await;
            with_timeout(
                "auth/logout",
                AUTH_PROBE_TIMEOUT,
                connection.send_request(untyped("auth/logout", &json!({}))?).block_task(),
            )
            .await?;
            Ok(())
        },
    )
    .await;

    match result {
        Ok(()) => Ok(()),
        Err(err) if err.code == ErrorCode::AUTH_REQUIRED => Ok(()),
        Err(err) => Err(format!("Factory Droid logout failed: {err}")),
    }
}

pub fn effort_levels_for_model(model: &str) -> Option<Vec<Effort>> {
    if let Some(levels) = DROID_EFFORT_OVERRIDES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(model)
        .cloned()
    {
        return Some(levels.iter().filter_map(|s| parse_droid_effort(s)).collect());
    }

    FACTORY_DROID_CURATED_MODELS
        .iter()
        .find(|entry| entry.id == model)
        .map(|entry| {
            entry
                .efforts
                .iter()
                .filter_map(|s| parse_droid_effort(s))
                .collect()
        })
}

fn parse_droid_effort(raw: &str) -> Option<Effort> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "minimal" => Some(Effort::Minimal),
        "low" => Some(Effort::Low),
        "medium" => Some(Effort::Medium),
        "high" => Some(Effort::High),
        "xhigh" => Some(Effort::XHigh),
        "max" => Some(Effort::Max),
        _ => None,
    }
}

pub fn curated_model_infos() -> Vec<ModelInfo> {
    FACTORY_DROID_CURATED_MODELS
        .iter()
        .map(|entry| model_info_with_variants(entry.id, entry.efforts))
        .collect()
}

pub(crate) fn model_info_with_variants(model_id: &str, efforts: &[&str]) -> ModelInfo {
    let variants = efforts
        .iter()
        .filter_map(|raw| parse_droid_effort(raw))
        .map(variant_for_effort)
        .collect::<Vec<_>>();
    ModelInfo {
        model: model_id.to_string(),
        provider: FACTORY_DROID_PROVIDER_ID.to_string(),
        variants,
        copilot_supported_endpoints: None,
    }
}

pub(crate) fn variant_for_effort(effort: Effort) -> ModelVariant {
    let label = match effort {
        Effort::Minimal => "Minimal",
        Effort::Low => "Low",
        Effort::Medium => "Medium",
        Effort::High => "High",
        Effort::XHigh => "Extra High",
        Effort::Max => "Max",
        _ => "Reasoning",
    };
    let id = if effort == Effort::XHigh {
        "reasoning-extra-high".to_string()
    } else {
        format!("reasoning-{}", effort.as_str())
    };
    ModelVariant {
        id,
        name: format!("Reasoning: {label}"),
        description: format!("Request {label} reasoning effort via Factory Droid ACP"),
        deprecated: false,
    }
}

pub async fn fetch_models(api_key: Option<&str>) -> Result<Vec<ModelInfo>, String> {
    let Some(binary) = resolve_droid_binary() else {
        return Err("Factory Droid CLI not installed.".to_string());
    };

    if cfg!(test) && std::env::var_os("GOSPEL_DROID_PATH").is_none() {
        return Ok(curated_model_infos());
    }

    check_droid_version(&binary).await?;

    let cwd = std::env::temp_dir();
    let cwd_str = cwd.to_string_lossy().to_string();
    let envs = api_key_envs(api_key);

    let result = connect(
        agent_config(&binary, &droid_acp_args(Some(&cwd_str)), &envs),
        Arc::new(NoOpAcpHandler),
        None,
        move |connection| async move {
            with_timeout(
                "initialize",
                MODEL_FETCH_TIMEOUT,
                connection.send_request(initialize_request(false, false)).block_task(),
            )
            .await?;
            let params = json!({ "cwd": cwd_str, "mcpServers": Vec::<Value>::new() });
            let value = with_timeout(
                "session/new",
                MODEL_FETCH_TIMEOUT,
                connection.send_request(untyped("session/new", &params)?).block_task(),
            )
            .await?;
            let setup = DialectSessionSetup::parse(&value);
            if let Some(session_id) = setup.session_id.clone() {
                let close_params = json!({ "sessionId": session_id });
                if let Ok(msg) = untyped("session/close", &close_params) {
                    let _ = with_timeout(
                        "session/close",
                        Duration::from_secs(3),
                        connection.send_request(msg).block_task(),
                    )
                    .await;
                }
            }
            Ok(setup)
        },
    )
    .await;

    match result {
        Ok(setup) => {
            set_cached_authenticated(Some(true));
            let models = extract_models_from_session(&setup);
            if models.is_empty() {
                Ok(curated_model_infos())
            } else {
                Ok(models)
            }
        }
        Err(err) if err.code == ErrorCode::AUTH_REQUIRED => {
            set_cached_authenticated(Some(false));
            Err("Factory Droid session expired or unauthenticated; sign in again.".to_string())
        }
        Err(err) => {
            tracing::warn!(
                "Factory Droid live model discovery failed ({err}); falling back to curated catalog"
            );
            Ok(curated_model_infos())
        }
    }
}

fn extract_models_from_session(setup: &DialectSessionSetup) -> Vec<ModelInfo> {
    let mut effort_choices: Vec<String> = Vec::new();
    for opt in &setup.config_options {
        if opt.category.as_deref() == Some("thought_level") || opt.id == "reasoning_effort" {
            effort_choices = opt
                .all_choices()
                .into_iter()
                .map(|c| c.value.clone())
                .filter(|v| !v.is_empty())
                .collect();
        }
    }

    // Prefer per-model efforts advertised through the `models` metadata (the
    // authoritative Factory extension); fall back to the config-option model
    // list and finally to the curated catalog.
    let mut per_model: HashMap<String, Vec<String>> = HashMap::new();
    let mut ordered_ids: Vec<String> = Vec::new();
    if let Some(models) = &setup.models {
        for model in &models.available_models {
            if model.model_id.is_empty() || ordered_ids.contains(&model.model_id) {
                continue;
            }
            ordered_ids.push(model.model_id.clone());
            let efforts = model.supported_reasoning_efforts();
            if !efforts.is_empty() {
                per_model.insert(model.model_id.clone(), efforts);
            }
        }
    }
    if ordered_ids.is_empty() {
        for opt in &setup.config_options {
            if opt.category.as_deref() == Some("model") || opt.id == "model" {
                for choice in opt.all_choices() {
                    if !choice.value.is_empty() && !ordered_ids.contains(&choice.value) {
                        ordered_ids.push(choice.value.clone());
                    }
                }
            }
        }
    }

    if !per_model.is_empty() {
        let mut overrides = DROID_EFFORT_OVERRIDES
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        for (id, efforts) in &per_model {
            overrides.insert(id.clone(), efforts.clone());
        }
    }

    ordered_ids
        .into_iter()
        .map(|id| {
            if let Some(efforts) = per_model.get(&id) {
                let refs: Vec<&str> = efforts.iter().map(String::as_str).collect();
                return model_info_with_variants(&id, &refs);
            }
            let curated_efforts = FACTORY_DROID_CURATED_MODELS
                .iter()
                .find(|m| m.id == id)
                .map(|m| m.efforts);
            match curated_efforts {
                Some(efforts) => model_info_with_variants(&id, efforts),
                None => {
                    let refs: Vec<&str> = effort_choices.iter().map(String::as_str).collect();
                    model_info_with_variants(&id, &refs)
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("1.21.0"), Some((1, 21, 0)));
        assert_eq!(parse_version("droid 1.21.0"), Some((1, 21, 0)));
        assert_eq!(parse_version("1.21"), Some((1, 21, 0)));
        assert_eq!(parse_version("garbage"), None);
    }

    #[test]
    fn minimum_version_comparison() {
        assert!((1, 21, 0) >= MINIMUM_DROID_VERSION);
        assert!((1, 20, 9) < MINIMUM_DROID_VERSION);
        assert!((2, 0, 0) >= MINIMUM_DROID_VERSION);
    }

    #[test]
    fn extracts_models_preferring_meta_efforts() {
        let value = json!({
            "sessionId": "s1",
            "configOptions": [
                {"id": "reasoning_effort", "category": "thought_level", "type": "select",
                 "currentValue": "high", "options": [{"value": "low"}, {"value": "high"}]}
            ],
            "models": {"availableModels": [
                {"modelId": "new-model-x", "name": "X", "_meta": {"factory": {"supportedReasoningEfforts": ["off", "low"]}}},
                {"modelId": "claude-opus-5-5", "name": "Opus"}
            ]}
        });
        let setup = DialectSessionSetup::parse(&value);
        let models = extract_models_from_session(&setup);
        assert_eq!(models.len(), 2);
        let x = &models[0];
        assert_eq!(x.model, "new-model-x");
        assert_eq!(x.variants.len(), 1); // "off" is not a reasoning variant; only "low" parses
    }
}
