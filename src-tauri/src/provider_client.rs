/// Dispatches to the correct rig provider client based on the provider string.
///
/// New Credentialed Providers must be registered in `providers::CREDENTIALED_PROVIDERS`
/// and given a match arm below.
///
/// Usage:
/// ```ignore
/// provider_client!(provider, model, api_key, session, client_err, unsupported_err, |client| {
///     let agent = client.agent(model).build();
///     agent.prompt(prompt).await.map_err(client_err)?
/// })
/// ```
///
/// - `$model`: model id; providers serving multiple wire protocols (OpenCode Go)
///   use it to pick the correct client
/// - `$session`: `Option<&str>` conversation id for providers that accept a
///   session header; a fresh UUID is generated when absent
/// - `$client_err`: expression converting client construction `String` errors
/// - `$unsupported_err`: expression converting the unsupported-provider `String`
macro_rules! provider_client {
    ($provider:expr, $model:expr, $api_key:expr, $session:expr, $client_err:expr, $unsupported_err:expr, |$client:ident| $body:block) => {
        match $provider {
            "openai" => {
                let $client = rig::providers::openai::Client::new($api_key)
                    .map_err(|e| $client_err(e.to_string()))?;
                $body
            }
            "chatgpt" => {
                let $client = rig::providers::chatgpt::Client::builder()
                    .oauth()
                    .build()
                    .map_err(|e| $client_err(e.to_string()))?;
                $body
            }
            "github_copilot" => {
                let $client = rig::providers::copilot::Client::builder()
                    .oauth()
                    .token_dir(crate::keychain::github_copilot_token_dir())
                    .build()
                    .map_err(|e| $client_err(e.to_string()))?;
                $body
            }
            "grok" => {
                let access_token = $crate::provider_client::grok_access_token($api_key)
                    .await
                    .map_err(|e| $client_err(e))?;
                let $client = $crate::provider_client::grok_subscription_client(&access_token)
                    .map_err(|e| $client_err(e))?;
                $body
            }
            "anthropic" => {
                let $client = rig::providers::anthropic::Client::new($api_key)
                    .map_err(|e| $client_err(e.to_string()))?;
                $body
            }
            "gemini" => {
                let $client = rig::providers::gemini::Client::new($api_key)
                    .map_err(|e| $client_err(e.to_string()))?;
                $body
            }
            "groq" => {
                let $client = rig::providers::groq::Client::new($api_key)
                    .map_err(|e| $client_err(e.to_string()))?;
                $body
            }
            "mistral" => {
                let $client = rig::providers::mistral::Client::new($api_key)
                    .map_err(|e| $client_err(e.to_string()))?;
                $body
            }
            "opencode_go" => {
                let opencode_headers = $crate::provider_client::opencode_go_headers($session);
                match $crate::provider_client::opencode_go_protocol(&$model) {
                    $crate::provider_client::OpencodeGoProtocol::AnthropicMessages => {
                        let $client = rig::providers::anthropic::Client::builder()
                            .api_key($api_key)
                            .base_url($crate::provider_client::OPENCODE_GO_ANTHROPIC_BASE_URL)
                            .http_headers(opencode_headers)
                            .build()
                            .map_err(|e| $client_err(e.to_string()))?;
                        $body
                    }
                    $crate::provider_client::OpencodeGoProtocol::OpenAiResponses => {
                        let $client = rig::providers::openai::Client::builder()
                            .api_key($api_key)
                            .base_url($crate::provider_client::OPENCODE_GO_OPENAI_BASE_URL)
                            .http_headers(opencode_headers)
                            .build()
                            .map_err(|e| $client_err(e.to_string()))?;
                        $body
                    }
                    $crate::provider_client::OpencodeGoProtocol::OpenAiChatCompletions => {
                        let $client = rig::providers::openai::CompletionsClient::builder()
                            .api_key($api_key)
                            .base_url($crate::provider_client::OPENCODE_GO_OPENAI_BASE_URL)
                            .http_headers(opencode_headers)
                            .build()
                            .map_err(|e| $client_err(e.to_string()))?;
                        $body
                    }
                }
            }
            other => return Err($unsupported_err(other.to_string())),
        }
    };
}
pub(crate) use provider_client;

const GROK_SUBSCRIPTION_BASE_URL: &str = "https://cli-chat-proxy.grok.com";
const GROK_CLI_TOKEN_AUTH: &str = "xai-grok-cli";
const GROK_CLI_CLIENT_IDENTIFIER: &str = "grok-shell";
const GROK_CLI_CLIENT_VERSION: &str = "0.2.93";

fn grok_subscription_headers() -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-xai-token-auth",
        reqwest::header::HeaderValue::from_static(GROK_CLI_TOKEN_AUTH),
    );
    headers.insert(
        "x-grok-client-identifier",
        reqwest::header::HeaderValue::from_static(GROK_CLI_CLIENT_IDENTIFIER),
    );
    headers.insert(
        "x-grok-client-version",
        reqwest::header::HeaderValue::from_static(GROK_CLI_CLIENT_VERSION),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("text/event-stream"),
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
        Err(e) => {
            tracing::warn!("Grok token refresh failed ({e}); using stored access token");
            crate::grok_oauth::access_token(&auth_path)
        }
    }
}

pub(crate) fn grok_subscription_client(
    access_token: &str,
) -> Result<rig::providers::xai::Client, String> {
    rig::providers::xai::Client::builder()
        .api_key(access_token)
        .base_url(GROK_SUBSCRIPTION_BASE_URL)
        .http_headers(grok_subscription_headers())
        .build()
        .map_err(|e| e.to_string())
}

/// OpenCode Go serves one upstream endpoint per model family
/// (<https://opencode.ai/docs/go/>): MiniMax and Qwen models speak Anthropic
/// Messages, Grok/GPT Luna/Muse Spark speak OpenAI Responses, and every other
/// model speaks OpenAI Chat Completions. Unknown models default to chat
/// completions, which is where OpenCode adds new open-model families.
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

pub(crate) fn opencode_go_headers(session: Option<&str>) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(concat!("gospel/", env!("CARGO_PKG_VERSION"))),
    );
    let session = session
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Ok(value) = reqwest::header::HeaderValue::from_str(&session) {
        headers.insert("x-opencode-session", value);
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DispatchSnapshot {
        host: String,
        authorization: String,
        token_auth: String,
        client_id: String,
        client_version: String,
        accept: String,
    }

    fn dispatch_snapshot(base_url: &str, headers: &reqwest::header::HeaderMap) -> DispatchSnapshot {
        DispatchSnapshot {
            host: base_url.to_string(),
            authorization: header_value(headers, reqwest::header::AUTHORIZATION),
            token_auth: header_value(headers, "x-xai-token-auth"),
            client_id: header_value(headers, "x-grok-client-identifier"),
            client_version: header_value(headers, "x-grok-client-version"),
            accept: header_value(headers, "accept"),
        }
    }

    fn header_value(
        headers: &reqwest::header::HeaderMap,
        name: impl reqwest::header::AsHeaderName,
    ) -> String {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string()
    }

    fn assert_subscription_path(snapshot: &DispatchSnapshot, access_token: &str) {
        assert_eq!(snapshot.host, "https://cli-chat-proxy.grok.com");
        assert!(!snapshot.host.contains("api.x.ai"));
        assert_eq!(snapshot.authorization, format!("Bearer {access_token}"));
        assert_eq!(snapshot.token_auth, "xai-grok-cli");
        assert_eq!(snapshot.client_id, "grok-shell");
        assert_eq!(snapshot.client_version, "0.2.93");
        assert_eq!(snapshot.accept, "text/event-stream");
    }

    fn assert_subscription_client(client: &rig::providers::xai::Client, access_token: &str) {
        assert_subscription_path(
            &dispatch_snapshot(client.base_url(), client.headers()),
            access_token,
        );
    }

    async fn grok_dispatch_snapshot(api_key: &str) -> Result<DispatchSnapshot, String> {
        provider_client!("grok", "grok-4", api_key, None, |e: String| e, |s: String| s, |client| {
            Ok(dispatch_snapshot(client.base_url(), client.headers()))
        })
    }

    #[tokio::test]
    async fn grok_dispatch_constructs_subscription_client() {
        let snapshot = grok_dispatch_snapshot("grok-oauth-access-token")
            .await
            .unwrap();
        assert_subscription_path(&snapshot, "grok-oauth-access-token");
    }

    #[tokio::test]
    async fn grok_dispatch_uses_stored_oauth_provider_credential() {
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

        let snapshot = grok_dispatch_snapshot("").await.unwrap();
        assert_subscription_path(&snapshot, "stored-grok-oauth-token");
    }

    #[test]
    fn opencode_go_protocol_maps_documented_model_families() {
        use OpencodeGoProtocol::*;

        for model in ["minimax-m3", "minimax-m2.7", "qwen3.8-max", "qwen3.8-flash", "qwen3.7-plus"] {
            assert_eq!(opencode_go_protocol(model), AnthropicMessages, "{model}");
        }
        for model in ["grok-4.7", "grok-4.6", "gpt-6-luna", "gpt-5.6-luna", "muse-spark-1.3-contributor"] {
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
            assert_eq!(opencode_go_protocol(model), OpenAiChatCompletions, "{model}");
        }
    }

    #[test]
    fn opencode_go_headers_identify_client_and_session() {
        let headers = opencode_go_headers(Some("conversation-123"));
        assert_eq!(
            headers.get("x-opencode-session").and_then(|v| v.to_str().ok()),
            Some("conversation-123")
        );
        let user_agent = headers
            .get(reqwest::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert!(user_agent.starts_with("gospel/"), "user agent: {user_agent}");

        let generated = opencode_go_headers(None);
        assert!(generated.get("x-opencode-session").is_some());
    }

    async fn opencode_go_dispatch_base_url(model: &str) -> Result<String, String> {
        provider_client!(
            "opencode_go",
            model,
            "sk-test",
            Some("conversation-123"),
            |e: String| e,
            |s: String| s,
            |client| { Ok(client.base_url().to_string()) }
        )
    }

    #[tokio::test]
    async fn opencode_go_dispatch_routes_models_to_documented_endpoints() {
        assert_eq!(
            opencode_go_dispatch_base_url("glm-5.3").await.unwrap(),
            "https://opencode.ai/zen/go/v1"
        );
        assert_eq!(
            opencode_go_dispatch_base_url("grok-4.7").await.unwrap(),
            "https://opencode.ai/zen/go/v1"
        );
        assert_eq!(
            opencode_go_dispatch_base_url("minimax-m3").await.unwrap(),
            "https://opencode.ai/zen/go"
        );
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
        let client = grok_subscription_client(&token).unwrap();

        assert_eq!(token, "stored-grok-oauth-token");
        assert_subscription_client(&client, "stored-grok-oauth-token");
    }
}
