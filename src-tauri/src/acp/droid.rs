//! Factory Droid ACP integration (`droid exec --output-format acp`).
//!
//! Invokes the user's locally installed Factory Droid CLI over its official
//! Agent Client Protocol (stdio NDJSON JSON-RPC 2.0) so Factory subscriptions
//! (`device-pairing`) and optional `FACTORY_API_KEY` overrides run through
//! Factory's supported CLI without touching private auth files or endpoints.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, LazyLock, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use rig::completion::Effort;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::client::{AcpClient, AcpClientHandler, AcpTransportError, NoOpAcpHandler};
use super::protocol::{
    extract_auth_challenge_from_text, extract_auth_challenge_from_value, AgentCapabilities,
    AuthStatusResult, AuthenticateParams, ClientCapabilities, ClientInfo, ExtractedAuthChallenge,
    FsCapabilities, InitializeParams, InitializeResult, ParsedSessionUpdate, PermissionOption,
    PermissionOutcome, PromptContentBlock, ReadTextFileParams, ReadTextFileResult,
    RequestPermissionParams, RequestPermissionResult, SessionCancelParams, SessionCloseParams,
    SessionConfigOption, SessionForkParams, SessionForkResult, SessionLoadParams, SessionNewParams,
    SessionNewResult, SessionPromptParams, SessionPromptResult, SessionSetConfigOptionParams,
    SessionSetModelParams, SessionUpdateNotification, WriteTextFileParams, ACP_PROTOCOL_VERSION,
};
use crate::approval_broker::{
    ApprovalBroker, ApprovalDecision, ApprovalOutcome, ApprovalRequest,
};
use crate::harness_profile::ActiveWorkspaceContext;
use crate::llm::{LlmError, ReasoningPayload, ReasoningPhase, StreamCompletionResult, StreamEvent};
use crate::models::{ModelInfo, ModelVariant};
use crate::oauth::OauthChallenge;
use crate::shell_tools::{CommandPolicy, CommandRisk, CommandSafety};
use crate::symlink_guard;
use crate::text_utils::truncate_text_bytes;

pub const FACTORY_DROID_PROVIDER_ID: &str = "factory_droid";
const AUTH_PROBE_TIMEOUT: Duration = Duration::from_secs(6);
#[cfg_attr(test, allow(dead_code))]
const MODEL_FETCH_TIMEOUT: Duration = Duration::from_secs(12);
const AUTH_FLOW_TIMEOUT: Duration = Duration::from_secs(300);
const SESSION_OP_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_FS_READ_BYTES: usize = 2 * 1024 * 1024;
const MAX_FS_WRITE_BYTES: usize = 4 * 1024 * 1024;
const MAX_TOOL_RESULT_DISPLAY_BYTES: usize = 24 * 1024;

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
    DroidCuratedModel {
        id: "claude-opus-5-5",
        efforts: &["low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "claude-opus-5-5-fast",
        efforts: &["low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "claude-opus-5",
        efforts: &["off", "low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "claude-sonnet-5",
        efforts: &["off", "low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "claude-opus-4-8",
        efforts: &["off", "low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "claude-opus-4-7",
        efforts: &["off", "low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "claude-opus-4-6",
        efforts: &["off", "low", "medium", "high", "max"],
    },
    DroidCuratedModel {
        id: "claude-sonnet-4-6",
        efforts: &["off", "low", "medium", "high", "max"],
    },
    DroidCuratedModel {
        id: "claude-haiku-4-5-20251001",
        efforts: &["off", "low", "medium", "high"],
    },
    DroidCuratedModel {
        id: "claude-fable-5.1",
        efforts: &["off", "low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "claude-fable-5",
        efforts: &["off", "low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "gpt-6-astra",
        efforts: &["low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "gpt-6.1-sol",
        efforts: &["low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "gpt-6.1-sol-ultrafast",
        efforts: &["low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "gpt-6-sol",
        efforts: &["none", "low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "gpt-6-luna",
        efforts: &["none", "low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "gpt-5.6-sol",
        efforts: &["none", "low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "gpt-5.6-luna",
        efforts: &["none", "low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "gpt-5.6-terra",
        efforts: &["none", "low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "gpt-5.5",
        efforts: &["low", "medium", "high", "xhigh"],
    },
    DroidCuratedModel {
        id: "gpt-5.5-pro",
        efforts: &["medium", "high", "xhigh"],
    },
    DroidCuratedModel {
        id: "gpt-5.4",
        efforts: &["low", "medium", "high", "xhigh"],
    },
    DroidCuratedModel {
        id: "gpt-5.4-mini",
        efforts: &["low", "medium", "high", "xhigh"],
    },
    DroidCuratedModel {
        id: "gpt-5.3-codex",
        efforts: &["low", "medium", "high", "xhigh"],
    },
    DroidCuratedModel {
        id: "gpt-5.2",
        efforts: &["off", "low", "medium", "high", "xhigh"],
    },
    DroidCuratedModel {
        id: "gemini-3.1-pro-preview",
        efforts: &["low", "medium", "high"],
    },
    DroidCuratedModel {
        id: "gemini-3.8-flash",
        efforts: &["low", "medium", "high"],
    },
    DroidCuratedModel {
        id: "gemini-3.7-flash",
        efforts: &["low", "medium", "high"],
    },
    DroidCuratedModel {
        id: "gemini-3.5-flash",
        efforts: &["minimal", "low", "medium", "high"],
    },
    DroidCuratedModel {
        id: "gemini-3-flash-preview",
        efforts: &["minimal", "low", "medium", "high"],
    },
    DroidCuratedModel {
        id: "grok-4.7",
        efforts: &["low", "medium", "high", "xhigh"],
    },
    DroidCuratedModel {
        id: "grok-4.6",
        efforts: &["low", "medium", "high", "xhigh"],
    },
    DroidCuratedModel {
        id: "grok-4.5",
        efforts: &["low", "medium", "high"],
    },
    DroidCuratedModel {
        id: "glm-5.3",
        efforts: &["low", "high", "max"],
    },
    DroidCuratedModel {
        id: "glm-5.3-flash",
        efforts: &["low", "high", "max"],
    },
    DroidCuratedModel {
        id: "glm-5.2",
        efforts: &["off", "high", "max"],
    },
    DroidCuratedModel {
        id: "kimi-k3",
        efforts: &["off", "low", "high", "max"],
    },
    DroidCuratedModel {
        id: "qwen3.8-max",
        efforts: &["low", "medium", "xhigh"],
    },
    DroidCuratedModel {
        id: "deepseek-v4.1-flash",
        efforts: &["off", "low", "high", "max"],
    },
    DroidCuratedModel {
        id: "minimax-m3",
        efforts: &["high"],
    },
    DroidCuratedModel {
        id: "muse-spark-1.3",
        efforts: &["low", "medium", "high", "xhigh"],
    },
    DroidCuratedModel {
        id: "mistral-medium-3.5",
        efforts: &["off", "high"],
    },
    DroidCuratedModel {
        id: "inkling",
        efforts: &["off", "minimal", "low", "medium", "high", "xhigh", "max"],
    },
    DroidCuratedModel {
        id: "nemotron-3-ultra",
        efforts: &["off", "high"],
    },
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

pub fn effective_api_key(explicit: Option<&str>) -> Option<String> {
    if let Some(key) = explicit.map(str::trim).filter(|k| !k.is_empty()) {
        return Some(key.to_string());
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

fn initialize_params(workspace: Option<&Path>) -> InitializeParams {
    let allow_fs = workspace.is_some();
    InitializeParams {
        protocol_version: ACP_PROTOCOL_VERSION,
        client_capabilities: ClientCapabilities {
            fs: FsCapabilities {
                read_text_file: allow_fs,
                write_text_file: allow_fs,
            },
            terminal: false,
        },
        client_info: ClientInfo {
            name: "gospel".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
    }
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

    let handler: Arc<dyn AcpClientHandler> = Arc::new(NoOpAcpHandler);
    let Ok(client) = AcpClient::spawn(
        &binary,
        &["exec", "--output-format", "acp"],
        None,
        &[],
        handler,
        None,
    ) else {
        set_cached_authenticated(Some(false));
        return false;
    };

    let authenticated = async {
        let _init: InitializeResult = client
            .request(
                "initialize",
                &initialize_params(None),
                Some(AUTH_PROBE_TIMEOUT),
            )
            .await
            .ok()?;
        let status: AuthStatusResult = client
            .request("auth/status", &json!({}), Some(AUTH_PROBE_TIMEOUT))
            .await
            .ok()?;
        Some(status.is_authenticated())
    }
    .await
    .unwrap_or(false);

    client.shutdown().await;
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

#[async_trait]
impl AcpClientHandler for AuthFlowHandler {
    async fn on_session_update(&self, notification: SessionUpdateNotification) {
        if let Some(challenge) = extract_auth_challenge_from_value(&notification.update) {
            self.maybe_emit(challenge);
        }
    }

    async fn on_notification(&self, _method: &str, params: &Value) {
        if let Some(challenge) = extract_auth_challenge_from_value(params) {
            self.maybe_emit(challenge);
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

    let handler = Arc::new(AuthFlowHandler {
        emitted: StdMutex::new(false),
        on_challenge: Arc::new(on_challenge),
    });
    let stderr_handler = Arc::clone(&handler);
    let stderr_cb: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line| {
        if let Some(challenge) = extract_auth_challenge_from_text(line) {
            stderr_handler.maybe_emit(challenge);
        }
    });

    let client = AcpClient::spawn(
        &binary,
        &["exec", "--output-format", "acp"],
        None,
        &[],
        Arc::clone(&handler) as Arc<dyn AcpClientHandler>,
        Some(stderr_cb),
    )
    .map_err(|e| e.to_string())?;

    let init: InitializeResult = client
        .request(
            "initialize",
            &initialize_params(None),
            Some(AUTH_PROBE_TIMEOUT),
        )
        .await
        .map_err(|e| format!("Factory Droid ACP initialize failed: {e}"))?;

    if !init
        .auth_methods
        .iter()
        .any(|method| method.id == "device-pairing")
    {
        client.shutdown().await;
        return Err("Installed Factory Droid CLI does not advertise device-pairing login.".into());
    }

    // If Droid already holds a valid session, short-circuit cleanly.
    if let Ok(status) = client
        .request::<_, AuthStatusResult>("auth/status", &json!({}), Some(AUTH_PROBE_TIMEOUT))
        .await
    {
        if status.is_authenticated() {
            set_cached_authenticated(Some(true));
            handler.emit_fallback_if_needed();
            client.shutdown().await;
            return Ok(());
        }
    }

    // Fallback prompt after 2s in case Droid opens the browser directly without emitting a challenge notification.
    let fallback_handler = Arc::clone(&handler);
    let fallback_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        fallback_handler.emit_fallback_if_needed();
    });

    let auth_res: Result<Value, AcpTransportError> = client
        .request(
            "authenticate",
            &AuthenticateParams {
                method_id: "device-pairing".to_string(),
            },
            Some(AUTH_FLOW_TIMEOUT),
        )
        .await;

    fallback_task.abort();

    let result = match auth_res {
        Ok(val) => {
            if let Some(challenge) = extract_auth_challenge_from_value(&val) {
                handler.maybe_emit(challenge);
            }
            let confirmed = client
                .request::<_, AuthStatusResult>("auth/status", &json!({}), Some(AUTH_PROBE_TIMEOUT))
                .await
                .map(|s| s.is_authenticated())
                .unwrap_or(true);
            if confirmed {
                set_cached_authenticated(Some(true));
                Ok(())
            } else {
                set_cached_authenticated(Some(false));
                Err("Factory Droid sign-in did not complete.".to_string())
            }
        }
        Err(err) => {
            set_cached_authenticated(Some(false));
            Err(format!("Factory Droid sign-in failed: {err}"))
        }
    };

    client.shutdown().await;
    result
}

pub async fn logout() -> Result<(), String> {
    set_cached_authenticated(Some(false));
    let Some(binary) = resolve_droid_binary() else {
        return Ok(());
    };
    if cfg!(test) && std::env::var_os("GOSPEL_DROID_PATH").is_none() {
        return Ok(());
    }
    let handler: Arc<dyn AcpClientHandler> = Arc::new(NoOpAcpHandler);
    let client = AcpClient::spawn(
        &binary,
        &["exec", "--output-format", "acp"],
        None,
        &[],
        handler,
        None,
    )
    .map_err(|e| e.to_string())?;

    let _ = client
        .request::<_, InitializeResult>(
            "initialize",
            &initialize_params(None),
            Some(AUTH_PROBE_TIMEOUT),
        )
        .await;
    let logout_res: Result<Value, AcpTransportError> = client
        .request("auth/logout", &json!({}), Some(AUTH_PROBE_TIMEOUT))
        .await;
    client.shutdown().await;
    if let Err(AcpTransportError::Rpc(err)) = logout_res {
        // Already logged out / unauthenticated is idempotent.
        if err.code != -32000 {
            return Err(format!("Factory Droid logout failed: {err}"));
        }
    }
    Ok(())
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

#[cfg_attr(test, allow(dead_code))]
pub fn curated_model_infos() -> Vec<ModelInfo> {
    FACTORY_DROID_CURATED_MODELS
        .iter()
        .map(|entry| model_info_with_variants(entry.id, entry.efforts))
        .collect()
}

#[cfg_attr(test, allow(dead_code))]
fn model_info_with_variants(model_id: &str, efforts: &[&str]) -> ModelInfo {
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

#[cfg_attr(test, allow(dead_code))]
fn variant_for_effort(effort: Effort) -> ModelVariant {
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

#[cfg_attr(test, allow(dead_code))]
pub async fn fetch_models(api_key: Option<&str>) -> Result<Vec<ModelInfo>, String> {
    let Some(binary) = resolve_droid_binary() else {
        return Err("Factory Droid CLI not installed.".to_string());
    };

    if cfg!(test) && std::env::var_os("GOSPEL_DROID_PATH").is_none() {
        return Ok(curated_model_infos());
    }

    let api_key_override = effective_api_key(api_key);
    let mut envs = Vec::new();
    if let Some(ref key) = api_key_override {
        envs.push(("FACTORY_API_KEY", key.as_str()));
    }

    let handler: Arc<dyn AcpClientHandler> = Arc::new(NoOpAcpHandler);
    let client = AcpClient::spawn(
        &binary,
        &["exec", "--output-format", "acp"],
        None,
        &envs,
        handler,
        None,
    )
    .map_err(|e| e.to_string())?;

    let cwd = std::env::temp_dir();
    let cwd_str = cwd.to_string_lossy().to_string();

    let live_result: Result<(Option<String>, Vec<ModelInfo>, AgentCapabilities), AcpTransportError> =
        async {
            let init: InitializeResult = client
                .request(
                    "initialize",
                    &initialize_params(None),
                    Some(MODEL_FETCH_TIMEOUT),
                )
                .await?;
            let session: SessionNewResult = client
                .request(
                    "session/new",
                    &SessionNewParams {
                        cwd: cwd_str,
                        mcp_servers: Vec::new(),
                    },
                    Some(MODEL_FETCH_TIMEOUT),
                )
                .await?;
            let models = extract_models_from_session(&session.config_options, session.models.as_ref());
            Ok((Some(session.session_id), models, init.agent_capabilities))
        }
        .await;

    match live_result {
        Ok((session_id, models, caps)) => {
            if let (Some(sid), true) = (session_id, caps.supports_close()) {
                let _ = client
                    .request::<_, Value>(
                        "session/close",
                        &SessionCloseParams { session_id: sid },
                        Some(Duration::from_secs(3)),
                    )
                    .await;
            }
            client.shutdown().await;
            set_cached_authenticated(Some(true));
            if models.is_empty() {
                Ok(curated_model_infos())
            } else {
                Ok(models)
            }
        }
        Err(AcpTransportError::Rpc(rpc)) if rpc.code == -32000 => {
            client.shutdown().await;
            set_cached_authenticated(Some(false));
            Err("Factory Droid session expired or unauthenticated; sign in again.".to_string())
        }
        Err(err) => {
            client.shutdown().await;
            tracing::warn!("Factory Droid live model discovery failed ({err}); falling back to curated catalog");
            Ok(curated_model_infos())
        }
    }
}

#[cfg_attr(test, allow(dead_code))]
fn extract_models_from_session(
    config_options: &[SessionConfigOption],
    models_meta: Option<&super::protocol::ModelsMetadata>,
) -> Vec<ModelInfo> {
    let mut effort_choices: Vec<String> = Vec::new();
    for opt in config_options {
        if opt.category.as_deref() == Some("thought_level") || opt.id == "reasoning_effort" {
            effort_choices = opt
                .all_choices()
                .into_iter()
                .map(|c| c.value.clone())
                .filter(|v| !v.is_empty())
                .collect();
        }
    }

    let mut collected_ids: Vec<String> = Vec::new();
    for opt in config_options {
        if opt.category.as_deref() == Some("model") || opt.id == "model" {
            for choice in opt.all_choices() {
                if !choice.value.is_empty() && !collected_ids.contains(&choice.value) {
                    collected_ids.push(choice.value.clone());
                }
            }
        }
    }
    if collected_ids.is_empty() {
        if let Some(meta) = models_meta {
            for model in &meta.available_models {
                if !model.model_id.is_empty() && !collected_ids.contains(&model.model_id) {
                    collected_ids.push(model.model_id.clone());
                }
            }
        }
    }

    if !effort_choices.is_empty() {
        let mut overrides = DROID_EFFORT_OVERRIDES
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        for id in &collected_ids {
            if !FACTORY_DROID_CURATED_MODELS.iter().any(|m| m.id == id) {
                overrides.insert(id.clone(), effort_choices.clone());
            }
        }
    }

    collected_ids
        .into_iter()
        .map(|id| {
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

// ============================================================================
// Active ACP Run Cancellation Registry
// ============================================================================

struct ActiveAcpRun {
    client: Arc<AcpClient>,
    acp_session_id: StdMutex<Option<String>>,
}

#[derive(Default)]
pub struct AcpRunRegistry {
    runs: StdMutex<HashMap<String, Arc<ActiveAcpRun>>>,
}

impl AcpRunRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn register(&self, gospel_session_id: &str, client: Arc<AcpClient>) -> Arc<ActiveAcpRun> {
        let handle = Arc::new(ActiveAcpRun {
            client,
            acp_session_id: StdMutex::new(None),
        });
        self.runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(gospel_session_id.to_string(), Arc::clone(&handle));
        handle
    }

    fn unregister(&self, gospel_session_id: &str, handle: &Arc<ActiveAcpRun>) {
        let mut guard = self.runs.lock().unwrap_or_else(|p| p.into_inner());
        if guard
            .get(gospel_session_id)
            .is_some_and(|existing| Arc::ptr_eq(existing, handle))
        {
            guard.remove(gospel_session_id);
        }
    }

    pub fn cancel_session(&self, gospel_session_id: &str) {
        let entry = self
            .runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(gospel_session_id);
        if let Some(handle) = entry {
            let acp_sid = handle
                .acp_session_id
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            tokio::spawn(async move {
                if let Some(sid) = acp_sid {
                    let _ = handle
                        .client
                        .notify("session/cancel", &SessionCancelParams { session_id: sid })
                        .await;
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                handle.client.shutdown().await;
            });
        }
    }
}

// ============================================================================
// ACP Turn Execution
// ============================================================================

pub struct AcpTurnRequest<'a> {
    pub prompt: &'a str,
    pub model: &'a str,
    pub variant: Option<&'a str>,
    pub api_key: &'a str,
    pub workspace: Option<&'a ActiveWorkspaceContext>,
    pub existing_acp_session_id: Option<&'a str>,
    pub fallback_context_summary: Option<&'a str>,
    pub matched_skills_section: Option<&'a str>,
    pub invoked_skill_section: Option<&'a str>,
    pub memory_section: Option<&'a str>,
    pub gospel_session_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub approval_broker: Option<Arc<ApprovalBroker>>,
    pub run_registry: Option<Arc<AcpRunRegistry>>,
}

#[derive(Default)]
struct TurnTraceState {
    accepting_updates: bool,
    full_response: String,
    blocks: Vec<Value>,
    tool_calls: usize,
    source_edit_succeeded: bool,
    active_reasoning_id: Option<String>,
    reasoning_text: String,
    reasoning_seq: usize,
    tool_names_by_id: HashMap<String, String>,
}

impl TurnTraceState {
    fn close_reasoning(&mut self, on_event: &mut (dyn FnMut(StreamEvent) + Send)) {
        if let Some(id) = self.active_reasoning_id.take() {
            let text = std::mem::take(&mut self.reasoning_text);
            if !text.is_empty() {
                on_event(StreamEvent::Reasoning(ReasoningPayload {
                    id,
                    text,
                    phase: ReasoningPhase::Complete,
                }));
            }
        }
    }

    fn append_text(&mut self, chunk: &str, on_event: &mut (dyn FnMut(StreamEvent) + Send)) {
        if chunk.is_empty() {
            return;
        }
        self.close_reasoning(on_event);
        self.full_response.push_str(chunk);
        on_event(StreamEvent::Text(chunk.to_string()));

        if let Some(last) = self.blocks.last_mut() {
            if last.get("kind").and_then(Value::as_str) == Some("text") {
                let prev = last.get("text").and_then(Value::as_str).unwrap_or("");
                let combined = format!("{prev}{chunk}");
                last["text"] = json!(combined);
                return;
            }
        }
        let id = format!("text-{}", self.blocks.len());
        self.blocks.push(json!({
            "kind": "text",
            "id": id,
            "text": chunk,
        }));
    }

    fn append_thought(&mut self, chunk: &str, on_event: &mut (dyn FnMut(StreamEvent) + Send)) {
        if chunk.is_empty() {
            return;
        }
        let id = match &self.active_reasoning_id {
            Some(id) => id.clone(),
            None => {
                self.reasoning_seq += 1;
                let next = format!("reasoning-{}", self.reasoning_seq);
                self.active_reasoning_id = Some(next.clone());
                next
            }
        };
        self.reasoning_text.push_str(chunk);
        on_event(StreamEvent::Reasoning(ReasoningPayload {
            id,
            text: chunk.to_string(),
            phase: ReasoningPhase::Delta,
        }));
    }

    fn start_tool(
        &mut self,
        id: String,
        title: String,
        kind: Option<&str>,
        raw_input: Option<Value>,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) {
        self.close_reasoning(on_event);
        let id = if id.is_empty() {
            format!("acp-tool-{}", self.tool_calls + 1)
        } else {
            id
        };
        let normalized = normalize_acp_tool_name(&title, kind);
        let args = raw_input.unwrap_or_else(|| json!({ "title": title }));

        if let Some(existing) = self
            .blocks
            .iter_mut()
            .find(|b| b.get("kind").and_then(Value::as_str) == Some("tool") && b.get("id").and_then(Value::as_str) == Some(id.as_str()))
        {
            existing["name"] = json!(normalized);
            existing["arguments"] = args.clone();
            return;
        }

        self.tool_calls += 1;
        self.tool_names_by_id.insert(id.clone(), normalized.clone());
        on_event(StreamEvent::ToolCall {
            id: id.clone(),
            name: normalized.clone(),
            arguments: args.clone(),
        });
        self.blocks.push(json!({
            "kind": "tool",
            "id": id,
            "name": normalized,
            "arguments": args,
            "status": "calling",
        }));
    }

    #[allow(clippy::too_many_arguments)]
    fn update_tool(
        &mut self,
        id: String,
        title: Option<&str>,
        kind: Option<&str>,
        status: Option<&str>,
        raw_input: Option<Value>,
        raw_output: Option<Value>,
        content_text: Option<String>,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) {
        if id.is_empty() {
            return;
        }
        let existing_name = self.tool_names_by_id.get(&id).cloned().unwrap_or_else(|| {
            normalize_acp_tool_name(title.unwrap_or("tool"), kind)
        });

        let is_terminal = matches!(
            status,
            Some("completed" | "failed" | "error" | "cancelled")
        );
        let ui_status = match status {
            Some("cancelled") => "cancelled",
            Some("completed" | "failed" | "error") => "completed",
            _ => "calling",
        };

        let result_text = raw_output
            .as_ref()
            .and_then(|v| serde_json::to_string_pretty(v).ok())
            .or(content_text);

        if let Some(block) = self.blocks.iter_mut().find(|b| {
            b.get("kind").and_then(Value::as_str) == Some("tool")
                && b.get("id").and_then(Value::as_str) == Some(id.as_str())
        }) {
            if let Some(input) = raw_input {
                block["arguments"] = input;
            }
            if is_terminal || result_text.is_some() {
                block["status"] = json!(ui_status);
            }
            if let Some(ref text) = result_text {
                let (capped, _) = truncate_text_bytes(text, MAX_TOOL_RESULT_DISPLAY_BYTES);
                block["result"] = json!(capped);
            }
        } else if is_terminal {
            self.tool_calls += 1;
            let args = raw_input.unwrap_or_else(|| json!({}));
            on_event(StreamEvent::ToolCall {
                id: id.clone(),
                name: existing_name.clone(),
                arguments: args.clone(),
            });
            let mut block = json!({
                "kind": "tool",
                "id": id,
                "name": existing_name,
                "arguments": args,
                "status": ui_status,
            });
            if let Some(ref text) = result_text {
                let (capped, _) = truncate_text_bytes(text, MAX_TOOL_RESULT_DISPLAY_BYTES);
                block["result"] = json!(capped);
            }
            self.blocks.push(block);
        }

        if is_terminal {
            if (matches!(kind, Some("edit" | "delete" | "move"))
                || matches!(existing_name.as_str(), "edit_file" | "write_file" | "source_edit"))
                && status == Some("completed")
            {
                self.source_edit_succeeded = true;
            }
            let emitted_result = result_text.unwrap_or_else(|| {
                if status == Some("cancelled") {
                    "Cancelled".to_string()
                } else {
                    "Completed".to_string()
                }
            });
            let (capped, _) = truncate_text_bytes(&emitted_result, MAX_TOOL_RESULT_DISPLAY_BYTES);
            on_event(StreamEvent::ToolResult {
                id,
                name: existing_name,
                result: capped,
            });
        }
    }
}

fn normalize_acp_tool_name(title: &str, kind: Option<&str>) -> String {
    match kind {
        Some("read") => "read_file".to_string(),
        Some("edit") => "edit_file".to_string(),
        Some("execute") => "bash".to_string(),
        Some("search") => "search_code".to_string(),
        _ => {
            let lower = title.trim().to_ascii_lowercase();
            if lower.contains("read") {
                "read_file".to_string()
            } else if lower.contains("edit") || lower.contains("patch") {
                "edit_file".to_string()
            } else if lower.contains("write") {
                "write_file".to_string()
            } else if lower.contains("grep") || lower.contains("search") {
                "search_code".to_string()
            } else if lower.contains("glob") || lower.contains("find") {
                "find_files".to_string()
            } else if lower.contains("bash") || lower.contains("exec") || lower.contains("run ") {
                "bash".to_string()
            } else {
                let clean = title.trim();
                if clean.is_empty() {
                    "tool".to_string()
                } else {
                    clean.to_string()
                }
            }
        }
    }
}

struct AcpTurnHandler {
    workspace: Option<PathBuf>,
    approval_broker: Option<Arc<ApprovalBroker>>,
    run_id: Option<String>,
    state: Mutex<TurnTraceState>,
    event_tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
}

#[async_trait]
impl AcpClientHandler for AcpTurnHandler {
    async fn on_session_update(&self, notification: SessionUpdateNotification) {
        let parsed = super::protocol::parse_session_update(&notification.update);
        let mut state = self.state.lock().await;
        if !state.accepting_updates {
            return;
        }
        let mut emit = |ev: StreamEvent| {
            let _ = self.event_tx.send(ev);
        };
        match parsed {
            ParsedSessionUpdate::AgentMessageChunk { text } => {
                state.append_text(&text, &mut emit);
            }
            ParsedSessionUpdate::AgentThoughtChunk { text } => {
                state.append_thought(&text, &mut emit);
            }
            ParsedSessionUpdate::ToolCall {
                tool_call_id,
                title,
                kind,
                raw_input,
                ..
            } => {
                state.start_tool(tool_call_id, title, kind.as_deref(), raw_input, &mut emit);
            }
            ParsedSessionUpdate::ToolCallUpdate {
                tool_call_id,
                title,
                kind,
                status,
                raw_input,
                raw_output,
                content_text,
            } => {
                if kind.as_deref() == Some("edit") && status.as_deref() == Some("completed") {
                    state.source_edit_succeeded = true;
                }
                state.update_tool(
                    tool_call_id,
                    title.as_deref(),
                    kind.as_deref(),
                    status.as_deref(),
                    raw_input,
                    raw_output,
                    content_text,
                    &mut emit,
                );
            }
            ParsedSessionUpdate::Other => {}
        }
    }

    async fn request_permission(
        &self,
        params: RequestPermissionParams,
    ) -> Result<RequestPermissionResult, String> {
        let Some(broker) = self.approval_broker.as_ref() else {
            return Ok(RequestPermissionResult {
                outcome: PermissionOutcome::Cancelled,
            });
        };

        let request = build_approval_request(&params, self.workspace.as_deref(), self.run_id.as_deref());
        let (outcome, decision) = broker.request_with_decision(request).await;
        Ok(RequestPermissionResult {
            outcome: map_approval_to_acp_outcome(outcome, decision, &params.options),
        })
    }

    async fn read_text_file(&self, params: ReadTextFileParams) -> Result<ReadTextFileResult, String> {
        let Some(root) = self.workspace.as_deref() else {
            return Err("No active workspace is open".to_string());
        };
        let resolved = resolve_workspace_path(root, Path::new(&params.path), false)?;
        let bytes = tokio::fs::read(&resolved)
            .await
            .map_err(|e| format!("Failed to read {}: {e}", params.path))?;
        if bytes.len() > MAX_FS_READ_BYTES {
            return Err(format!("File {} exceeds {MAX_FS_READ_BYTES} bytes", params.path));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| format!("File {} is not valid UTF-8", params.path))?;
        let sliced = slice_lines(&text, params.line, params.limit);
        Ok(ReadTextFileResult { content: sliced })
    }

    async fn write_text_file(&self, params: WriteTextFileParams) -> Result<(), String> {
        let Some(root) = self.workspace.as_deref() else {
            return Err("No active workspace is open".to_string());
        };
        if params.content.len() > MAX_FS_WRITE_BYTES {
            return Err(format!(
                "Write exceeds {MAX_FS_WRITE_BYTES} bytes for {}",
                params.path
            ));
        }
        let resolved = resolve_workspace_path(root, Path::new(&params.path), true)?;
        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create parent directory: {e}"))?;
        }
        tokio::fs::write(&resolved, params.content.as_bytes())
            .await
            .map_err(|e| format!("Failed to write {}: {e}", params.path))?;
        self.state.lock().await.source_edit_succeeded = true;
        Ok(())
    }
}

fn slice_lines(text: &str, line: Option<usize>, limit: Option<usize>) -> String {
    if line.is_none() && limit.is_none() {
        return text.to_string();
    }
    let start = line.unwrap_or(1).saturating_sub(1);
    let max_lines = limit.unwrap_or(usize::MAX);
    text.lines()
        .skip(start)
        .take(max_lines)
        .collect::<Vec<_>>()
        .join("\n")
}

fn resolve_workspace_path(
    workspace_root: &Path,
    requested: &Path,
    for_write: bool,
) -> Result<PathBuf, String> {
    let canonical_root = symlink_guard::canonical(workspace_root)
        .map_err(|e| format!("Invalid workspace root: {e}"))?;
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        canonical_root.join(requested)
    };

    if for_write {
        symlink_guard::validate_existing_ancestors(&canonical_root, &candidate)
            .map_err(|e| format!("Workspace containment rejected write: {e}"))?;
        if is_sensitive_workspace_target(&canonical_root, &candidate) {
            return Err(format!(
                "Refusing write to protected path {}",
                requested.display()
            ));
        }
        Ok(candidate)
    } else {
        let resolved = symlink_guard::canonical(&candidate)
            .map_err(|e| format!("Failed to resolve {}: {e}", requested.display()))?;
        if !symlink_guard::is_within(&canonical_root, &resolved) {
            return Err(format!(
                "Path {} escapes active workspace",
                requested.display()
            ));
        }
        Ok(resolved)
    }
}

fn is_sensitive_workspace_target(root: &Path, candidate: &Path) -> bool {
    let Ok(rel) = candidate.strip_prefix(root) else {
        return true;
    };
    let rel_str = rel.to_string_lossy();
    let file_name = candidate
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    rel.components().any(|c| c.as_os_str() == ".git")
        || file_name.starts_with(".env")
        || rel_str.ends_with(".pem")
        || rel_str.ends_with(".key")
}

fn build_approval_request(
    params: &RequestPermissionParams,
    workspace_root: Option<&Path>,
    run_id: Option<&str>,
) -> ApprovalRequest {
    let kind = params.tool_call.kind.as_deref().unwrap_or("");
    let raw_input = params.tool_call.raw_input.as_ref();

    if kind == "execute" {
        let command = raw_input
            .and_then(|v| {
                v.get("command")
                    .or_else(|| v.get("cmd"))
                    .and_then(Value::as_str)
            })
            .unwrap_or(params.tool_call.title.as_str());
        let (risk, workspace_scoped) = classify_acp_command(command, workspace_root);
        return ApprovalRequest::command(
            "factory_droid",
            command,
            format!("Factory Droid requested permission: {}", params.tool_call.title),
            risk,
            run_id,
        )
        .with_workspace_scope(workspace_scoped);
    }

    let extracted_path = raw_input
        .and_then(|v| {
            v.get("path")
                .or_else(|| v.get("file_path"))
                .or_else(|| v.get("filePath"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            params
                .tool_call
                .locations
                .first()
                .and_then(|loc| loc.get("path").and_then(Value::as_str))
        });

    if let Some(path_str) = extracted_path {
        let is_inside = workspace_root
            .and_then(|root| resolve_workspace_path(root, Path::new(path_str), true).ok())
            .is_some();
        if !is_inside {
            return ApprovalRequest::external_path(
                "factory_droid",
                path_str,
                format!("Factory Droid requested access outside workspace: {}", params.tool_call.title),
                run_id,
            );
        }
    }

    let risk = if kind == "delete" {
        CommandRisk::Destructive
    } else {
        CommandRisk::Mutating
    };
    ApprovalRequest::command(
        "factory_droid",
        params.tool_call.title.clone(),
        "Factory Droid requested permission to modify workspace files or state.",
        risk,
        run_id,
    )
    .with_workspace_scope(workspace_root.is_some())
}

fn classify_acp_command(command: &str, workspace_root: Option<&Path>) -> (CommandRisk, bool) {
    let parts: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    let Some((program, args)) = parts.split_first() else {
        return (CommandRisk::Restricted, false);
    };
    let root = workspace_root.unwrap_or_else(|| Path::new("."));
    let safety = CommandPolicy.classify_shell(program, args, root);
    let risk = match safety {
        CommandSafety::Destructive => CommandRisk::Destructive,
        CommandSafety::Restricted(_) | CommandSafety::Blocked(_) => CommandRisk::Restricted,
        CommandSafety::ReadOnly | CommandSafety::Mutating => CommandRisk::Mutating,
    };
    let scoped = workspace_root.is_some() && risk == CommandRisk::Mutating;
    (risk, scoped)
}

fn map_approval_to_acp_outcome(
    outcome: ApprovalOutcome,
    decision: Option<ApprovalDecision>,
    options: &[PermissionOption],
) -> PermissionOutcome {
    let select_kind = |preferred_kinds: &[&str]| -> PermissionOutcome {
        for kind in preferred_kinds {
            if let Some(opt) = options.iter().find(|o| o.kind == *kind) {
                return PermissionOutcome::Selected {
                    option_id: opt.option_id.clone(),
                };
            }
        }
        PermissionOutcome::Cancelled
    };

    match (outcome, decision) {
        (ApprovalOutcome::Approved, Some(ApprovalDecision::AlwaysAllow)) => {
            select_kind(&["allow_always", "allow_session", "allow_once"])
        }
        (ApprovalOutcome::Approved | ApprovalOutcome::AutoApproved, _) => {
            select_kind(&["allow_once", "allow_always", "allow_session"])
        }
        (ApprovalOutcome::Denied, Some(ApprovalDecision::AlwaysDeny)) => {
            select_kind(&["reject_always", "reject_once"])
        }
        (ApprovalOutcome::Denied | ApprovalOutcome::AutoDenied, _) => {
            select_kind(&["reject_once", "reject_always"])
        }
        (ApprovalOutcome::TimedOut, _) => PermissionOutcome::Cancelled,
    }
}

fn build_acp_prompt(request: &AcpTurnRequest<'_>, seed_context: Option<&str>) -> String {
    let mut sections = Vec::new();
    if let Some(invoked) = request.invoked_skill_section.filter(|s| !s.trim().is_empty()) {
        sections.push(invoked.trim().to_string());
    } else if let Some(matched) = request.matched_skills_section.filter(|s| !s.trim().is_empty()) {
        sections.push(matched.trim().to_string());
    }
    if let Some(memory) = request.memory_section.filter(|s| !s.trim().is_empty()) {
        sections.push(memory.trim().to_string());
    }
    if let Some(ctx) = seed_context.filter(|s| !s.trim().is_empty()) {
        sections.push(format!("## Previous Conversation Context\n\n{ctx}"));
    }
    sections.push(request.prompt.to_string());
    sections.join("\n\n---\n\n")
}

fn variant_to_droid_effort(variant: Option<&str>) -> Option<&str> {
    let raw = variant.map(str::trim).filter(|v| !v.is_empty())?;
    let stripped = raw.strip_prefix("reasoning-").unwrap_or(raw);
    match stripped {
        "extra-high" | "extra_high" => Some("xhigh"),
        other => Some(other),
    }
}

async fn apply_session_model_and_effort(
    client: &AcpClient,
    session_id: &str,
    model: &str,
    effort: Option<&str>,
    config_options: &[SessionConfigOption],
    models_meta: Option<&super::protocol::ModelsMetadata>,
) {
    let model_opt = config_options
        .iter()
        .find(|opt| opt.category.as_deref() == Some("model") || opt.id == "model");
    if let Some(opt) = model_opt {
        if opt.current_value.as_str() != Some(model) {
            let _ = client
                .request::<_, Value>(
                    "session/set_config_option",
                    &SessionSetConfigOptionParams {
                        session_id: session_id.to_string(),
                        config_id: opt.id.clone(),
                        value: json!(model),
                    },
                    Some(SESSION_OP_TIMEOUT),
                )
                .await;
        }
    } else if models_meta
        .and_then(|m| m.current_model_id.as_deref())
        .is_some_and(|cur| cur != model)
    {
        let _ = client
            .request::<_, Value>(
                "session/set_model",
                &SessionSetModelParams {
                    session_id: session_id.to_string(),
                    model_id: model.to_string(),
                },
                Some(SESSION_OP_TIMEOUT),
            )
            .await;
    }

    if let Some(effort_value) = effort {
        if let Some(effort_opt) = config_options.iter().find(|opt| {
            opt.category.as_deref() == Some("thought_level") || opt.id == "reasoning_effort"
        }) {
            if effort_opt.current_value.as_str() != Some(effort_value) {
                let _ = client
                    .request::<_, Value>(
                        "session/set_config_option",
                        &SessionSetConfigOptionParams {
                            session_id: session_id.to_string(),
                            config_id: effort_opt.id.clone(),
                            value: json!(effort_value),
                        },
                        Some(SESSION_OP_TIMEOUT),
                    )
                    .await;
            }
        }
    }
}

pub async fn run_acp_turn<F>(
    request: AcpTurnRequest<'_>,
    mut on_event: F,
) -> Result<StreamCompletionResult, LlmError>
where
    F: FnMut(StreamEvent) + Send,
{
    let binary = resolve_droid_binary().ok_or_else(|| {
        LlmError::ProviderError(
            "Factory Droid CLI not found. Install droid or set GOSPEL_DROID_PATH.".to_string(),
        )
    })?;

    let workspace_path = request.workspace.map(|w| w.workspace_path.clone());
    let cwd_path = workspace_path
        .clone()
        .unwrap_or_else(std::env::temp_dir);
    let cwd_str = cwd_path.to_string_lossy().to_string();

    let effort = variant_to_droid_effort(request.variant);
    let mut args: Vec<String> = vec![
        "exec".to_string(),
        "--output-format".to_string(),
        "acp".to_string(),
        "--cwd".to_string(),
        cwd_str.clone(),
    ];
    if !request.model.trim().is_empty() {
        args.push("-m".to_string());
        args.push(request.model.to_string());
    }
    if let Some(level) = effort {
        args.push("-r".to_string());
        args.push(level.to_string());
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

    let api_key_override = effective_api_key(Some(request.api_key));
    let mut envs = Vec::new();
    if let Some(ref key) = api_key_override {
        envs.push(("FACTORY_API_KEY", key.as_str()));
    }

    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handler = Arc::new(AcpTurnHandler {
        workspace: workspace_path.clone(),
        approval_broker: request.approval_broker.clone(),
        run_id: request.run_id.map(str::to_string),
        state: Mutex::new(TurnTraceState::default()),
        event_tx,
    });

    let client = AcpClient::spawn(
        &binary,
        &arg_refs,
        Some(&cwd_path),
        &envs,
        Arc::clone(&handler) as Arc<dyn AcpClientHandler>,
        None,
    )
    .map_err(|e| LlmError::ProviderError(e.to_string()))?;

    let run_handle = request
        .gospel_session_id
        .zip(request.run_registry.as_ref())
        .map(|(sid, registry)| (sid.to_string(), Arc::clone(registry), registry.register(sid, Arc::clone(&client))));

    struct CleanupGuard {
        client: Arc<AcpClient>,
        run_handle: Option<(String, Arc<AcpRunRegistry>, Arc<ActiveAcpRun>)>,
    }
    impl Drop for CleanupGuard {
        fn drop(&mut self) {
            if let Some((sid, registry, handle)) = self.run_handle.take() {
                registry.unregister(&sid, &handle);
            }
            let client = Arc::clone(&self.client);
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                rt.spawn(async move {
                    client.shutdown().await;
                });
            }
        }
    }

    let _guard = CleanupGuard {
        client: Arc::clone(&client),
        run_handle: run_handle.clone(),
    };

    let init: InitializeResult = client
        .request(
            "initialize",
            &initialize_params(workspace_path.as_deref()),
            Some(SESSION_OP_TIMEOUT),
        )
        .await
        .map_err(|e| map_acp_error(e, "initialize"))?;

    let mut used_fallback_context = false;
    let (acp_session_id, config_options, models_meta) =
        if let (Some(existing_sid), true) = (request.existing_acp_session_id, init.agent_capabilities.load_session) {
            match client
                .request::<_, Value>(
                    "session/load",
                    &SessionLoadParams {
                        session_id: existing_sid.to_string(),
                        cwd: cwd_str.clone(),
                        mcp_servers: Vec::new(),
                    },
                    Some(SESSION_OP_TIMEOUT),
                )
                .await
            {
                Ok(val) => {
                    let parsed: SessionNewResult =
                        serde_json::from_value(val).unwrap_or(SessionNewResult {
                            session_id: existing_sid.to_string(),
                            config_options: Vec::new(),
                            models: None,
                        });
                    let sid = if parsed.session_id.is_empty() {
                        existing_sid.to_string()
                    } else {
                        parsed.session_id
                    };
                    (sid, parsed.config_options, parsed.models)
                }
                Err(AcpTransportError::Rpc(rpc)) if rpc.code == -32000 => {
                    return Err(map_acp_error(AcpTransportError::Rpc(rpc), "session/load"));
                }
                Err(err) => {
                    tracing::warn!(
                        "ACP session/load failed for {existing_sid} ({err}); starting a fresh session"
                    );
                    used_fallback_context = true;
                    let created: SessionNewResult = client
                        .request(
                            "session/new",
                            &SessionNewParams {
                                cwd: cwd_str.clone(),
                                mcp_servers: Vec::new(),
                            },
                            Some(SESSION_OP_TIMEOUT),
                        )
                        .await
                        .map_err(|e| map_acp_error(e, "session/new"))?;
                    (created.session_id, created.config_options, created.models)
                }
            }
        } else {
            used_fallback_context = request.existing_acp_session_id.is_none()
                && request.fallback_context_summary.is_some();
            let created: SessionNewResult = client
                .request(
                    "session/new",
                    &SessionNewParams {
                        cwd: cwd_str.clone(),
                        mcp_servers: Vec::new(),
                    },
                    Some(SESSION_OP_TIMEOUT),
                )
                .await
                .map_err(|e| map_acp_error(e, "session/new"))?;
            (created.session_id, created.config_options, created.models)
        };

    if let Some((_, _, ref handle)) = run_handle {
        *handle
            .acp_session_id
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(acp_session_id.clone());
    }

    apply_session_model_and_effort(
        &client,
        &acp_session_id,
        request.model,
        effort,
        &config_options,
        models_meta.as_ref(),
    )
    .await;

    let full_prompt = build_acp_prompt(
        &request,
        if used_fallback_context {
            request.fallback_context_summary
        } else {
            None
        },
    );

    handler.state.lock().await.accepting_updates = true;

    let prompt_params = SessionPromptParams {
        session_id: acp_session_id.clone(),
        prompt: vec![PromptContentBlock::Text { text: full_prompt }],
    };
    let prompt_future =
        client.request::<_, SessionPromptResult>("session/prompt", &prompt_params, None);
    tokio::pin!(prompt_future);

    let prompt_result: SessionPromptResult = loop {
        tokio::select! {
            res = &mut prompt_future => {
                break res.map_err(|e| map_acp_error(e, "session/prompt"))?;
            }
            maybe_ev = event_rx.recv() => {
                if let Some(ev) = maybe_ev {
                    on_event(ev);
                }
            }
        }
    };

    if init.agent_capabilities.supports_close() {
        let _ = client
            .request::<_, Value>(
                "session/close",
                &SessionCloseParams {
                    session_id: acp_session_id.clone(),
                },
                Some(Duration::from_secs(3)),
            )
            .await;
    }

    let mut state = handler.state.lock().await;
    state.accepting_updates = false;
    {
        let mut emit = |ev: StreamEvent| {
            let _ = handler.event_tx.send(ev);
        };
        state.close_reasoning(&mut emit);
    }
    while let Ok(ev) = event_rx.try_recv() {
        on_event(ev);
    }

    if prompt_result.stop_reason == "cancelled" {
        return Err(LlmError::ControlledStop(
            "Factory Droid ACP turn was cancelled.".to_string(),
        ));
    }

    let (prompt_tokens, response_tokens) = prompt_result
        .usage
        .as_ref()
        .map(|u| {
            (
                u.prompt_tokens.or(u.input_tokens).unwrap_or(0),
                u.response_tokens.or(u.output_tokens).unwrap_or(0),
            )
        })
        .unwrap_or((0, 0));

    let full_response = std::mem::take(&mut state.full_response);
    let blocks = std::mem::take(&mut state.blocks);
    let tool_calls = state.tool_calls;
    let source_edit_succeeded = state.source_edit_succeeded;
    drop(state);

    client.shutdown().await;

    Ok(StreamCompletionResult {
        full_response,
        history: None,
        source_edit_succeeded,
        prompt_tokens,
        response_tokens,
        tool_calls,
        acp_session_id: Some(acp_session_id),
        acp_turn_blocks: Some(blocks),
    })
}

pub async fn fork_acp_session(acp_session_id: &str, cwd: Option<&Path>) -> Option<String> {
    let binary = resolve_droid_binary()?;
    if cfg!(test) && std::env::var_os("GOSPEL_DROID_PATH").is_none() {
        return None;
    }
    let cwd_path = cwd.map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
    let cwd_str = cwd_path.to_string_lossy().to_string();
    let handler: Arc<dyn AcpClientHandler> = Arc::new(NoOpAcpHandler);
    let client = AcpClient::spawn(
        &binary,
        &["exec", "--output-format", "acp", "--cwd", &cwd_str],
        Some(&cwd_path),
        &[],
        handler,
        None,
    )
    .ok()?;

    let forked = async {
        let init: InitializeResult = client
            .request(
                "initialize",
                &initialize_params(cwd),
                Some(AUTH_PROBE_TIMEOUT),
            )
            .await
            .ok()?;
        if !init.agent_capabilities.supports_fork() {
            return None;
        }
        let res: SessionForkResult = client
            .request(
                "session/fork",
                &SessionForkParams {
                    session_id: acp_session_id.to_string(),
                    cwd: cwd_str,
                    mcp_servers: Vec::new(),
                },
                Some(SESSION_OP_TIMEOUT),
            )
            .await
            .ok()?;
        (!res.session_id.is_empty()).then_some(res.session_id)
    }
    .await;

    client.shutdown().await;
    forked
}

fn map_acp_error(err: AcpTransportError, stage: &str) -> LlmError {
    match err {
        AcpTransportError::Rpc(rpc) if rpc.code == -32000 => {
            set_cached_authenticated(Some(false));
            LlmError::ProviderError(format!("401 Unauthorized: {}", rpc.message))
        }
        other => LlmError::ProviderError(format!("Factory Droid ACP {stage} failed: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct EnvVarGuard {
        key: &'static str,
        prev: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let prev = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, prev }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.prev.take() {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    static TEST_DROID_LOCK: LazyLock<tokio::sync::Mutex<()>> =
        LazyLock::new(|| tokio::sync::Mutex::new(()));

    fn write_mock_droid(dir: &Path, script_body: &str) -> PathBuf {
        let path = dir.join("droid");
        std::fs::write(&path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[tokio::test]
    async fn probes_and_runs_turn_against_mock_acp_binary() {
        let _lock = TEST_DROID_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("ws");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::write(workspace.join("hello.txt"), "world\n").unwrap();

        let mock = write_mock_droid(
            dir.path(),
            r#"
while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*)
      printf '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"close":{},"fork":{}}},"agentInfo":{"name":"@factory/cli","title":"Factory Droid","version":"1.21.0"},"authMethods":[{"id":"device-pairing"}]}}\n'
      ;;
    *'"auth/status"'*)
      printf '{"jsonrpc":"2.0","id":2,"result":{"state":"authenticated"}}\n'
      ;;
    *'"session/new"'*)
      printf '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"droid-sess-42","configOptions":[{"id":"model","category":"model","type":"select","currentValue":"auto","options":[{"value":"auto","name":"Auto"},{"value":"claude-opus-5-5","name":"Opus 5.5"}]}]}}\n'
      ;;
    *'"session/set_config_option"'*)
      printf '{"jsonrpc":"2.0","id":3,"result":{"configOptions":[]}}\n'
      ;;
    *'"session/prompt"'*)
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"droid-sess-42","update":{"sessionUpdate":"agent_thought_chunk","content":{"thought":"reading"}}}}\n'
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"droid-sess-42","update":{"sessionUpdate":"tool_call","toolCallId":"tc-1","title":"Read hello.txt","kind":"read","status":"in_progress","rawInput":{"path":"hello.txt"}}}}\n'
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"droid-sess-42","update":{"sessionUpdate":"tool_call_update","toolCallId":"tc-1","status":"completed","content":[{"type":"text","text":"world"}]}}}\n'
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"droid-sess-42","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done!"}}}}\n'
      printf '{"jsonrpc":"2.0","id":4,"result":{"stopReason":"end_turn","usage":{"promptTokens":12,"responseTokens":4}}}\n'
      ;;
    *'"session/close"'*)
      printf '{"jsonrpc":"2.0","id":5,"result":{}}\n'
      exit 0
      ;;
  esac
done
"#,
        );

        let _guard = EnvVarGuard::set("GOSPEL_DROID_PATH", &mock);
        set_cached_authenticated(None);

        assert!(probe_auth_status().await);
        assert!(is_credentialed_sync());

        let events = Arc::new(StdMutex::new(Vec::new()));
        let events_cb = Arc::clone(&events);
        let ws_ctx = ActiveWorkspaceContext {
            workspace_path: workspace,
            source_edit_allowed: true,
        };
        let result = run_acp_turn(
            AcpTurnRequest {
                prompt: "Say done",
                model: "claude-opus-5-5",
                variant: Some("reasoning-high"),
                api_key: "",
                workspace: Some(&ws_ctx),
                existing_acp_session_id: None,
                fallback_context_summary: None,
                matched_skills_section: None,
                invoked_skill_section: None,
                memory_section: None,
                gospel_session_id: Some("g1"),
                run_id: Some("r1"),
                approval_broker: None,
                run_registry: None,
            },
            move |evt| {
                events_cb.lock().unwrap().push(format!("{evt:?}"));
            },
        )
        .await
        .unwrap();

        assert_eq!(result.full_response, "Done!");
        assert_eq!(result.acp_session_id.as_deref(), Some("droid-sess-42"));
        assert_eq!(result.tool_calls, 1);
        assert_eq!(result.prompt_tokens, 12);
        assert_eq!(result.response_tokens, 4);
        assert!(result.history.is_none());
        let blocks = result.acp_turn_blocks.unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["kind"], "tool");
        assert_eq!(blocks[0]["status"], "completed");
        assert_eq!(blocks[1]["kind"], "text");
        assert_eq!(blocks[1]["text"], "Done!");
    }

    #[test]
    fn workspace_containment_blocks_escaping_symlinks_and_git_writes() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(ws.join(".git")).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "top-secret").unwrap();

        assert!(resolve_workspace_path(&ws, Path::new("../outside/secret.txt"), false).is_err());
        assert!(resolve_workspace_path(&ws, Path::new(".git/config"), true).is_err());
        assert!(resolve_workspace_path(&ws, Path::new(".env.local"), true).is_err());
        assert!(resolve_workspace_path(&ws, Path::new("src/main.rs"), true).is_ok());
    }
}
