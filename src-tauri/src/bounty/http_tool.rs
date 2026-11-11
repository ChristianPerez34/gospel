use super::scope::{BountyScope, ScopeError};
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use reqwest::{Method, Url};
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;
use uuid::Uuid;

const DEFAULT_METHOD: &str = "GET";
const BODY_PREVIEW_BYTES: usize = 8 * 1024;
const MAX_TIMEOUT_MS: u64 = 60_000;
const MAX_REDIRECT_HOPS: usize = 5;

#[derive(Error, Debug)]
pub enum BountyHttpError {
    #[error("bounty scope error: {0}")]
    Scope(#[from] ScopeError),
    #[error("failed to build HTTP client: {0}")]
    Client(String),
    #[error("failed to prepare evidence directory: {0}")]
    Evidence(String),
}

#[derive(Debug, Clone)]
pub struct BountyHttpTool {
    pub scope: Option<Arc<BountyScope>>,
    pub scope_error: Option<String>,
    run_dir: PathBuf,
    client: reqwest::Client,
    rate_limiter: Arc<HostRateLimiter>,
    evidence_dir: PathBuf,
    audit_path: PathBuf,
    max_response_bytes: usize,
}

impl BountyHttpTool {
    pub fn new(workspace_path: &Path, run_id: &str) -> Result<Self, BountyHttpError> {
        let (scope, scope_error) = match BountyScope::load(workspace_path) {
            Ok(scope) => (Some(Arc::new(scope)), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let run_dir = run_dir(workspace_path, run_id);
        let evidence_dir = run_dir.join("evidence");
        std::fs::create_dir_all(&evidence_dir)
            .map_err(|error| BountyHttpError::Evidence(error.to_string()))?;
        let audit_path = run_dir.join("requests.jsonl");

        // Redirects are followed manually so every hop re-passes scope.decide.
        let redirect_policy = reqwest::redirect::Policy::none();

        let client = reqwest::Client::builder()
            .user_agent("gospel-bounty/0.1")
            .redirect(redirect_policy)
            .timeout(
                scope
                    .as_ref()
                    .map(|scope| scope.timeout)
                    .unwrap_or(Duration::from_secs(30)),
            )
            .build()
            .map_err(|error| BountyHttpError::Client(error.to_string()))?;

        let requests_per_second = scope
            .as_ref()
            .map(|scope| scope.rate_limit.requests_per_second)
            .filter(|value| *value > 0.0)
            .unwrap_or(1.0);
        let rate_limiter = Arc::new(HostRateLimiter::new(Duration::from_secs_f64(
            1.0 / requests_per_second,
        )));
        let max_response_bytes = scope
            .as_ref()
            .map(|scope| scope.max_response_bytes)
            .unwrap_or(1024 * 1024);

        Ok(Self {
            scope,
            scope_error,
            run_dir,
            client,
            rate_limiter,
            evidence_dir,
            audit_path,
            max_response_bytes,
        })
    }

    fn audit(&self, record: &AuditRecord) {
        let Ok(line) = serde_json::to_string(record) else {
            return;
        };
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.audit_path)
        {
            let _ = writeln!(file, "{line}");
        }
    }

    fn write_evidence(&self, record: &EvidenceRecord) -> Result<String, String> {
        let evidence_id = Uuid::new_v4().to_string();
        let path = self.evidence_dir.join(format!("{evidence_id}.json"));
        let payload =
            serde_json::to_string_pretty(record).map_err(|error| error.to_string())?;
        std::fs::write(&path, payload).map_err(|error| error.to_string())?;
        Ok(evidence_id)
    }
}

pub(crate) fn sanitize_run_id(raw: &str) -> String {
    let sanitized: String = raw
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-' || *character == '_')
        .collect();
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

/// The per-run artifact directory under `.gospel/bounty/runs/<run-id>/`.
pub(crate) fn run_dir(workspace_path: &Path, run_id: &str) -> PathBuf {
    workspace_path
        .join(".gospel")
        .join("bounty")
        .join("runs")
        .join(sanitize_run_id(run_id))
}

#[derive(Debug)]
struct HostRateLimiter {
    interval: Duration,
    gates: Mutex<HashMap<String, Instant>>,
}

impl HostRateLimiter {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            gates: Mutex::new(HashMap::new()),
        }
    }

    async fn wait(&self, host: &str) {
        let sleep_for = {
            let mut gates = self.gates.lock().unwrap();
            let now = Instant::now();
            let next = gates.entry(host.to_string()).or_insert(now);
            let wait = if *next > now {
                *next - now
            } else {
                Duration::ZERO
            };
            *next = now + wait + self.interval;
            wait
        };
        if !sleep_for.is_zero() {
            tokio::time::sleep(sleep_for).await;
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct BountyHttpArgs {
    #[serde(default = "default_method")]
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub auth_profile: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

fn default_method() -> String {
    DEFAULT_METHOD.to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct HeaderRecord {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct BountyHttpOutput {
    pub success: bool,
    pub denied: bool,
    pub status: Option<u16>,
    pub url: String,
    pub evidence_id: Option<String>,
    pub summary: String,
    pub response_headers: Vec<HeaderRecord>,
    pub body_preview: Option<String>,
    pub body_truncated: bool,
    #[serde(default)]
    pub redirects: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
struct AuditRecord {
    timestamp: String,
    method: String,
    url: String,
    status: Option<u16>,
    duration_ms: u128,
    evidence_id: Option<String>,
    denied: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct EvidenceRecord {
    timestamp: String,
    program: String,
    method: String,
    url: String,
    request_headers: Vec<HeaderRecord>,
    request_body: Option<String>,
    status: u16,
    response_headers: Vec<HeaderRecord>,
    response_body: String,
    response_body_truncated: bool,
    duration_ms: u128,
}

fn header_record(name: &str, value: &str) -> HeaderRecord {
    HeaderRecord {
        name: name.to_string(),
        value: redact_header_value(name, value),
    }
}

fn redact_header_value(name: &str, value: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "authorization"
            | "cookie"
            | "set-cookie"
            | "proxy-authorization"
            | "x-api-key"
            | "x-auth-token"
    ) {
        "[REDACTED]".to_string()
    } else {
        value.to_string()
    }
}

fn redact_request_headers(headers: &HashMap<String, String>) -> Vec<HeaderRecord> {
    let mut records: Vec<HeaderRecord> = headers
        .iter()
        .map(|(name, value)| header_record(name, value))
        .collect();
    records.sort_by(|a, b| a.name.cmp(&b.name));
    records
}

fn redact_response_headers(headers: &HeaderMap) -> Vec<HeaderRecord> {
    let mut records: Vec<HeaderRecord> = headers
        .iter()
        .map(|(name, value)| {
            header_record(
                name.as_str(),
                value.to_str().unwrap_or("<non-utf8 header value>"),
            )
        })
        .collect();
    records.sort_by(|a, b| a.name.cmp(&b.name));
    records
}

fn redact_body(body: &str) -> String {
    use std::sync::LazyLock;
    static SECRET_PAIR: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r#"(?i)("?(?:password|passwd|secret|token|api[_-]?key|access[_-]?token|refresh[_-]?token|authorization|cookie)"?\s*[:=]\s*)("[^"]*"|[^&\s,}]+)"#,
        )
        .expect("secret redaction regex is valid")
    });
    SECRET_PAIR
        .replace_all(body, "${1}[REDACTED]")
        .into_owned()
}

fn redact_url(url: &str) -> String {
    redact_body(url)
}

fn build_request_headers(
    user_headers: Option<&HashMap<String, String>>,
    auth_profile: Option<&str>,
    scope: &BountyScope,
) -> Result<HashMap<String, String>, String> {
    let mut headers = user_headers.cloned().unwrap_or_default();
    if let Some(profile_name) = auth_profile {
        let profile = scope.auth_profiles.get(profile_name).ok_or_else(|| {
            let available = scope
                .auth_profiles
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "auth profile '{profile_name}' is not defined in the scope file; available: [{}]",
                if available.is_empty() {
                    "none".to_string()
                } else {
                    available
                }
            )
        })?;
        let (name, value) = profile
            .header_pair(profile_name)
            .map_err(|error| error.to_string())?;
        headers.insert(name, value);
    }
    Ok(headers)
}

fn apply_headers(
    mut request: reqwest::RequestBuilder,
    headers: &HashMap<String, String>,
) -> Result<reqwest::RequestBuilder, String> {
    for (name, value) in headers {
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| format!("invalid header name '{name}': {error}"))?;
        if matches!(header_name.as_str(), "host" | "content-length") {
            continue;
        }
        let header_value = HeaderValue::from_str(value)
            .map_err(|error| format!("invalid header value for '{name}': {error}"))?;
        request = request.header(header_name, header_value);
    }
    Ok(request)
}

async fn read_capped_body(
    response: reqwest::Response,
    cap: usize,
) -> Result<(Vec<u8>, bool), String> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    let mut truncated = false;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| error.to_string())?;
        if body.len() + chunk.len() > cap {
            let remaining = cap.saturating_sub(body.len());
            body.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, truncated))
}

impl Tool for BountyHttpTool {
    const NAME: &'static str = "http_request";

    type Error = BountyHttpError;
    type Args = BountyHttpArgs;
    type Output = BountyHttpOutput;

    fn description(&self) -> String {
        let program = self
            .scope
            .as_ref()
            .map(|scope| scope.program.as_str())
            .unwrap_or("unconfigured");
        format!(
            "Send one scoped HTTP request to an authorized bug-bounty target (program '{program}'). Requests outside the workspace scope file are refused. Returns status, headers, a capped body preview, and an evidence id for the full redacted request/response record."
        )
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "method": {
                    "type": "string",
                    "description": "HTTP method. Defaults to GET. Mutating methods require `allow_mutations: true` in the scope file."
                },
                "url": {
                    "type": "string",
                    "description": "Absolute URL inside the scope file's in-scope hosts, paths, methods, and ports."
                },
                "headers": {
                    "type": "object",
                    "description": "Optional request headers. Authorization/Cookie headers are redacted from evidence."
                },
                "body": {
                    "type": "string",
                    "description": "Optional request body."
                },
                "auth_profile": {
                    "type": "string",
                    "description": "Optional named auth profile from the scope file. The secret is resolved at request time and never returned."
                },
                "content_type": {
                    "type": "string",
                    "description": "Optional Content-Type header."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Optional per-request timeout in milliseconds, capped at 60000."
                }
            },
            "required": ["url"]
        })
    }

    fn map_error(&self, error: Self::Error) -> ToolExecutionError {
        ToolExecutionError::other(error.to_string())
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let Some(scope) = self.scope.as_ref() else {
            let reason = self
                .scope_error
                .clone()
                .unwrap_or_else(|| "no bounty scope is configured".to_string());
            self.audit(&AuditRecord {
                timestamp: chrono::Utc::now().to_rfc3339(),
                method: args.method.clone(),
                url: redact_url(args.url.trim()),
                status: None,
                duration_ms: 0,
                evidence_id: None,
                denied: true,
                error: Some(reason.clone()),
            });
            return Ok(BountyHttpOutput {
                success: false,
                denied: true,
                status: None,
                url: redact_url(args.url.trim()),
                evidence_id: None,
                summary: format!("Request denied: {reason}"),
                response_headers: Vec::new(),
                body_preview: None,
                body_truncated: false,
                redirects: Vec::new(),
                error: Some(reason),
            });
        };

        let method = args.method.trim().to_ascii_uppercase();
        let method = Method::from_bytes(method.as_bytes()).map_err(|error| {
            BountyHttpError::Client(format!("invalid HTTP method '{}': {error}", args.method))
        })?;
        let url = Url::parse(args.url.trim()).map_err(|error| {
            BountyHttpError::Client(format!("invalid URL '{}': {error}", args.url))
        })?;

        // A pending threat-model checkpoint freezes all in-scope traffic until
        // a human approves it (capability-enforced, not prompt-enforced).
        if super::checkpoint::checkpoint_state(&self.run_dir)
            == super::checkpoint::CheckpointState::Pending
        {
            let reason =
                "threat-model checkpoint is pending human approval; http_request is refused until approved"
                    .to_string();
            self.audit(&AuditRecord {
                timestamp: chrono::Utc::now().to_rfc3339(),
                method: method.as_str().to_string(),
                url: redact_url(url.as_str()),
                status: None,
                duration_ms: 0,
                evidence_id: None,
                denied: true,
                error: Some(reason.clone()),
            });
            return Ok(BountyHttpOutput {
                success: false,
                denied: true,
                status: None,
                url: redact_url(url.as_str()),
                evidence_id: None,
                summary: format!("Request denied: {reason}"),
                response_headers: Vec::new(),
                body_preview: None,
                body_truncated: false,
                redirects: Vec::new(),
                error: Some(reason),
            });
        }

        if let Err(reason) = scope.decide(method.as_str(), &url) {
            self.audit(&AuditRecord {
                timestamp: chrono::Utc::now().to_rfc3339(),
                method: method.as_str().to_string(),
                url: redact_url(url.as_str()),
                status: None,
                duration_ms: 0,
                evidence_id: None,
                denied: true,
                error: Some(reason.clone()),
            });
            return Ok(BountyHttpOutput {
                success: false,
                denied: true,
                status: None,
                url: redact_url(url.as_str()),
                evidence_id: None,
                summary: format!("Request denied by scope: {reason}"),
                response_headers: Vec::new(),
                body_preview: None,
                body_truncated: false,
                redirects: Vec::new(),
                error: Some(reason),
            });
        }

        let headers = match build_request_headers(
            args.headers.as_ref(),
            args.auth_profile.as_deref(),
            scope,
        ) {
            Ok(headers) => headers,
            Err(error) => {
                self.audit(&AuditRecord {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    method: method.as_str().to_string(),
                    url: redact_url(url.as_str()),
                    status: None,
                    duration_ms: 0,
                    evidence_id: None,
                    denied: false,
                    error: Some(error.clone()),
                });
                return Ok(BountyHttpOutput {
                    success: false,
                    denied: false,
                    status: None,
                    url: redact_url(url.as_str()),
                    evidence_id: None,
                    summary: format!("Request not sent: {error}"),
                    response_headers: Vec::new(),
                    body_preview: None,
                    body_truncated: false,
                    redirects: Vec::new(),
                    error: Some(error),
                });
            }
        };

        let mut current_url = url.clone();
        let mut current_method = method.clone();
        let mut current_body = args.body.clone();
        let mut current_headers = headers.clone();
        let mut redirects: Vec<String> = Vec::new();

        let started = Instant::now();
        let response = loop {
            if let Some(host) = current_url.host_str() {
                self.rate_limiter.wait(host).await;
            }

            let mut request = self
                .client
                .request(current_method.clone(), current_url.clone());
            request = match apply_headers(request, &current_headers) {
                Ok(request) => request,
                Err(error) => {
                    return Ok(BountyHttpOutput {
                        success: false,
                        denied: false,
                        status: None,
                        url: redact_url(current_url.as_str()),
                        evidence_id: None,
                        summary: format!("Request not sent: {error}"),
                        response_headers: Vec::new(),
                        body_preview: None,
                        body_truncated: false,
                        redirects,
                        error: Some(error),
                    });
                }
            };
            if let Some(content_type) = args.content_type.as_deref() {
                request = request.header(CONTENT_TYPE, content_type);
            }
            if let Some(body) = current_body.as_ref() {
                request = request.body(body.clone());
            }
            if let Some(timeout_ms) = args.timeout_ms {
                request = request.timeout(Duration::from_millis(timeout_ms.min(MAX_TIMEOUT_MS)));
            }

            let response = match request.send().await {
                Ok(response) => response,
                Err(error) => {
                    let message = error.to_string();
                    self.audit(&AuditRecord {
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        method: current_method.as_str().to_string(),
                        url: redact_url(current_url.as_str()),
                        status: None,
                        duration_ms: started.elapsed().as_millis(),
                        evidence_id: None,
                        denied: false,
                        error: Some(message.clone()),
                    });
                    return Ok(BountyHttpOutput {
                        success: false,
                        denied: false,
                        status: None,
                        url: redact_url(current_url.as_str()),
                        evidence_id: None,
                        summary: format!("Request failed: {message}"),
                        response_headers: Vec::new(),
                        body_preview: None,
                        body_truncated: false,
                        redirects,
                        error: Some(message),
                    });
                }
            };

            let status = response.status().as_u16();
            let is_redirect = matches!(status, 301 | 302 | 303 | 307 | 308);
            if !is_redirect || !scope.follow_redirects {
                break response;
            }
            let Some(location) = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
            else {
                break response;
            };
            if redirects.len() >= MAX_REDIRECT_HOPS {
                redirects.push(format!("redirect cap ({MAX_REDIRECT_HOPS}) reached"));
                break response;
            }
            let next_method = if status == 303
                || ((status == 301 || status == 302)
                    && !matches!(current_method, Method::GET | Method::HEAD))
            {
                Method::GET
            } else {
                current_method.clone()
            };
            let next_url = match current_url.join(location) {
                Ok(url) => url,
                Err(error) => {
                    redirects.push(format!("unresolvable Location '{location}': {error}"));
                    break response;
                }
            };
            if let Err(reason) = scope.decide(next_method.as_str(), &next_url) {
                self.audit(&AuditRecord {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    method: next_method.as_str().to_string(),
                    url: redact_url(next_url.as_str()),
                    status: Some(status),
                    duration_ms: started.elapsed().as_millis(),
                    evidence_id: None,
                    denied: true,
                    error: Some(format!("redirect target denied: {reason}")),
                });
                redirects.push(format!(
                    "{} -> {} (denied by scope: {reason})",
                    status,
                    redact_url(next_url.as_str())
                ));
                break response;
            }
            // Authorization/Cookie must not leak across a host change.
            if next_url.host_str() != current_url.host_str() {
                current_headers.retain(|name, _| {
                    !matches!(
                        name.to_ascii_lowercase().as_str(),
                        "authorization" | "cookie"
                    )
                });
            }
            if next_method != current_method {
                current_body = None;
            }
            redirects.push(format!("{status} -> {}", redact_url(next_url.as_str())));
            current_method = next_method;
            current_url = next_url;
        };
        let url = current_url;
        let method = current_method;

        let status = response.status().as_u16();
        let response_headers = redact_response_headers(response.headers());
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let (body_bytes, body_truncated) =
            match read_capped_body(response, self.max_response_bytes).await {
                Ok(result) => result,
                Err(error) => {
                    self.audit(&AuditRecord {
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        method: method.as_str().to_string(),
                        url: redact_url(url.as_str()),
                        status: Some(status),
                        duration_ms: started.elapsed().as_millis(),
                        evidence_id: None,
                        denied: false,
                        error: Some(error.clone()),
                    });
                    return Ok(BountyHttpOutput {
                        success: false,
                        denied: false,
                        status: Some(status),
                        url: redact_url(url.as_str()),
                        evidence_id: None,
                        summary: format!("Response body read failed: {error}"),
                        response_headers,
                        body_preview: None,
                        body_truncated: false,
                        redirects,
                        error: Some(error),
                    });
                }
            };
        let duration_ms = started.elapsed().as_millis();
        let body_text = String::from_utf8_lossy(&body_bytes).into_owned();
        let redacted_body = redact_body(&body_text);
        let (preview, preview_truncated) = truncate_utf8(&redacted_body, BODY_PREVIEW_BYTES);
        let body_truncated = body_truncated || preview_truncated;

        let evidence = EvidenceRecord {
            timestamp: chrono::Utc::now().to_rfc3339(),
            program: scope.program.clone(),
            method: method.as_str().to_string(),
            url: redact_url(url.as_str()),
            request_headers: redact_request_headers(&headers),
            request_body: args.body.as_ref().map(|body| redact_body(body)),
            status,
            response_headers: response_headers.clone(),
            response_body: redacted_body,
            response_body_truncated: body_truncated,
            duration_ms,
        };
        let evidence_id = self.write_evidence(&evidence).ok();

        self.audit(&AuditRecord {
            timestamp: chrono::Utc::now().to_rfc3339(),
            method: method.as_str().to_string(),
            url: redact_url(url.as_str()),
            status: Some(status),
            duration_ms,
            evidence_id: evidence_id.clone(),
            denied: false,
            error: None,
        });

        let content_type_note = content_type
            .as_deref()
            .map(|value| format!(" content-type={value}"))
            .unwrap_or_default();
        Ok(BountyHttpOutput {
            success: true,
            denied: false,
            status: Some(status),
            url: redact_url(url.as_str()),
            evidence_id,
            summary: format!(
                "{} {} -> {status}{content_type_note} ({duration_ms}ms)",
                method.as_str(),
                redact_url(url.as_str())
            ),
            response_headers,
            body_preview: Some(preview),
            body_truncated,
            redirects,
            error: None,
        })
    }
}

fn truncate_utf8(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;
    use tempfile::tempdir;

    struct MockServer {
        port: u16,
        captured: Arc<Mutex<Option<String>>>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl MockServer {
        fn start(response: &'static str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let captured = Arc::new(Mutex::new(None));
            let captured_clone = captured.clone();
            let handle = std::thread::spawn(move || {
                if let Ok((mut socket, _)) = listener.accept() {
                    let mut buffer = vec![0u8; 16 * 1024];
                    let read = socket.read(&mut buffer).unwrap_or(0);
                    *captured_clone.lock().unwrap() =
                        Some(String::from_utf8_lossy(&buffer[..read]).to_string());
                    let _ = socket.write_all(response.as_bytes());
                }
            });
            Self {
                port,
                captured,
                handle: Some(handle),
            }
        }

        fn url(&self) -> String {
            format!("http://127.0.0.1:{}/", self.port)
        }

        fn finish(mut self) -> Option<String> {
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
            self.captured.lock().unwrap().clone()
        }
    }

    /// Serves a fixed response per request path across multiple connections.
    struct RedirectServer {
        port: u16,
        captured: Arc<Mutex<Vec<String>>>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl RedirectServer {
        fn start(routes: Vec<(&'static str, &'static str)>, accepts: usize) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = listener.local_addr().unwrap().port();
            let captured = Arc::new(Mutex::new(Vec::new()));
            let captured_clone = captured.clone();
            let handle = std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut served = 0;
                while served < accepts && std::time::Instant::now() < deadline {
                    let Ok((mut socket, _)) = listener.accept() else {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    };
                    let _ = socket.set_nonblocking(false);
                    let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut buffer = vec![0u8; 16 * 1024];
                    let read = socket.read(&mut buffer).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                    captured_clone.lock().unwrap().push(request.clone());
                    let path = request
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .to_string();
                    let (_, response) = routes
                        .iter()
                        .find(|(route, _)| *route == path)
                        .unwrap_or(&("**", "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"));
                    let _ = socket.write_all(response.as_bytes());
                    served += 1;
                }
            });
            Self {
                port,
                captured,
                handle: Some(handle),
            }
        }

        fn url(&self) -> String {
            format!("http://127.0.0.1:{}/", self.port)
        }

        fn finish(mut self) -> Vec<String> {
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
            self.captured.lock().unwrap().clone()
        }
    }

    fn tool_for(port: u16, extra_scope: &str) -> (tempfile::TempDir, BountyHttpTool) {
        let workspace = tempdir().unwrap();
        let path = workspace.path().join(".gospel/bounty/scope.yaml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!(
                "program: test\nauthorized: true\nallow_http: true\nallow_private_hosts: true\nallowed_ports: [{port}]\nin_scope:\n  - host: 127.0.0.1\n{extra_scope}"
            ),
        )
        .unwrap();
        let tool = BountyHttpTool::new(workspace.path(), "test-run").unwrap();
        (workspace, tool)
    }

    #[tokio::test]
    async fn sends_allowed_request_and_writes_evidence() {
        let server = MockServer::start(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}",
        );
        let (_workspace, tool) = tool_for(server.port, "");
        let output = tool
            .call(
                &mut ToolContext::default(),
                BountyHttpArgs {
                    method: "GET".to_string(),
                    url: server.url(),
                    headers: None,
                    body: None,
                    auth_profile: None,
                    content_type: None,
                    timeout_ms: None,
                },
            )
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(output.status, Some(200));
        assert!(output.evidence_id.is_some());
        assert!(output.body_preview.unwrap().contains("ok"));
        server.finish();
    }

    #[tokio::test]
    async fn denies_out_of_scope_host_without_sending() {
        let (_workspace, tool) = tool_for(443, "");
        let output = tool
            .call(
                &mut ToolContext::default(),
                BountyHttpArgs {
                    method: "GET".to_string(),
                    url: "https://out-of-scope.example/".to_string(),
                    headers: None,
                    body: None,
                    auth_profile: None,
                    content_type: None,
                    timeout_ms: None,
                },
            )
            .await
            .unwrap();

        assert!(!output.success);
        assert!(output.denied);
        assert!(output.error.unwrap().contains("not in scope"));
    }

    #[tokio::test]
    async fn denies_mutation_without_scope_opt_in() {
        let (_workspace, tool) = tool_for(443, "");
        let output = tool
            .call(
                &mut ToolContext::default(),
                BountyHttpArgs {
                    method: "POST".to_string(),
                    url: "https://127.0.0.1/".to_string(),
                    headers: None,
                    body: Some("x=1".to_string()),
                    auth_profile: None,
                    content_type: None,
                    timeout_ms: None,
                },
            )
            .await
            .unwrap();

        assert!(!output.success);
        assert!(output.denied);
        assert!(output.error.unwrap().contains("allow_mutations"));
    }

    #[tokio::test]
    async fn injects_auth_profile_and_redacts_evidence() {
        let variable = format!("GOSPEL_HTTP_TEST_TOKEN_{}", std::process::id());
        std::env::set_var(&variable, "super-secret");
        let server = MockServer::start("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        let (_workspace, tool) = tool_for(
            server.port,
            &format!(
                "auth_profiles:\n  user_a:\n    type: bearer\n    secret_ref: env:{variable}\n"
            ),
        );
        let output = tool
            .call(
                &mut ToolContext::default(),
                BountyHttpArgs {
                    method: "GET".to_string(),
                    url: server.url(),
                    headers: None,
                    body: None,
                    auth_profile: Some("user_a".to_string()),
                    content_type: None,
                    timeout_ms: None,
                },
            )
            .await
            .unwrap();

        assert!(output.success);
        let request = server.finish().unwrap();
        assert!(request.contains("authorization: Bearer super-secret"));

        let evidence_path = _workspace
            .path()
            .join(".gospel/bounty/runs/test-run/evidence")
            .join(format!("{}.json", output.evidence_id.unwrap()));
        let evidence = std::fs::read_to_string(evidence_path).unwrap();
        assert!(!evidence.contains("super-secret"));
        assert!(evidence.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn caps_response_body() {
        let body = "a".repeat(64 * 1024);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let response: &'static str = Box::leak(response.into_boxed_str());
        let server = MockServer::start(response);
        let workspace = tempdir().unwrap();
        let path = workspace.path().join(".gospel/bounty/scope.yaml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!(
                "program: test\nauthorized: true\nallow_http: true\nallow_private_hosts: true\nallowed_ports: [{}]\nmax_response_bytes: 1024\nin_scope:\n  - host: 127.0.0.1",
                server.port
            ),
        )
        .unwrap();
        let tool = BountyHttpTool::new(workspace.path(), "cap-run").unwrap();
        let output = tool
            .call(
                &mut ToolContext::default(),
                BountyHttpArgs {
                    method: "GET".to_string(),
                    url: server.url(),
                    headers: None,
                    body: None,
                    auth_profile: None,
                    content_type: None,
                    timeout_ms: None,
                },
            )
            .await
            .unwrap();

        assert!(output.success);
        assert!(output.body_truncated);
        assert!(output.body_preview.unwrap().len() <= BODY_PREVIEW_BYTES);
        server.finish();
    }

    fn get_args(url: String) -> BountyHttpArgs {
        BountyHttpArgs {
            method: "GET".to_string(),
            url,
            headers: None,
            body: None,
            auth_profile: None,
            content_type: None,
            timeout_ms: None,
        }
    }

    #[tokio::test]
    async fn follows_in_scope_redirects() {
        let server = RedirectServer::start(
            vec![
                (
                    "/",
                    "HTTP/1.1 302 Found\r\nLocation: /app\r\nContent-Length: 0\r\n\r\n",
                ),
                (
                    "/app",
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello",
                ),
            ],
            2,
        );
        let (_workspace, tool) = tool_for(server.port, "\nfollow_redirects: true");
        let output = tool
            .call(&mut ToolContext::default(), get_args(server.url()))
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(output.status, Some(200));
        assert!(output.url.ends_with("/app"));
        assert_eq!(output.redirects.len(), 1);
        let requests = server.finish();
        assert_eq!(requests.len(), 2);
    }

    #[tokio::test]
    async fn stops_at_out_of_scope_redirect_target() {
        let server = RedirectServer::start(
            vec![(
                "/",
                "HTTP/1.1 302 Found\r\nLocation: http://out-of-scope.example/\r\nContent-Length: 0\r\n\r\n",
            )],
            1,
        );
        let (_workspace, tool) = tool_for(server.port, "\nfollow_redirects: true");
        let output = tool
            .call(&mut ToolContext::default(), get_args(server.url()))
            .await
            .unwrap();

        // The 302 itself is returned; the out-of-scope hop is denied and noted.
        assert_eq!(output.status, Some(302));
        assert_eq!(output.redirects.len(), 1);
        assert!(output.redirects[0].contains("denied by scope"));
        let requests = server.finish();
        assert_eq!(requests.len(), 1);
    }

    #[tokio::test]
    async fn pending_checkpoint_freezes_requests_until_approved() {
        let server = MockServer::start(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
        );
        let (workspace, tool) = tool_for(server.port, "");
        // Simulate an agent-declared checkpoint without approval.
        let run_dir = run_dir(workspace.path(), "test-run");
        std::fs::write(run_dir.join("checkpoint.pending"), "{}").unwrap();
        let output = tool
            .call(&mut ToolContext::default(), get_args(server.url()))
            .await
            .unwrap();
        assert!(output.denied);
        assert!(output.error.unwrap().contains("checkpoint"));

        crate::bounty::checkpoint::approve_checkpoint(&run_dir).unwrap();
        let output = tool
            .call(&mut ToolContext::default(), get_args(server.url()))
            .await
            .unwrap();
        assert!(output.success);
        server.finish();
    }

    #[tokio::test]
    async fn does_not_follow_redirects_unless_enabled() {
        let server = RedirectServer::start(
            vec![(
                "/",
                "HTTP/1.1 302 Found\r\nLocation: /app\r\nContent-Length: 0\r\n\r\n",
            )],
            1,
        );
        let (_workspace, tool) = tool_for(server.port, "");
        let output = tool
            .call(&mut ToolContext::default(), get_args(server.url()))
            .await
            .unwrap();

        assert_eq!(output.status, Some(302));
        assert!(output.redirects.is_empty());
        let requests = server.finish();
        assert_eq!(requests.len(), 1);
    }
}
