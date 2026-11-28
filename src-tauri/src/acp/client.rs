//! ACP client connections built on the official `agent-client-protocol` SDK.
//!
//! Gospel spawns one short-lived agent process per operation (turn, auth probe,
//! model fetch, fork) and drives it through the SDK's typed v1 client. The SDK
//! owns the NDJSON JSON-RPC transport, framing, and process lifecycle; this
//! module only adapts Gospel's session handlers onto its typed callbacks.

use std::future::Future;
use std::path::Path;
use std::sync::Arc;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v1::schema::{
    ClientCapabilities, FileSystemCapabilities, Implementation, InitializeRequest,
    ReadTextFileRequest, ReadTextFileResponse, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SessionNotification, WriteTextFileRequest,
    WriteTextFileResponse,
};
use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, Client, Error, LineDirection};
use async_trait::async_trait;

pub type StderrLineCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// Gospel-side handler for agent-initiated traffic, typed on SDK wire types.
///
/// Permission and filesystem callbacks run off the connection event loop (the
/// SDK dispatches them through `spawn`), so a long user approval never stalls
/// streaming of subsequent session updates.
#[async_trait]
pub trait AcpSessionHandler: Send + Sync {
    async fn on_session_update(&self, _notification: &SessionNotification) {}

    async fn request_permission(
        &self,
        _request: &RequestPermissionRequest,
    ) -> RequestPermissionOutcome {
        RequestPermissionOutcome::Cancelled
    }

    async fn read_text_file(&self, _request: &ReadTextFileRequest) -> Result<String, String> {
        Err("fs/read_text_file is not enabled".to_string())
    }

    async fn write_text_file(&self, _request: &WriteTextFileRequest) -> Result<(), String> {
        Err("fs/write_text_file is not enabled".to_string())
    }
}

pub struct NoOpAcpHandler;

#[async_trait]
impl AcpSessionHandler for NoOpAcpHandler {}

/// Builds the launch configuration for one agent process. Shared by every
/// operation so the spawn shape lives in exactly one place.
pub fn agent_config(binary: &Path, args: &[String], envs: &[(String, String)]) -> AcpAgentConfig {
    let mut config = AcpAgentConfig::new(binary);
    config = config.args(args.iter().cloned());
    for (key, value) in envs {
        config = config.env(key.clone(), value.clone());
    }
    config
}

/// Client capabilities Gospel advertises. Filesystem access is only offered
/// when a workspace is open; writes additionally require the workspace's
/// source-edit gate so replay/internal consumers stay read-only. Terminal
/// methods are not implemented (see docs/research/factory-droid-acp.md).
pub fn client_capabilities(workspace_open: bool, source_edit_allowed: bool) -> ClientCapabilities {
    let mut capabilities = ClientCapabilities::new();
    capabilities.fs = FileSystemCapabilities::new()
        .read_text_file(workspace_open)
        .write_text_file(workspace_open && source_edit_allowed);
    capabilities.terminal = false;
    capabilities
}

pub fn initialize_request(workspace_open: bool, source_edit_allowed: bool) -> InitializeRequest {
    let mut request = InitializeRequest::new(ProtocolVersion::V1);
    request.client_capabilities = client_capabilities(workspace_open, source_edit_allowed);
    request.client_info = Some(Implementation::new("gospel", env!("CARGO_PKG_VERSION")));
    request
}

/// Connects to one agent process and drives `f` with the typed connection.
///
/// The returned future is `Send` and may be spawned so callers can abort it
/// for hard cancellation; dropping it tears down the agent process group.
/// `f` runs as the connection's foreground closure: when it returns, the
/// connection shuts down and the SDK reaps the child.
pub async fn connect<F, Fut, T>(
    config: AcpAgentConfig,
    handler: Arc<dyn AcpSessionHandler>,
    on_stderr_line: Option<StderrLineCallback>,
    f: F,
) -> Result<T, Error>
where
    F: FnOnce(ConnectionTo<Agent>) -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    let mut agent = AcpAgent::new(config);
    if let Some(callback) = on_stderr_line {
        agent = agent.with_debug(move |line, direction| {
            if direction == LineDirection::Stderr {
                callback(line);
            }
        });
    }

    let updates_handler = Arc::clone(&handler);
    let permission_handler = Arc::clone(&handler);
    let read_handler = Arc::clone(&handler);
    let write_handler = Arc::clone(&handler);

    Client
        .v1()
        .name("gospel")
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                updates_handler.on_session_update(&notification).await;
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, cx| {
                let handler = Arc::clone(&permission_handler);
                // Offload the approval wait so the event loop keeps streaming
                // session updates while the user decides.
                let spawned = cx.spawn(async move {
                    let outcome = handler.request_permission(&request).await;
                    responder.respond(RequestPermissionResponse::new(outcome))
                });
                match spawned {
                    Ok(()) => Ok(()),
                    Err(err) => Err(err),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReadTextFileRequest, responder, cx| {
                let handler = Arc::clone(&read_handler);
                let spawned = cx.spawn(async move {
                    match handler.read_text_file(&request).await {
                        Ok(content) => responder.respond(ReadTextFileResponse::new(content)),
                        Err(message) => responder.respond_with_error(Error::new(
                            agent_client_protocol::ErrorCode::INTERNAL_ERROR,
                            message,
                        )),
                    }
                });
                match spawned {
                    Ok(()) => Ok(()),
                    Err(err) => Err(err),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WriteTextFileRequest, responder, cx| {
                let handler = Arc::clone(&write_handler);
                let spawned = cx.spawn(async move {
                    match handler.write_text_file(&request).await {
                        Ok(()) => responder.respond(WriteTextFileResponse::new()),
                        Err(message) => responder.respond_with_error(Error::new(
                            agent_client_protocol::ErrorCode::INTERNAL_ERROR,
                            message,
                        )),
                    }
                });
                match spawned {
                    Ok(()) => Ok(()),
                    Err(err) => Err(err),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |connection| f(connection))
        .await
}

/// Wraps a request future with a timeout, mapping expiry to an internal error
/// tagged with the operation stage.
pub async fn with_timeout<T>(
    stage: &str,
    timeout: std::time::Duration,
    future: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    match tokio::time::timeout(timeout, future).await {
        Ok(result) => result,
        Err(_) => Err(Error::new(
            agent_client_protocol::ErrorCode::INTERNAL_ERROR,
            format!("{stage} timed out"),
        )),
    }
}
