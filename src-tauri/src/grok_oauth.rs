use serde_json::Value;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::time::{Duration, Instant};

pub const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const DEVICE_CODE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const SCOPE: &str =
    "openid profile email offline_access grok-cli:access api:access conversations:read conversations:write";
const DEFAULT_DEVICE_CODE_URL: &str = "https://auth.x.ai/oauth2/device/code";
const DEFAULT_TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
const DEFAULT_EXPIRES_SECS: u64 = 900;
const DEFAULT_INTERVAL_SECS: u64 = 5;
const MIN_INTERVAL_SECS: u64 = 1;
const MAX_INTERVAL_SECS: u64 = 30;
const SLOW_DOWN_INCREMENT_SECS: u64 = 5;

type FormPostFuture<'a> = Pin<Box<dyn Future<Output = Result<(u16, Value), String>> + Send + 'a>>;

pub struct Endpoints {
    pub device_code_url: String,
    pub token_url: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            device_code_url: DEFAULT_DEVICE_CODE_URL.to_string(),
            token_url: DEFAULT_TOKEN_URL.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
}

struct DeviceCode {
    device_code: String,
    user_code: String,
    verification_url: String,
    expires_in: u64,
    interval: u64,
}

pub trait FormPoster: Send + Sync {
    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> FormPostFuture<'_>;
}

pub struct ReqwestPoster;

impl FormPoster for ReqwestPoster {
    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> FormPostFuture<'_> {
        let url = url.to_string();
        let form = form
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect::<Vec<_>>();
        Box::pin(async move {
            let response = reqwest::Client::new()
                .post(&url)
                .header("Accept", "application/json")
                .form(&form)
                .send()
                .await
                .map_err(|e| format!("Grok OAuth request failed: {e}"))?;
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            let value = serde_json::from_str(&text).unwrap_or(Value::Null);
            Ok((status, value))
        })
    }
}

pub async fn login(
    poster: &dyn FormPoster,
    endpoints: &Endpoints,
    auth_path: &Path,
    on_device_code: impl Fn(String, String),
) -> Result<(), String> {
    let challenge = request_device_code(poster, &endpoints.device_code_url).await?;
    on_device_code(
        challenge.verification_url.clone(),
        challenge.user_code.clone(),
    );
    let tokens = poll_for_tokens(poster, &endpoints.token_url, &challenge).await?;
    write_tokens(auth_path, &tokens)
}

pub async fn refresh(
    poster: &dyn FormPoster,
    endpoints: &Endpoints,
    auth_path: &Path,
) -> Result<TokenPair, String> {
    let stored = read_tokens(auth_path)?;
    let tokens = refresh_tokens(poster, &endpoints.token_url, &stored.refresh_token).await?;
    write_tokens(auth_path, &tokens)?;
    Ok(tokens)
}

async fn request_device_code(
    poster: &dyn FormPoster,
    device_code_url: &str,
) -> Result<DeviceCode, String> {
    let (status, value) = poster
        .post_form(
            device_code_url,
            &[("client_id", CLIENT_ID), ("scope", SCOPE)],
        )
        .await?;
    if status != 200 {
        return Err(format!("Grok device-code request failed (HTTP {status})"));
    }
    parse_device_code(&value)
        .ok_or_else(|| "Grok device-code response missing required fields".to_string())
}

async fn poll_for_tokens(
    poster: &dyn FormPoster,
    token_url: &str,
    device: &DeviceCode,
) -> Result<TokenPair, String> {
    let deadline = Instant::now() + Duration::from_secs(device.expires_in);
    let mut interval = device.interval.max(MIN_INTERVAL_SECS);
    while Instant::now() < deadline {
        let (status, value) = poster
            .post_form(
                token_url,
                &[
                    ("grant_type", DEVICE_CODE_GRANT_TYPE),
                    ("client_id", CLIENT_ID),
                    ("device_code", device.device_code.as_str()),
                ],
            )
            .await?;
        if status == 200 {
            return parse_token_pair(&value).ok_or_else(|| {
                "Grok token response missing access_token or refresh_token".to_string()
            });
        }
        match classify_poll_error(&value) {
            PollOutcome::Pending => {}
            PollOutcome::SlowDown => {
                interval = (interval + SLOW_DOWN_INCREMENT_SECS).min(MAX_INTERVAL_SECS);
            }
            PollOutcome::Failed(reason) => return Err(reason),
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
    }
    Err("Grok device authorization timed out".to_string())
}

async fn refresh_tokens(
    poster: &dyn FormPoster,
    token_url: &str,
    refresh_token: &str,
) -> Result<TokenPair, String> {
    let (status, value) = poster
        .post_form(
            token_url,
            &[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", refresh_token),
            ],
        )
        .await?;
    if status != 200 {
        return Err(format!("Grok token refresh failed (HTTP {status})"));
    }
    parse_refreshed_tokens(&value, refresh_token)
        .ok_or_else(|| "Grok refresh response missing access_token".to_string())
}

fn parse_device_code(value: &Value) -> Option<DeviceCode> {
    let verification_url = value
        .get("verification_uri_complete")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| value.get("verification_uri").and_then(Value::as_str))?
        .to_string();
    Some(DeviceCode {
        device_code: value.get("device_code")?.as_str()?.to_string(),
        user_code: value.get("user_code")?.as_str()?.to_string(),
        verification_url,
        expires_in: positive_secs(value.get("expires_in"), DEFAULT_EXPIRES_SECS),
        interval: positive_secs(value.get("interval"), DEFAULT_INTERVAL_SECS)
            .max(MIN_INTERVAL_SECS),
    })
}

fn parse_token_pair(value: &Value) -> Option<TokenPair> {
    let access_token = nonempty_str(value.get("access_token"))?;
    let refresh_token = nonempty_str(value.get("refresh_token"))?;
    Some(TokenPair {
        access_token,
        refresh_token,
    })
}

fn parse_refreshed_tokens(value: &Value, previous_refresh_token: &str) -> Option<TokenPair> {
    let access_token = nonempty_str(value.get("access_token"))?;
    let refresh_token = nonempty_str(value.get("refresh_token"))
        .unwrap_or_else(|| previous_refresh_token.to_string());
    Some(TokenPair {
        access_token,
        refresh_token,
    })
}

fn nonempty_str(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn positive_secs(value: Option<&Value>, default: u64) -> u64 {
    let parsed = match value {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    };
    parsed.filter(|n| *n > 0).unwrap_or(default)
}

fn read_tokens(path: &Path) -> Result<TokenPair, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|_| "Grok OAuth Provider Credential not found".to_string())?;
    let value: Value = serde_json::from_str(&content)
        .map_err(|_| "Grok OAuth Provider Credential is invalid".to_string())?;
    parse_token_pair(&value)
        .ok_or_else(|| "Grok OAuth Provider Credential is missing tokens".to_string())
}

/// Returns the access token stored in the Gospel-owned Grok auth file.
pub fn access_token(path: &Path) -> Result<String, String> {
    Ok(read_tokens(path)?.access_token)
}

/// Refreshes the Grok OAuth session (when present) so subsequent API calls use a fresh access token.
///
/// Used by `model_fetch` (excluded from libtest via a stub module).
#[cfg_attr(test, allow(dead_code))]
pub async fn ensure_fresh_access_token(auth_path: &Path) -> Result<String, String> {
    if !auth_path.exists() {
        return Err("Grok OAuth Provider Credential not found".to_string());
    }
    let tokens = refresh(&ReqwestPoster, &Endpoints::default(), auth_path).await?;
    Ok(tokens.access_token)
}

fn write_tokens(path: &Path, tokens: &TokenPair) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create Grok auth directory: {e}"))?;
    }
    let value = serde_json::json!({
        "access_token": tokens.access_token,
        "refresh_token": tokens.refresh_token,
    });
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&value).map_err(|e| e.to_string())?)
        .map_err(|e| format!("failed to write Grok auth file: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("failed to set Grok auth file permissions: {e}"))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("failed to persist Grok auth file: {e}"))
}

enum PollOutcome {
    Pending,
    SlowDown,
    Failed(String),
}

fn classify_poll_error(value: &Value) -> PollOutcome {
    match value.get("error").and_then(Value::as_str).unwrap_or("") {
        "authorization_pending" => PollOutcome::Pending,
        "slow_down" => PollOutcome::SlowDown,
        "access_denied" | "authorization_denied" => {
            PollOutcome::Failed("Grok device authorization was denied".to_string())
        }
        "expired_token" => PollOutcome::Failed("Grok device code expired".to_string()),
        other if !other.is_empty() => {
            PollOutcome::Failed(format!("Grok device authorization failed: {other}"))
        }
        _ => PollOutcome::Failed("Grok device authorization failed".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    struct ScriptedPoster {
        remaining: Mutex<Vec<(String, u16, Value)>>,
        seen: Mutex<Vec<(String, Vec<(String, String)>)>>,
    }

    impl ScriptedPoster {
        fn new(responses: Vec<(&str, u16, Value)>) -> Self {
            Self {
                remaining: Mutex::new(
                    responses
                        .into_iter()
                        .map(|(url, status, body)| (url.to_string(), status, body))
                        .collect(),
                ),
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    impl FormPoster for ScriptedPoster {
        fn post_form(&self, url: &str, form: &[(&str, &str)]) -> FormPostFuture<'_> {
            self.seen.lock().unwrap().push((
                url.to_string(),
                form.iter()
                    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                    .collect(),
            ));
            let next = self.remaining.lock().unwrap().first().cloned();
            let result = match next {
                Some((expected_url, status, body)) if expected_url == url => {
                    self.remaining.lock().unwrap().remove(0);
                    Ok((status, body))
                }
                Some((expected_url, _, _)) => Err(format!("expected {expected_url}, got {url}")),
                None => Err(format!("unexpected Grok OAuth request to {url}")),
            };
            Box::pin(async move { result })
        }
    }

    #[tokio::test]
    async fn successful_login_stores_oauth_provider_credential() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        let poster = ScriptedPoster::new(vec![
            (
                "https://auth.test/device/code",
                200,
                json!({
                    "device_code": "dev-1",
                    "user_code": "ABCD-1234",
                    "verification_uri": "https://accounts.x.ai/device",
                    "verification_uri_complete": "https://accounts.x.ai/device?user_code=ABCD-1234",
                    "expires_in": 600,
                    "interval": 5
                }),
            ),
            (
                "https://auth.test/token",
                200,
                json!({
                    "access_token": "grok-access-1",
                    "refresh_token": "grok-refresh-1"
                }),
            ),
        ]);
        let seen = Arc::new(Mutex::new(None));
        let seen_clone = Arc::clone(&seen);

        login(&poster, &test_endpoints(), &path, |url, code| {
            *seen_clone.lock().unwrap() = Some((url, code));
        })
        .await
        .unwrap();

        assert_eq!(
            seen.lock().unwrap().clone(),
            Some((
                "https://accounts.x.ai/device?user_code=ABCD-1234".to_string(),
                "ABCD-1234".to_string()
            ))
        );
        let stored: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(stored["access_token"], "grok-access-1");
        assert_eq!(stored["refresh_token"], "grok-refresh-1");
        assert_eq!(
            poster.seen.lock().unwrap()[0],
            (
                "https://auth.test/device/code".to_string(),
                vec![
                    (
                        "client_id".to_string(),
                        "b1a00492-073a-47ea-816f-4c329264a828".to_string()
                    ),
                    (
                        "scope".to_string(),
                        "openid profile email offline_access grok-cli:access api:access conversations:read conversations:write".to_string()
                    ),
                ]
            )
        );
        assert_eq!(
            poster.seen.lock().unwrap()[1],
            (
                "https://auth.test/token".to_string(),
                vec![
                    (
                        "grant_type".to_string(),
                        "urn:ietf:params:oauth:grant-type:device_code".to_string()
                    ),
                    (
                        "client_id".to_string(),
                        "b1a00492-073a-47ea-816f-4c329264a828".to_string()
                    ),
                    ("device_code".to_string(), "dev-1".to_string()),
                ]
            )
        );
    }

    #[tokio::test]
    async fn login_continues_past_authorization_pending() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        let poster = ScriptedPoster::new(vec![
            (
                "https://auth.test/device/code",
                200,
                json!({
                    "device_code": "dev-1",
                    "user_code": "WXYZ-9876",
                    "verification_uri": "https://accounts.x.ai/device",
                    "expires_in": 30,
                    "interval": 1
                }),
            ),
            (
                "https://auth.test/token",
                400,
                json!({ "error": "authorization_pending" }),
            ),
            (
                "https://auth.test/token",
                200,
                json!({
                    "access_token": "grok-access-2",
                    "refresh_token": "grok-refresh-2"
                }),
            ),
        ]);

        login(&poster, &test_endpoints(), &path, |_, _| {})
            .await
            .unwrap();

        let stored: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(stored["access_token"], "grok-access-2");
        assert_eq!(stored["refresh_token"], "grok-refresh-2");
    }

    #[tokio::test]
    async fn refresh_persists_rotated_refresh_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        write_tokens(
            &path,
            &TokenPair {
                access_token: "old-access".to_string(),
                refresh_token: "old-refresh".to_string(),
            },
        )
        .unwrap();
        let poster = ScriptedPoster::new(vec![(
            "https://auth.test/token",
            200,
            json!({
                "access_token": "new-access",
                "refresh_token": "rotated-refresh"
            }),
        )]);

        let tokens = refresh(&poster, &test_endpoints(), &path).await.unwrap();

        assert_eq!(
            tokens,
            TokenPair {
                access_token: "new-access".to_string(),
                refresh_token: "rotated-refresh".to_string(),
            }
        );
        let stored: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(stored["access_token"], "new-access");
        assert_eq!(stored["refresh_token"], "rotated-refresh");
        assert_eq!(
            poster.seen.lock().unwrap()[0],
            (
                "https://auth.test/token".to_string(),
                vec![
                    ("grant_type".to_string(), "refresh_token".to_string()),
                    (
                        "client_id".to_string(),
                        "b1a00492-073a-47ea-816f-4c329264a828".to_string()
                    ),
                    ("refresh_token".to_string(), "old-refresh".to_string()),
                ]
            )
        );
    }

    #[tokio::test]
    async fn refresh_without_rotated_token_keeps_existing_refresh_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        write_tokens(
            &path,
            &TokenPair {
                access_token: "old-access".to_string(),
                refresh_token: "old-refresh".to_string(),
            },
        )
        .unwrap();
        let poster = ScriptedPoster::new(vec![(
            "https://auth.test/token",
            200,
            json!({ "access_token": "new-access" }),
        )]);

        let tokens = refresh(&poster, &test_endpoints(), &path).await.unwrap();

        assert_eq!(
            tokens,
            TokenPair {
                access_token: "new-access".to_string(),
                refresh_token: "old-refresh".to_string(),
            }
        );
        let stored: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(stored["access_token"], "new-access");
        assert_eq!(stored["refresh_token"], "old-refresh");
    }

    fn test_endpoints() -> Endpoints {
        Endpoints {
            device_code_url: "https://auth.test/device/code".to_string(),
            token_url: "https://auth.test/token".to_string(),
        }
    }
}
