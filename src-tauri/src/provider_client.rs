//! Builds the rig completion model for a provider id.
//!
//! New Credentialed Providers must be registered in
//! `providers::CREDENTIALED_PROVIDERS` and given a match arm in
//! [`completion_model`].

use rig::http_client::{DynHttpClient, HeaderMap, HeaderValue, HttpMiddleware, Method, Uri};
use rig::providers::{anthropic, chatgpt, copilot, gemini, groq, mistral, openai, xai};
use rig::wasm_compat::WasmBoxedFuture;

pub(crate) type CompletionModel = rig::DynModel<rig::operation::Completion>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProviderClientError {
    /// Building the client failed (missing credential, refresh failure, ...).
    Client(String),
    /// The provider id is not a registered Credentialed Provider.
    Unsupported(String),
}

impl std::fmt::Display for ProviderClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Client(message) => f.write_str(message),
            Self::Unsupported(provider) => write!(f, "unsupported provider: {provider}"),
        }
    }
}

/// The completion model for `model` on `provider`.
///
/// `session` is the conversation id for providers that accept a session
/// header; a fresh UUID is generated when absent. OAuth providers read their
/// cached session and never start an interactive device flow here.
pub(crate) async fn completion_model(
    provider: &str,
    model: &str,
    api_key: &str,
    session: Option<&str>,
) -> Result<CompletionModel, ProviderClientError> {
    let client_err = |error: String| ProviderClientError::Client(error);
    let model = match provider {
        "openai" => openai::OpenAI::new(api_key).completion(model).into(),
        "chatgpt" => openai::OpenAIConfig::with_key(&chatgpt::DIALECT, "")
            .client()
            .authenticate(&chatgpt_authenticator(None))
            .await
            .map_err(|e| client_err(e.to_string()))?
            .completion(model)
            .into(),
        "github_copilot" => copilot::CopilotConfig::new("")
            .client()
            .authenticate(&copilot_authenticator(None))
            .await
            .map_err(|e| client_err(e.to_string()))?
            .completion(model)
            .into(),
        "grok" => {
            let access_token = grok_access_token(api_key).await.map_err(client_err)?;
            grok_subscription_client(&access_token)
                .completion(model)
                .into()
        }
        "xai" => xai::new(api_key).completion(model).into(),
        "anthropic" => anthropic::Anthropic::new(api_key).completion(model).into(),
        "gemini" => gemini::Gemini::new(api_key).completion(model).into(),
        "groq" => groq::new(api_key).completion(model).into(),
        "mistral" => mistral::new(api_key).completion(model).into(),
        "opencode_go" => {
            let http = opencode_go_http(session);
            match opencode_go_protocol(model) {
                OpencodeGoProtocol::AnthropicMessages => opencode_go_anthropic_config(api_key)
                    .connect(http)
                    .completion(model)
                    .into(),
                OpencodeGoProtocol::OpenAiResponses => opencode_go_openai_config(api_key)
                    .connect(http)
                    .responses(model)
                    .into(),
                OpencodeGoProtocol::OpenAiChatCompletions => opencode_go_openai_config(api_key)
                    .connect(http)
                    .chat(model)
                    .into(),
            }
        }
        other => return Err(ProviderClientError::Unsupported(other.to_string())),
    };
    Ok(model)
}

/// The ChatGPT subscription authenticator over Gospel's auth cache. A device
/// code handler enables the interactive login; without one only the cached
/// session (refreshed when needed) is used.
pub(crate) fn chatgpt_authenticator(
    device_code_handler: Option<chatgpt::auth::DeviceCodeHandler>,
) -> chatgpt::auth::Authenticator {
    let allow_device_flow = device_code_handler.is_some();
    chatgpt::auth::Authenticator::new(
        chatgpt::auth::AuthSource::OAuth,
        Some(crate::keychain::chatgpt_auth_file_path()),
        device_code_handler.unwrap_or_default(),
        allow_device_flow,
    )
}

/// The GitHub Copilot authenticator over Gospel's token directory. See
/// [`chatgpt_authenticator`] for how the device code handler is used.
pub(crate) fn copilot_authenticator(
    device_code_handler: Option<copilot::auth::DeviceCodeHandler>,
) -> copilot::auth::Authenticator {
    let allow_device_flow = device_code_handler.is_some();
    copilot::auth::Authenticator::new(
        copilot::auth::AuthSource::OAuth,
        Some(crate::keychain::github_copilot_access_token_path()),
        Some(crate::keychain::github_copilot_api_key_path()),
        device_code_handler.unwrap_or_default(),
        allow_device_flow,
    )
}

/// Inserts fixed headers into every request, replacing any value the
/// provider wire already set.
#[derive(Clone, Debug)]
pub(crate) struct StaticHeaders(HeaderMap);

impl StaticHeaders {
    pub(crate) fn apply(&self, headers: &mut HeaderMap) {
        for (name, value) in &self.0 {
            headers.insert(name.clone(), value.clone());
        }
    }
}

impl HttpMiddleware for StaticHeaders {
    fn before_request_headers<'a>(
        &'a self,
        _method: &'a Method,
        _uri: &'a Uri,
        headers: &'a mut HeaderMap,
    ) -> WasmBoxedFuture<'a, rig::http_client::Result<()>> {
        self.apply(headers);
        Box::pin(async { Ok(()) })
    }
}

fn http_with_headers(headers: HeaderMap) -> DynHttpClient {
    rig::rig_reqwest::shared().with_middleware(StaticHeaders(headers))
}

const GROK_SUBSCRIPTION_BASE_URL: &str = "https://cli-chat-proxy.grok.com";
const GROK_CLI_TOKEN_AUTH: &str = "xai-grok-cli";
const GROK_CLI_CLIENT_IDENTIFIER: &str = "grok-shell";
const GROK_CLI_CLIENT_VERSION: &str = "0.2.93";

fn grok_subscription_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-xai-token-auth",
        HeaderValue::from_static(GROK_CLI_TOKEN_AUTH),
    );
    headers.insert(
        "x-grok-client-identifier",
        HeaderValue::from_static(GROK_CLI_CLIENT_IDENTIFIER),
    );
    headers.insert(
        "x-grok-client-version",
        HeaderValue::from_static(GROK_CLI_CLIENT_VERSION),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        HeaderValue::from_static("text/event-stream"),
    );
    headers
}

pub(crate) async fn grok_access_token(api_key: &str) -> Result<String, String> {
    if !api_key.trim().is_empty() {
        return Ok(api_key.to_string());
    }
    let auth_path = crate::keychain::grok_auth_file_path();
    match crate::grok_oauth::ensure_fresh_access_token(&auth_path).await {
        Ok(token) => Ok(token),
        Err(e) if refresh_requires_reauth(&e) => Err(format!(
            "Grok OAuth session expired or invalid; sign in again ({e})"
        )),
        Err(e) => {
            tracing::warn!("Grok token refresh failed ({e}); using stored access token");
            crate::grok_oauth::access_token(&auth_path).map_err(|stored_err| {
                format!(
                    "Grok OAuth session expired or invalid; sign in again ({e}; {stored_err})"
                )
            })
        }
    }
}

fn refresh_requires_reauth(error: &str) -> bool {
    crate::credential_failure::refresh_error_requires_reauth(error)
}

fn grok_subscription_config(access_token: &str) -> openai::OpenAIConfig {
    openai::OpenAIConfig::with_key(&xai::DIALECT, access_token)
        .with_base_url(GROK_SUBSCRIPTION_BASE_URL)
}

pub(crate) fn grok_subscription_client(access_token: &str) -> openai::OpenAI {
    grok_subscription_config(access_token).connect(http_with_headers(grok_subscription_headers()))
}

/// OpenCode Go serves one upstream endpoint per model family
/// (<https://opencode.ai/docs/go/>): MiniMax and Qwen models speak Anthropic
/// Messages, Grok/GPT Luna/Muse Spark speak OpenAI Responses, and every other
/// model speaks OpenAI Chat Completions. Unknown models default to chat
/// completions, which is where OpenCode adds new open-model families; update
/// the mapping if OpenCode adds a future Anthropic-Messages family.
pub(crate) const OPENCODE_GO_OPENAI_BASE_URL: &str = "https://opencode.ai/zen/go/v1";
pub(crate) const OPENCODE_GO_ANTHROPIC_BASE_URL: &str = "https://opencode.ai/zen/go";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpencodeGoProtocol {
    OpenAiResponses,
    OpenAiChatCompletions,
    AnthropicMessages,
}

pub(crate) fn opencode_go_protocol(model: &str) -> OpencodeGoProtocol {
    let id = model.to_lowercase();
    if id.starts_with("minimax-") || id.starts_with("qwen") {
        OpencodeGoProtocol::AnthropicMessages
    } else if id.starts_with("grok-") || id.starts_with("gpt-") || id.starts_with("muse-") {
        OpencodeGoProtocol::OpenAiResponses
    } else {
        OpencodeGoProtocol::OpenAiChatCompletions
    }
}

pub(crate) fn opencode_go_headers(session: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        HeaderValue::from_static(concat!("gospel/", env!("CARGO_PKG_VERSION"))),
    );
    let session = session
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Ok(value) = HeaderValue::from_str(&session) {
        headers.insert("x-opencode-session", value);
    }
    headers
}

pub(crate) fn opencode_go_http(session: Option<&str>) -> DynHttpClient {
    http_with_headers(opencode_go_headers(session))
}

pub(crate) fn opencode_go_openai_config(api_key: &str) -> openai::OpenAIConfig {
    openai::OpenAIConfig::new(api_key).with_base_url(OPENCODE_GO_OPENAI_BASE_URL)
}

fn opencode_go_anthropic_config(api_key: &str) -> anthropic::AnthropicConfig {
    anthropic::AnthropicConfig::new(api_key).with_base_url(OPENCODE_GO_ANTHROPIC_BASE_URL)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_value(headers: &HeaderMap, name: impl reqwest::header::AsHeaderName) -> String {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string()
    }

    fn applied(headers: HeaderMap) -> HeaderMap {
        let mut request_headers = HeaderMap::new();
        request_headers.insert(
            reqwest::header::ACCEPT,
            HeaderValue::from_static("application/json"),
        );
        StaticHeaders(headers).apply(&mut request_headers);
        request_headers
    }

    fn assert_subscription_config(config: &openai::OpenAIConfig, access_token: &str) {
        assert_eq!(config.base_url, "https://cli-chat-proxy.grok.com");
        assert!(!config.base_url.contains("api.x.ai"));
        assert_eq!(config.dialect.name, xai::DIALECT.name);
        assert_eq!(config.api_key.expose(), access_token);
    }

    #[test]
    fn grok_subscription_headers_identify_cli_and_override_accept() {
        let headers = applied(grok_subscription_headers());
        assert_eq!(header_value(&headers, "x-xai-token-auth"), "xai-grok-cli");
        assert_eq!(
            header_value(&headers, "x-grok-client-identifier"),
            "grok-shell"
        );
        assert_eq!(header_value(&headers, "x-grok-client-version"), "0.2.93");
        assert_eq!(header_value(&headers, "accept"), "text/event-stream");
    }

    #[test]
    fn grok_subscription_client_targets_cli_proxy() {
        let client = grok_subscription_client("grok-oauth-access-token");
        assert_subscription_config(client.config(), "grok-oauth-access-token");
    }

    #[tokio::test]
    async fn grok_dispatch_with_invalid_stored_refresh_requires_reauth() {
        let _lock = crate::keychain::lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = crate::keychain::isolate_config_home(dir.path());
        let grok_dir = dir.path().join("gospel").join("grok");
        std::fs::create_dir_all(&grok_dir).unwrap();
        std::fs::write(
            grok_dir.join("auth.json"),
            r#"{"access_token":"stored-grok-oauth-token","refresh_token":"stored-refresh"}"#,
        )
        .unwrap();

        let err = completion_model("grok", "grok-4", "", None)
            .await
            .unwrap_err()
            .to_string();
        let lower = err.to_lowercase();
        assert!(
            lower.contains("sign in again"),
            "invalid refresh must direct the user to re-auth, got: {err}"
        );
        assert!(
            crate::credential_failure::classify_credential_failure(&err)
                == Some(crate::credential_failure::CredentialFailureKind::ReauthRequired)
        );
    }

    #[test]
    fn opencode_go_protocol_maps_documented_model_families() {
        use OpencodeGoProtocol::*;

        for model in [
            "minimax-m3",
            "minimax-m2.7",
            "qwen3.8-max",
            "qwen3.8-flash",
            "qwen3.7-plus",
        ] {
            assert_eq!(opencode_go_protocol(model), AnthropicMessages, "{model}");
        }
        for model in [
            "grok-4.7",
            "grok-4.6",
            "gpt-6-luna",
            "gpt-5.6-luna",
            "muse-spark-1.3-contributor",
        ] {
            assert_eq!(opencode_go_protocol(model), OpenAiResponses, "{model}");
        }
        for model in [
            "glm-5.3",
            "kimi-k3",
            "longcat-2.0",
            "deepseek-v4-flash",
            "mimo-v2.6-pro",
            "hy4-preview",
            "hy3",
            "space-bunny-free",
            "some-future-model",
        ] {
            assert_eq!(
                opencode_go_protocol(model),
                OpenAiChatCompletions,
                "{model}"
            );
        }
    }

    #[test]
    fn opencode_go_headers_identify_client_and_session() {
        let headers = applied(opencode_go_headers(Some("conversation-123")));
        assert_eq!(
            header_value(&headers, "x-opencode-session"),
            "conversation-123"
        );
        let user_agent = header_value(&headers, reqwest::header::USER_AGENT);
        assert!(
            user_agent.starts_with("gospel/"),
            "user agent: {user_agent}"
        );

        let generated = opencode_go_headers(None);
        assert!(generated.get("x-opencode-session").is_some());
    }

    #[test]
    fn opencode_go_configs_target_documented_endpoints() {
        let openai_config = opencode_go_openai_config("sk-test");
        assert_eq!(openai_config.base_url, "https://opencode.ai/zen/go/v1");
        assert_eq!(openai_config.api_key.expose(), "sk-test");

        let anthropic_config = opencode_go_anthropic_config("sk-test");
        assert_eq!(anthropic_config.base_url, "https://opencode.ai/zen/go");
    }

    #[tokio::test]
    async fn opencode_go_dispatch_builds_a_model_for_every_protocol() {
        for model in ["grok-4.7", "glm-5.3", "minimax-m3"] {
            let built = completion_model("opencode_go", model, "sk-test", Some("conversation-123"))
                .await
                .unwrap();
            assert_eq!(built.id(), Some(model));
            assert_eq!(
                built.name() == "anthropic",
                opencode_go_protocol(model) == OpencodeGoProtocol::AnthropicMessages,
                "{model} built on {}",
                built.name()
            );
        }
    }

    #[test]
    fn stored_oauth_provider_credential_constructs_subscription_client() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(
            &path,
            r#"{"access_token":"stored-grok-oauth-token","refresh_token":"stored-refresh"}"#,
        )
        .unwrap();

        let token = crate::grok_oauth::access_token(&path).unwrap();
        let client = grok_subscription_client(&token);

        assert_eq!(token, "stored-grok-oauth-token");
        assert_subscription_config(client.config(), "stored-grok-oauth-token");
    }

    #[test]
    fn xai_api_key_client_uses_developer_api_not_subscription_proxy() {
        let client = xai::new("xai-payg-api-key");
        let config = client.config();
        assert_eq!(config.base_url, "https://api.x.ai");
        assert!(!config.base_url.contains("cli-chat-proxy"));
        assert_eq!(config.api_key.expose(), "xai-payg-api-key");
    }

    #[tokio::test]
    async fn unknown_provider_is_unsupported() {
        let err = completion_model("not-a-provider", "m", "k", None)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            ProviderClientError::Unsupported("not-a-provider".to_string())
        );
    }

    #[test]
    fn invalid_refresh_error_requires_reauth() {
        assert!(refresh_requires_reauth(
            "Grok token refresh failed (HTTP 400): invalid_grant"
        ));
        assert!(refresh_requires_reauth(
            "Grok OAuth Provider Credential not found"
        ));
        assert!(!refresh_requires_reauth("connection refused"));
        assert!(!refresh_requires_reauth("HTTP 503 service unavailable"));
    }
}
