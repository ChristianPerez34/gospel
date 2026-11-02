//! Live MCP connections: spawn/connect, tool discovery, and per-turn dynamic
//! tool registration with approval gating.
//!
//! Connections are app-level: one child process or HTTP session per enabled,
//! approved custom server, reused across turns and sessions. `session_tools`
//! converts a server's cached tool list into rig `DynamicTool`s at turn start;
//! non-`readOnlyHint` tools route through the approval broker before running.

use std::collections::HashMap;
use std::sync::Arc;

use rig::message::ToolName;
use rig::tool::rmcp::McpTool;
use rig::tool::{DynamicTool, RegisteredTool, ToolExecutionError};
use rmcp::service::{Peer, RoleClient, RunningService, ServiceExt};
use rmcp::transport::auth::{AuthClient, AuthorizationManager, OAuthState};
use rmcp::transport::child_process::{ConfigureCommandExt, TokioChildProcess};
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
};
use tokio::sync::Mutex;

use crate::approval_broker::{ApprovalBroker, ApprovalRequest};
use crate::mcp::{
    self, McpRefreshUpdate, McpServer, McpToolInventoryItem, MCP_HEALTH_AUTH_REQUIRED,
    MCP_HEALTH_CONNECTED, MCP_HEALTH_NOT_CONNECTED, MCP_READINESS_AWAITING_FIRST_CONNECTION,
    MCP_READINESS_READY, MCP_TRANSPORT_HTTP, MCP_TRANSPORT_STDIO,
};
use crate::mcp_credentials::{FileCredentialStore, McpServerSecrets};

type ClientService = RunningService<RoleClient, ()>;

const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

struct ActiveConnection {
    service: ClientService,
    tools: Vec<rmcp::model::Tool>,
    slug: String,
}

/// App-level registry of live MCP connections, keyed by server id.
pub struct McpRuntime {
    connections: Mutex<HashMap<String, ActiveConnection>>,
}

impl Default for McpRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl McpRuntime {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
        }
    }

    /// Connect (or reconnect) a custom server and report diagnostics.
    /// Never contacts an unapproved or disabled server — those return a
    /// persisted-state update without any side effects.
    pub async fn connect_and_report(&self, server: &McpServer) -> McpRefreshUpdate {
        self.disconnect(&server.id).await;

        if !server.trusted {
            return mcp::refresh_untrusted(server);
        }
        if !server.enabled {
            return McpRefreshUpdate {
                readiness: MCP_READINESS_AWAITING_FIRST_CONNECTION.to_string(),
                health: MCP_HEALTH_NOT_CONNECTED.to_string(),
                last_error_summary: None,
                last_success_at: None,
                last_resolved_executable_path: server.last_resolved_executable_path.clone(),
                inventory: Vec::new(),
            };
        }

        let secrets = crate::mcp_credentials::load_secrets(&server.id).unwrap_or_default();
        match tokio::time::timeout(CONNECT_TIMEOUT, self.connect_inner(server, &secrets)).await {
            Ok(Ok((service, tools))) => {
                let inventory = tools
                    .iter()
                    .map(|tool| McpToolInventoryItem {
                        name: tool.name.to_string(),
                        description: tool.description.as_deref().map(str::to_string),
                        read_only: tool
                            .annotations
                            .as_ref()
                            .and_then(|a| a.read_only_hint)
                            .unwrap_or(false),
                    })
                    .collect::<Vec<_>>();
                let slug = tool_slug(server);
                self.connections.lock().await.insert(
                    server.id.clone(),
                    ActiveConnection {
                        service,
                        tools,
                        slug,
                    },
                );
                McpRefreshUpdate {
                    readiness: MCP_READINESS_READY.to_string(),
                    health: MCP_HEALTH_CONNECTED.to_string(),
                    last_error_summary: None,
                    last_success_at: Some("CURRENT_TIMESTAMP".to_string()),
                    last_resolved_executable_path: resolved_path_for(server),
                    inventory,
                }
            }
            Ok(Err(error)) => failure_update(server, &error),
            Err(_elapsed) => failure_update(server, "Connection timed out"),
        }
    }

    async fn connect_inner(
        &self,
        server: &McpServer,
        secrets: &McpServerSecrets,
    ) -> Result<(ClientService, Vec<rmcp::model::Tool>), String> {
        if server.transport == MCP_TRANSPORT_HTTP {
            self.connect_http(server, secrets).await
        } else {
            self.connect_stdio(server, secrets).await
        }
    }

    async fn connect_stdio(
        &self,
        server: &McpServer,
        secrets: &McpServerSecrets,
    ) -> Result<(ClientService, Vec<rmcp::model::Tool>), String> {
        let program = server.command.as_deref().unwrap_or_default().trim();
        if program.is_empty() {
            return Err("command is required".to_string());
        }
        if mcp::resolve_executable(program).is_none() {
            return Err(format!("Executable was not found: {program}"));
        }
        let env_pairs: Vec<(String, String)> = server
            .env
            .iter()
            .map(|pair| (pair.key.clone(), pair.value.clone()))
            .chain(secrets.env.clone())
            .collect();
        let args = server.args.clone();
        let transport = TokioChildProcess::new(tokio::process::Command::new(program).configure(
            move |command| {
                command.args(&args);
                for (key, value) in &env_pairs {
                    command.env(key, value);
                }
            },
        ))
        .map_err(|e| format!("Failed to spawn MCP server: {e}"))?;

        let service = ().serve(transport).await.map_err(|e| format!("MCP handshake failed: {e}"))?;
        let tools = service
            .peer()
            .list_all_tools()
            .await
            .map_err(|e| format!("Failed to list MCP tools: {e}"))?;
        Ok((service, tools))
    }

    async fn connect_http(
        &self,
        server: &McpServer,
        secrets: &McpServerSecrets,
    ) -> Result<(ClientService, Vec<rmcp::model::Tool>), String> {
        let url = server
            .url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "url is required for HTTP MCP servers".to_string())?
            .to_string();

        let mut custom_headers: HashMap<http::HeaderName, http::HeaderValue> = HashMap::new();
        for pair in server
            .headers
            .iter()
            .map(|pair| (pair.key.clone(), pair.value.clone()))
            .chain(secrets.headers.clone())
        {
            let name = http::HeaderName::from_bytes(pair.0.as_bytes())
                .map_err(|_| format!("invalid header name: {}", pair.0))?;
            let value = http::HeaderValue::from_str(&pair.1)
                .map_err(|_| format!("invalid value for header {}", pair.0))?;
            custom_headers.insert(name, value);
        }
        let config = StreamableHttpClientTransportConfig::with_uri(url.as_str())
            .custom_headers(custom_headers);

        if secrets.oauth.is_some() {
            let manager = oauth_manager(&server.id, &url).await?;
            let service = {
                let client = AuthClient::new(reqwest_rmcp::Client::new(), manager);
                let transport = StreamableHttpClientTransport::with_client(client, config);
                ().serve(transport)
                    .await
                    .map_err(|e| format!("MCP handshake failed: {e}"))?
            };
            match service.peer().list_all_tools().await {
                Ok(tools) => return Ok((service, tools)),
                Err(e) => return Err(format!("Failed to list MCP tools: {e}")),
            }
        }

        let transport =
            StreamableHttpClientTransport::with_client(reqwest_rmcp::Client::new(), config);
        let service = ().serve(transport).await.map_err(|e| {
            if looks_like_auth_required(&e.to_string()) {
                AUTH_REQUIRED_ERROR.to_string()
            } else {
                format!("MCP handshake failed: {e}")
            }
        })?;
        let tools = service
            .peer()
            .list_all_tools()
            .await
            .map_err(|e| format!("Failed to list MCP tools: {e}"))?;
        Ok((service, tools))
    }

    /// Drop and cancel a connection. Also tears down the child process for
    /// stdio transports.
    pub async fn disconnect(&self, server_id: &str) {
        if let Some(connection) = self.connections.lock().await.remove(server_id) {
            let _ = connection.service.cancel().await;
        }
    }

    /// Rig tools for a turn: every connected server's cached tool list as
    /// namespaced dynamic tools. Tools that did not declare `readOnlyHint`
    /// wait on the approval broker before each call.
    pub async fn session_tools(
        &self,
        broker: Arc<ApprovalBroker>,
        run_id: Option<String>,
    ) -> Vec<RegisteredTool> {
        let connections = self.connections.lock().await;
        let mut tools = Vec::new();
        for (server_id, connection) in connections.iter() {
            for tool in &connection.tools {
                if let Some(dynamic) = build_session_tool(
                    server_id,
                    &connection.slug,
                    tool.clone(),
                    connection.service.peer().clone(),
                    broker.clone(),
                    run_id.clone(),
                ) {
                    tools.push(RegisteredTool::from_dynamic(dynamic));
                }
            }
        }
        tools
    }
}

const AUTH_REQUIRED_ERROR: &str = "__auth_required__";

fn looks_like_auth_required(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("401")
        || message.contains("unauthorized")
        || message.contains("authorization required")
}

fn failure_update(server: &McpServer, error: &str) -> McpRefreshUpdate {
    // Keep the prior success timestamp + inventory: a failed reconnect
    // shouldn't erase what the server last looked like when it worked.
    let (health, message) = if error == AUTH_REQUIRED_ERROR {
        (MCP_HEALTH_AUTH_REQUIRED, "Sign-in required.".to_string())
    } else {
        (MCP_HEALTH_NOT_CONNECTED, error.to_string())
    };
    McpRefreshUpdate {
        readiness: MCP_READINESS_AWAITING_FIRST_CONNECTION.to_string(),
        health: health.to_string(),
        last_error_summary: Some(message),
        last_success_at: server.last_success_at.clone(),
        last_resolved_executable_path: resolved_path_for(server),
        inventory: server.inventory.clone(),
    }
}

fn resolved_path_for(server: &McpServer) -> Option<String> {
    if server.transport != MCP_TRANSPORT_STDIO {
        return None;
    }
    server
        .command
        .as_deref()
        .and_then(mcp::resolve_executable)
        .map(|path| path.display().to_string())
}

/// `mcp_<slug>` — tool namespacing prefix derived from the display name so
/// `search` on two different servers stays distinct and readable.
fn tool_slug(server: &McpServer) -> String {
    let slug: String = server
        .display_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        "server".to_string()
    } else {
        slug.chars().take(24).collect()
    }
}

/// `mcp_<slug>__<tool>` capped at 64 characters (provider tool-name limits).
fn registered_tool_name(slug: &str, tool_name: &str) -> String {
    let prefix = format!("mcp_{slug}__");
    let budget = 64usize.saturating_sub(prefix.len());
    let tool_part: String = tool_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .take(budget)
        .collect();
    format!("{prefix}{tool_part}")
}

fn build_session_tool(
    server_id: &str,
    slug: &str,
    tool: rmcp::model::Tool,
    sink: Peer<RoleClient>,
    broker: Arc<ApprovalBroker>,
    run_id: Option<String>,
) -> Option<DynamicTool> {
    let registered = registered_tool_name(slug, &tool.name);
    let name = ToolName::new(registered.clone()).ok()?;
    let description = tool.description.as_deref().unwrap_or("").to_string();
    let parameters = tool.schema_as_json_value();
    let read_only = tool
        .annotations
        .as_ref()
        .and_then(|a| a.read_only_hint)
        .unwrap_or(false);
    let rule = format!("mcp://{server_id}/{}", tool.name);
    let reason = format!("The MCP server wants to run {}.", tool.name);
    let mcp_tool = Arc::new(McpTool::from_mcp_server(tool, sink.clone()));
    let liveness = sink.clone();

    let dynamic = DynamicTool::new(
        name,
        description,
        parameters,
        move |args: serde_json::Value| {
            let mcp_tool = mcp_tool.clone();
            let broker = broker.clone();
            let rule = rule.clone();
            let reason = reason.clone();
            let run_id = run_id.clone();
            let registered = registered.clone();
            Box::pin(async move {
                if !read_only {
                    let allowed = broker
                        .request(ApprovalRequest::mcp_tool(
                            registered,
                            rule,
                            reason,
                            run_id.as_deref(),
                        ))
                        .await;
                    if !allowed {
                        return Err(ToolExecutionError::other(
                            "The MCP tool call was denied.".to_string(),
                        ));
                    }
                }
                let result = mcp_tool.execute_mcp(args.to_string(), None).await?;
                let is_error = result.is_error == Some(true);
                let output = rig::tool::rmcp::mcp_result_output(&result)?;
                if is_error {
                    Err(ToolExecutionError::other(format!(
                        "MCP tool '{}' reported an execution error",
                        mcp_tool.definition().name
                    ))
                    .with_model_output(output))
                } else {
                    Ok(output)
                }
            })
        },
    )
    .with_liveness(move || !liveness.is_transport_closed());
    Some(dynamic)
}

/// Build an AuthorizationManager with the server's stored credentials loaded,
/// ready to attach to an `AuthClient`. Errors when no usable credentials are
/// stored.
async fn oauth_manager(server_id: &str, url: &str) -> Result<AuthorizationManager, String> {
    let mut manager = AuthorizationManager::new(url)
        .await
        .map_err(|e| format!("OAuth setup failed: {e}"))?;
    manager.set_credential_store(FileCredentialStore::new(server_id));
    let restored = manager
        .initialize_from_store()
        .await
        .map_err(|e| format!("Failed to restore OAuth credentials: {e}"))?;
    if !restored {
        return Err("No stored OAuth credentials".to_string());
    }
    Ok(manager)
}

const OAUTH_CALLBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// An OAuth flow in progress: owns the loopback listener and the rmcp state
/// machine until the browser callback lands.
pub struct McpOAuthFlow {
    state: OAuthState,
    pub auth_url: String,
    listener: tokio::net::TcpListener,
}

/// Start OAuth for an HTTP MCP server: discovers the authorization server,
/// registers Gospel as a client dynamically, and binds a loopback redirect
/// listener. `auth_url` is what to open in the browser.
pub async fn start_mcp_oauth(server: &McpServer) -> Result<McpOAuthFlow, String> {
    let url = server
        .url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "This server does not use HTTP transport.".to_string())?
        .to_string();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to start OAuth callback listener: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let mut state = OAuthState::new(url, None)
        .await
        .map_err(|e| format!("OAuth discovery failed: {e}"))?;
    state
        .start_authorization(&[], &redirect_uri, Some("Gospel"))
        .await
        .map_err(|e| format!("OAuth setup failed: {e}"))?;
    let auth_url = state
        .get_authorization_url()
        .await
        .map_err(|e| format!("OAuth setup failed: {e}"))?;
    Ok(McpOAuthFlow {
        state,
        auth_url,
        listener,
    })
}

impl McpOAuthFlow {
    /// Wait for the browser redirect, exchange the code, persist credentials,
    /// and leave the flow ready for a normal `connect_and_report`.
    pub async fn wait_for_callback(mut self, server_id: &str) -> Result<(), String> {
        let callback_url = tokio::time::timeout(
            OAUTH_CALLBACK_TIMEOUT,
            accept_oauth_callback(&self.listener),
        )
        .await
        .map_err(|_| "Sign-in timed out.".to_string())??;
        self.state
            .handle_callback_url(&callback_url)
            .await
            .map_err(|e| format!("OAuth callback was rejected: {e}"))?;
        self.state
            .complete_authorization()
            .await
            .map_err(|e| format!("OAuth completion failed: {e}"))?;
        let (client_id, token_response) = self
            .state
            .get_credentials()
            .await
            .map_err(|e| format!("OAuth credential capture failed: {e}"))?;
        crate::mcp_credentials::store_oauth_credentials(
            server_id,
            rmcp::transport::auth::StoredCredentials::new(
                client_id,
                token_response,
                Vec::new(),
                Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                ),
            ),
        )
        .map_err(|e| format!("Failed to store OAuth credentials: {e}"))?;
        Ok(())
    }
}

/// Accept the first loopback request and hand back the full callback URL.
/// Responds with a plain "close this tab" page.
async fn accept_oauth_callback(listener: &tokio::net::TcpListener) -> Result<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (mut stream, _peer) = listener
        .accept()
        .await
        .map_err(|e| format!("OAuth callback listener failed: {e}"))?;
    let mut buffer = vec![0u8; 8192];
    let n = stream
        .read(&mut buffer)
        .await
        .map_err(|e| format!("OAuth callback read failed: {e}"))?;
    let request = String::from_utf8_lossy(&buffer[..n]);
    let request_line = request.lines().next().unwrap_or_default();
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "OAuth callback was malformed.".to_string())?
        .to_string();

    let response = "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: 146\r\nconnection: close\r\n\r\n<!doctype html><title>Gospel sign-in complete</title><body style=\"font-family:system-ui;padding:2rem\">Sign-in complete. You can close this tab and return to Gospel.</body>";
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;

    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    Ok(format!("http://127.0.0.1:{port}{path}"))
}
