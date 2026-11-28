//! Bidirectional NDJSON JSON-RPC 2.0 client transport for ACP agent subprocesses.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, Mutex};

use super::protocol::{
    JsonRpcError, PermissionOutcome, RawJsonRpcMessage, ReadTextFileParams, ReadTextFileResult,
    RequestPermissionParams, RequestPermissionResult, SessionUpdateNotification,
    WriteTextFileParams,
};

const MAX_STDERR_LINES: usize = 40;
const MAX_STDERR_LINE_BYTES: usize = 512;

pub type StderrLineCallback = Arc<dyn Fn(&str) + Send + Sync>;

#[allow(clippy::double_must_use)]
#[async_trait]
pub trait AcpClientHandler: Send + Sync {
    async fn on_session_update(&self, notification: SessionUpdateNotification);

    async fn on_notification(&self, _method: &str, _params: &Value) {}

    async fn request_permission(
        &self,
        _params: RequestPermissionParams,
    ) -> Result<RequestPermissionResult, String> {
        Ok(RequestPermissionResult {
            outcome: PermissionOutcome::Cancelled,
        })
    }

    async fn read_text_file(&self, _params: ReadTextFileParams) -> Result<ReadTextFileResult, String> {
        Err("fs/read_text_file is not enabled".to_string())
    }

    async fn write_text_file(&self, _params: WriteTextFileParams) -> Result<(), String> {
        Err("fs/write_text_file is not enabled".to_string())
    }
}

pub struct NoOpAcpHandler;

#[async_trait]
impl AcpClientHandler for NoOpAcpHandler {
    async fn on_session_update(&self, _notification: SessionUpdateNotification) {}
}

#[derive(Debug)]
pub enum AcpTransportError {
    Spawn(String),
    Io(String),
    Timeout(String),
    Rpc(JsonRpcError),
    Decode(String),
    Closed(String),
}

impl std::fmt::Display for AcpTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(msg) => write!(f, "failed to launch ACP agent: {msg}"),
            Self::Io(msg) => write!(f, "ACP IO error: {msg}"),
            Self::Timeout(msg) => write!(f, "ACP request timed out: {msg}"),
            Self::Rpc(err) => write!(f, "{err}"),
            Self::Decode(msg) => write!(f, "ACP decode error: {msg}"),
            Self::Closed(msg) => write!(f, "ACP connection closed: {msg}"),
        }
    }
}

pub struct AcpClient {
    writer: Mutex<Box<dyn AsyncWrite + Unpin + Send>>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, JsonRpcError>>>>,
    next_id: AtomicU64,
    stderr_lines: Arc<Mutex<VecDeque<String>>>,
    child: Mutex<Option<Child>>,
    reader_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    stderr_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl AcpClient {
    pub fn spawn(
        program: &Path,
        args: &[&str],
        cwd: Option<&Path>,
        envs: &[(&str, &str)],
        handler: Arc<dyn AcpClientHandler>,
        on_stderr_line: Option<StderrLineCallback>,
    ) -> Result<Arc<Self>, AcpTransportError> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        for (k, v) in envs {
            command.env(k, v);
        }

        let mut child = command
            .spawn()
            .map_err(|e| AcpTransportError::Spawn(format!("{}: {e}", program.display())))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AcpTransportError::Spawn("missing child stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AcpTransportError::Spawn("missing child stdout".to_string()))?;
        let stderr = child.stderr.take();

        let client = Self::from_streams(stdin, stdout, Some(child), handler);
        if let Some(stderr) = stderr {
            let buffer = Arc::clone(&client.stderr_lines);
            let task = tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Some(cb) = on_stderr_line.as_ref() {
                        cb(trimmed);
                    }
                    let truncated: String = trimmed.chars().take(MAX_STDERR_LINE_BYTES).collect();
                    let mut guard = buffer.lock().await;
                    if guard.len() >= MAX_STDERR_LINES {
                        guard.pop_front();
                    }
                    guard.push_back(truncated);
                }
            });
            if let Ok(mut guard) = client.stderr_task.try_lock() {
                *guard = Some(task);
            }
        }
        Ok(client)
    }

    pub fn from_streams<W, R>(
        writer: W,
        reader: R,
        child: Option<Child>,
        handler: Arc<dyn AcpClientHandler>,
    ) -> Arc<Self>
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncRead + Unpin + Send + 'static,
    {
        let client = Arc::new(Self {
            writer: Mutex::new(Box::new(writer)),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            stderr_lines: Arc::new(Mutex::new(VecDeque::new())),
            child: Mutex::new(child),
            reader_task: Mutex::new(None),
            stderr_task: Mutex::new(None),
        });

        let weak = Arc::downgrade(&client);
        let reader_task = tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let Ok(msg) = serde_json::from_str::<RawJsonRpcMessage>(trimmed) else {
                    tracing::debug!("Ignoring non-JSON line from ACP stdout");
                    continue;
                };
                let Some(strong) = weak.upgrade() else {
                    break;
                };
                strong.dispatch_incoming(msg, Arc::clone(&handler)).await;
            }
            if let Some(strong) = weak.upgrade() {
                strong.fail_all_pending("ACP agent closed stdout").await;
            }
        });
        if let Ok(mut guard) = client.reader_task.try_lock() {
            *guard = Some(reader_task);
        }
        client
    }

    async fn dispatch_incoming(
        self: &Arc<Self>,
        msg: RawJsonRpcMessage,
        handler: Arc<dyn AcpClientHandler>,
    ) {
        if let Some(method) = msg.method {
            let params = msg.params.unwrap_or(Value::Null);
            if let Some(id) = msg.id {
                let client = Arc::clone(self);
                // Dispatch server->client requests concurrently so blocking on user
                // approval never stalls reading subsequent session/update frames.
                tokio::spawn(async move {
                    let response = handle_server_request(&method, params, &*handler).await;
                    let _ = client.send_raw_response(id, response).await;
                });
            } else if method == "session/update" {
                match serde_json::from_value::<SessionUpdateNotification>(params) {
                    Ok(update) => handler.on_session_update(update).await,
                    Err(e) => tracing::warn!("Failed to decode ACP session/update: {e}"),
                }
            } else {
                handler.on_notification(&method, &params).await;
            }
            return;
        }

        if let Some(id_val) = msg.id {
            if let Some(id) = id_val.as_u64() {
                if let Some(tx) = self.pending.lock().await.remove(&id) {
                    let payload = match msg.error {
                        Some(err) => Err(err),
                        None => Ok(msg.result.unwrap_or(Value::Null)),
                    };
                    let _ = tx.send(payload);
                }
            }
        }
    }

    async fn fail_all_pending(&self, reason: &str) {
        let mut pending = self.pending.lock().await;
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err(JsonRpcError {
                code: -32001,
                message: reason.to_string(),
                data: None,
            }));
        }
    }

    async fn write_line(&self, value: &Value) -> Result<(), AcpTransportError> {
        let mut encoded = serde_json::to_string(value)
            .map_err(|e| AcpTransportError::Decode(e.to_string()))?;
        encoded.push('\n');
        let mut writer = self.writer.lock().await;
        writer
            .write_all(encoded.as_bytes())
            .await
            .map_err(|e| AcpTransportError::Io(e.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|e| AcpTransportError::Io(e.to_string()))?;
        Ok(())
    }

    async fn send_raw_response(
        &self,
        id: Value,
        result: Result<Value, (i64, String)>,
    ) -> Result<(), AcpTransportError> {
        let payload = match result {
            Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
            Err((code, message)) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": code, "message": message }
            }),
        };
        self.write_line(&payload).await
    }

    pub async fn notify<P: Serialize>(
        &self,
        method: &str,
        params: &P,
    ) -> Result<(), AcpTransportError> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.write_line(&payload).await
    }

    pub async fn request<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: &P,
        timeout: Option<Duration>,
    ) -> Result<R, AcpTransportError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        if let Err(err) = self.write_line(&payload).await {
            self.pending.lock().await.remove(&id);
            return Err(err);
        }

        let response = match timeout {
            Some(dur) => match tokio::time::timeout(dur, rx).await {
                Ok(res) => res,
                Err(_) => {
                    self.pending.lock().await.remove(&id);
                    return Err(AcpTransportError::Timeout(method.to_string()));
                }
            },
            None => rx.await,
        };

        let raw = match response {
            Ok(Ok(val)) => val,
            Ok(Err(rpc_err)) => return Err(AcpTransportError::Rpc(rpc_err)),
            Err(_) => {
                let stderr = self.stderr_summary().await;
                let msg = if stderr.is_empty() {
                    format!("{method} aborted before response")
                } else {
                    format!("{method} aborted: {stderr}")
                };
                return Err(AcpTransportError::Closed(msg));
            }
        };

        serde_json::from_value(raw)
            .map_err(|e| AcpTransportError::Decode(format!("{method}: {e}")))
    }

    pub async fn stderr_summary(&self) -> String {
        let lines = self.stderr_lines.lock().await;
        lines
            .iter()
            .filter(|line| !line.contains("telemetry.factory.ai"))
            .cloned()
            .collect::<Vec<_>>()
            .join(" | ")
    }

    pub async fn shutdown(&self) {
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        if let Some(task) = self.reader_task.lock().await.take() {
            task.abort();
        }
        if let Some(task) = self.stderr_task.lock().await.take() {
            task.abort();
        }
    }
}

async fn handle_server_request(
    method: &str,
    params: Value,
    handler: &dyn AcpClientHandler,
) -> Result<Value, (i64, String)> {
    match method {
        "session/request_permission" => {
            let req: RequestPermissionParams = serde_json::from_value(params)
                .map_err(|e| (-32602, format!("Invalid permission params: {e}")))?;
            let result = handler
                .request_permission(req)
                .await
                .map_err(|e| (-32000, e))?;
            serde_json::to_value(result).map_err(|e| (-32603, e.to_string()))
        }
        "fs/read_text_file" => {
            let req: ReadTextFileParams = serde_json::from_value(params)
                .map_err(|e| (-32602, format!("Invalid read_text_file params: {e}")))?;
            let result = handler
                .read_text_file(req)
                .await
                .map_err(|e| (-32000, e))?;
            serde_json::to_value(result).map_err(|e| (-32603, e.to_string()))
        }
        "fs/write_text_file" => {
            let req: WriteTextFileParams = serde_json::from_value(params)
                .map_err(|e| (-32602, format!("Invalid write_text_file params: {e}")))?;
            handler
                .write_text_file(req)
                .await
                .map_err(|e| (-32000, e))?;
            Ok(json!({}))
        }
        other => Err((-32601, format!("Method not found: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::protocol::{ClientCapabilities, ClientInfo, InitializeParams, InitializeResult};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::sync::Mutex as TokioMutex;

    struct RecordingHandler {
        updates: TokioMutex<Vec<SessionUpdateNotification>>,
    }

    #[async_trait]
    impl AcpClientHandler for RecordingHandler {
        async fn on_session_update(&self, notification: SessionUpdateNotification) {
            self.updates.lock().await.push(notification);
        }

        async fn request_permission(
            &self,
            params: RequestPermissionParams,
        ) -> Result<RequestPermissionResult, String> {
            let first = params
                .options
                .first()
                .map(|o| o.option_id.clone())
                .unwrap_or_default();
            Ok(RequestPermissionResult {
                outcome: PermissionOutcome::Selected { option_id: first },
            })
        }
    }

    #[tokio::test]
    async fn handles_bidirectional_rpc_notifications_and_permissions() {
        let (client_read, server_write) = tokio::io::duplex(4096);
        let (server_read, client_write) = tokio::io::duplex(4096);

        let handler = Arc::new(RecordingHandler {
            updates: TokioMutex::new(Vec::new()),
        });
        let client = AcpClient::from_streams(
            client_write,
            client_read,
            None,
            Arc::clone(&handler) as Arc<dyn AcpClientHandler>,
        );

        let server = tokio::spawn(async move {
            let mut lines = BufReader::new(server_read).lines();
            let mut writer = server_write;

            let init_line = lines.next_line().await.unwrap().unwrap();
            let init_req: Value = serde_json::from_str(&init_line).unwrap();
            assert_eq!(init_req["method"], "initialize");
            let init_resp = json!({
                "jsonrpc": "2.0",
                "id": init_req["id"],
                "result": { "protocolVersion": 1, "agentInfo": { "name": "mock", "version": "1.0" } }
            });
            writer
                .write_all(format!("{init_resp}\n").as_bytes())
                .await
                .unwrap();

            let prompt_line = lines.next_line().await.unwrap().unwrap();
            let prompt_req: Value = serde_json::from_str(&prompt_line).unwrap();

            // Send update notification + server->client permission request while prompt is in flight.
            let update = json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "sessionId": "s1",
                    "update": { "sessionUpdate": "agent_message_chunk", "content": { "text": "ok" } }
                }
            });
            writer
                .write_all(format!("{update}\n").as_bytes())
                .await
                .unwrap();

            let perm_req = json!({
                "jsonrpc": "2.0",
                "id": "srv-1",
                "method": "session/request_permission",
                "params": {
                    "sessionId": "s1",
                    "toolCall": { "toolCallId": "tc-1", "title": "Run cargo test", "kind": "execute" },
                    "options": [{ "optionId": "allow-1", "name": "Allow once", "kind": "allow_once" }]
                }
            });
            writer
                .write_all(format!("{perm_req}\n").as_bytes())
                .await
                .unwrap();

            let perm_resp_line = lines.next_line().await.unwrap().unwrap();
            let perm_resp: Value = serde_json::from_str(&perm_resp_line).unwrap();
            assert_eq!(perm_resp["id"], "srv-1");
            assert_eq!(perm_resp["result"]["outcome"]["outcome"], "selected");
            assert_eq!(perm_resp["result"]["outcome"]["optionId"], "allow-1");

            let done = json!({
                "jsonrpc": "2.0",
                "id": prompt_req["id"],
                "result": { "stopReason": "end_turn" }
            });
            writer
                .write_all(format!("{done}\n").as_bytes())
                .await
                .unwrap();
        });

        let init: InitializeResult = client
            .request(
                "initialize",
                &InitializeParams {
                    protocol_version: 1,
                    client_capabilities: ClientCapabilities::default(),
                    client_info: ClientInfo {
                        name: "gospel".into(),
                        version: "0.1.0".into(),
                    },
                },
                Some(Duration::from_secs(2)),
            )
            .await
            .unwrap();
        assert_eq!(init.protocol_version, 1);

        let prompt_res: crate::acp::protocol::SessionPromptResult = client
            .request(
                "session/prompt",
                &crate::acp::protocol::SessionPromptParams {
                    session_id: "s1".into(),
                    prompt: vec![crate::acp::protocol::PromptContentBlock::Text {
                        text: "hi".into(),
                    }],
                },
                Some(Duration::from_secs(2)),
            )
            .await
            .unwrap();
        assert_eq!(prompt_res.stop_reason, "end_turn");
        assert_eq!(handler.updates.lock().await.len(), 1);

        server.await.unwrap();
        client.shutdown().await;
    }
}
