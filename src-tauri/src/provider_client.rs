/// Dispatches to the correct rig provider client based on the provider string.
///
/// Usage:
/// ```ignore
/// provider_client!(provider, api_key, client_err, unsupported_err, |client| {
///     let agent = client.agent(model).build();
///     agent.prompt(prompt).await.map_err(client_err)?
/// })
/// ```
///
/// - `$client_err`: expression converting client construction `String` errors
/// - `$unsupported_err`: expression converting the unsupported-provider `String`
macro_rules! provider_client {
    ($provider:expr, $api_key:expr, $client_err:expr, $unsupported_err:expr, |$client:ident| $body:block) => {
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
                let access_token = $crate::provider_client::grok_dispatch_access_token($api_key)
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

pub(crate) async fn grok_dispatch_access_token(api_key: &str) -> Result<String, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_subscription_client(client: &rig::providers::xai::Client, access_token: &str) {
        let expected_auth = format!("Bearer {access_token}");
        assert_eq!(client.base_url(), "https://cli-chat-proxy.grok.com");
        assert!(!client.base_url().contains("api.x.ai"));
        assert_eq!(
            client
                .headers()
                .get(reqwest::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok()),
            Some(expected_auth.as_str())
        );
        assert_eq!(
            client
                .headers()
                .get("x-xai-token-auth")
                .and_then(|v| v.to_str().ok()),
            Some("xai-grok-cli")
        );
        assert_eq!(
            client
                .headers()
                .get("x-grok-client-identifier")
                .and_then(|v| v.to_str().ok()),
            Some("grok-shell")
        );
        assert_eq!(
            client
                .headers()
                .get("x-grok-client-version")
                .and_then(|v| v.to_str().ok()),
            Some("0.2.93")
        );
        assert_eq!(
            client.headers().get("accept").and_then(|v| v.to_str().ok()),
            Some("text/event-stream")
        );
    }

    struct DispatchSnapshot {
        host: String,
        authorization: String,
        token_auth: String,
        client_id: String,
        client_version: String,
        accept: String,
    }

    #[tokio::test]
    async fn grok_dispatch_constructs_subscription_client() {
        async fn dispatch() -> Result<DispatchSnapshot, String> {
            provider_client!(
                "grok",
                "grok-oauth-access-token",
                |e: String| e,
                |s: String| s,
                |client| {
                    Ok(DispatchSnapshot {
                        host: client.base_url().to_string(),
                        authorization: header_value(
                            client.headers(),
                            reqwest::header::AUTHORIZATION,
                        ),
                        token_auth: header_value(client.headers(), "x-xai-token-auth"),
                        client_id: header_value(client.headers(), "x-grok-client-identifier"),
                        client_version: header_value(client.headers(), "x-grok-client-version"),
                        accept: header_value(client.headers(), "accept"),
                    })
                }
            )
        }

        let snapshot = dispatch().await.unwrap();

        assert_eq!(snapshot.host, "https://cli-chat-proxy.grok.com");
        assert!(!snapshot.host.contains("api.x.ai"));
        assert_eq!(snapshot.authorization, "Bearer grok-oauth-access-token");
        assert_eq!(snapshot.token_auth, "xai-grok-cli");
        assert_eq!(snapshot.client_id, "grok-shell");
        assert_eq!(snapshot.client_version, "0.2.93");
        assert_eq!(snapshot.accept, "text/event-stream");
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

    #[test]
    fn stored_oauth_credential_constructs_subscription_client() {
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
