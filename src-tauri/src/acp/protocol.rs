//! ACP (Agent Client Protocol) JSON-RPC 2.0 wire types over NDJSON stdio.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ACP_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsCapabilities {
    #[serde(default)]
    pub read_text_file: bool,
    #[serde(default)]
    pub write_text_file: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(default)]
    pub fs: FsCapabilities,
    #[serde(default)]
    pub terminal: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: u32,
    pub client_capabilities: ClientCapabilities,
    pub client_info: ClientInfo,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct AuthMethod {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    #[serde(default)]
    pub load_session: bool,
    #[serde(default)]
    pub session_capabilities: Value,
    #[serde(default)]
    pub prompt_capabilities: Value,
    #[serde(default)]
    pub mcp_capabilities: Value,
    #[serde(default)]
    pub auth: Value,
}

impl AgentCapabilities {
    pub fn supports_fork(&self) -> bool {
        self.session_capabilities.get("fork").is_some()
    }

    pub fn supports_close(&self) -> bool {
        self.session_capabilities.get("close").is_some()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    #[serde(default)]
    pub protocol_version: u32,
    #[serde(default)]
    pub agent_capabilities: AgentCapabilities,
    #[serde(default)]
    pub agent_info: AgentInfo,
    #[serde(default)]
    pub auth_methods: Vec<AuthMethod>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatusResult {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub method_id: Option<String>,
}

impl AuthStatusResult {
    pub fn is_authenticated(&self) -> bool {
        self.state.eq_ignore_ascii_case("authenticated")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateParams {
    pub method_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNewParams {
    pub cwd: String,
    pub mcp_servers: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLoadParams {
    pub session_id: String,
    pub cwd: String,
    pub mcp_servers: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionForkParams {
    pub session_id: String,
    pub cwd: String,
    pub mcp_servers: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCloseParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOptionChoice {
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOptionGroup {
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub options: Vec<ConfigOptionChoice>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfigOption {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default, rename = "type")]
    pub option_type: String,
    #[serde(default)]
    pub current_value: Value,
    #[serde(default)]
    pub options: Vec<ConfigOptionChoice>,
    #[serde(default)]
    pub groups: Vec<ConfigOptionGroup>,
}

impl SessionConfigOption {
    #[allow(dead_code)]
    pub fn all_choices(&self) -> Vec<&ConfigOptionChoice> {
        self.options
            .iter()
            .chain(self.groups.iter().flat_map(|g| g.options.iter()))
            .collect()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct AvailableAcpModel {
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct ModelsMetadata {
    #[serde(default)]
    pub current_model_id: Option<String>,
    #[serde(default)]
    pub available_models: Vec<AvailableAcpModel>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct SessionNewResult {
    pub session_id: String,
    #[serde(default)]
    pub config_options: Vec<SessionConfigOption>,
    #[serde(default)]
    pub models: Option<ModelsMetadata>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct SessionForkResult {
    pub session_id: String,
    #[serde(default)]
    pub config_options: Vec<SessionConfigOption>,
    #[serde(default)]
    pub models: Option<ModelsMetadata>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSetConfigOptionParams {
    pub session_id: String,
    pub config_id: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSetModelParams {
    pub session_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptContentBlock {
    Text { text: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPromptParams {
    pub session_id: String,
    pub prompt: Vec<PromptContentBlock>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPromptResult {
    #[serde(default)]
    pub stop_reason: String,
    #[serde(default)]
    pub usage: Option<PromptUsage>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptUsage {
    #[serde(default)]
    pub input_tokens: Option<usize>,
    #[serde(default)]
    pub output_tokens: Option<usize>,
    #[serde(default)]
    pub prompt_tokens: Option<usize>,
    #[serde(default)]
    pub response_tokens: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCancelParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    #[serde(default)]
    pub option_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct PermissionToolCall {
    #[serde(default)]
    pub tool_call_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub raw_input: Option<Value>,
    #[serde(default)]
    pub locations: Vec<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct RequestPermissionParams {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub tool_call: PermissionToolCall,
    #[serde(default)]
    pub options: Vec<PermissionOption>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum PermissionOutcome {
    Selected {
        #[serde(rename = "optionId")]
        option_id: String,
    },
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPermissionResult {
    pub outcome: PermissionOutcome,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct ReadTextFileParams {
    #[serde(default)]
    pub session_id: String,
    pub path: String,
    #[serde(default)]
    pub line: Option<usize>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadTextFileResult {
    pub content: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct WriteTextFileParams {
    #[serde(default)]
    pub session_id: String,
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdateNotification {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub update: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParsedSessionUpdate {
    AgentMessageChunk {
        text: String,
    },
    AgentThoughtChunk {
        text: String,
    },
    ToolCall {
        tool_call_id: String,
        title: String,
        kind: Option<String>,
        status: Option<String>,
        raw_input: Option<Value>,
    },
    ToolCallUpdate {
        tool_call_id: String,
        title: Option<String>,
        kind: Option<String>,
        status: Option<String>,
        raw_input: Option<Value>,
        raw_output: Option<Value>,
        content_text: Option<String>,
    },
    Other,
}

pub fn extract_text_from_content(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        return s.to_string();
    }
    if let Some(arr) = value.as_array() {
        let mut out = String::new();
        for item in arr {
            let chunk = extract_text_from_content(item);
            if !chunk.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&chunk);
            }
        }
        return out;
    }
    if let Some(obj) = value.as_object() {
        for key in ["text", "thought", "message"] {
            if let Some(s) = obj.get(key).and_then(Value::as_str) {
                return s.to_string();
            }
        }
        if let Some(nested) = obj.get("content") {
            return extract_text_from_content(nested);
        }
    }
    String::new()
}

pub fn parse_session_update(update: &Value) -> ParsedSessionUpdate {
    let kind = update
        .get("sessionUpdate")
        .or_else(|| update.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("");

    match kind {
        "agent_message_chunk" => {
            let text = update
                .get("content")
                .map(extract_text_from_content)
                .unwrap_or_default();
            ParsedSessionUpdate::AgentMessageChunk { text }
        }
        "agent_thought_chunk" => {
            let text = update
                .get("content")
                .map(extract_text_from_content)
                .unwrap_or_default();
            ParsedSessionUpdate::AgentThoughtChunk { text }
        }
        "tool_call" => ParsedSessionUpdate::ToolCall {
            tool_call_id: update
                .get("toolCallId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            title: update
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("tool_call")
                .to_string(),
            kind: update
                .get("kind")
                .and_then(Value::as_str)
                .map(str::to_string),
            status: update
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_string),
            raw_input: update.get("rawInput").cloned().filter(|v| !v.is_null()),
        },
        "tool_call_update" => {
            let content_text = update
                .get("content")
                .map(extract_text_from_content)
                .filter(|s| !s.is_empty());
            ParsedSessionUpdate::ToolCallUpdate {
                tool_call_id: update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                title: update
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                kind: update
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                status: update
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                raw_input: update.get("rawInput").cloned().filter(|v| !v.is_null()),
                raw_output: update.get("rawOutput").cloned().filter(|v| !v.is_null()),
                content_text,
            }
        }
        _ => ParsedSessionUpdate::Other,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct JsonRpcError {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (code {})", self.message, self.code)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawJsonRpcMessage {
    #[serde(default)]
    pub id: Option<Value>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub params: Option<Value>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

/// Auth prompt extracted from a JSON-RPC payload or stderr diagnostic during `authenticate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractedAuthChallenge {
    DeviceCode {
        verification_url: String,
        user_code: String,
    },
    Browser {
        authorize_url: String,
    },
}

pub fn extract_auth_challenge_from_value(value: &Value) -> Option<ExtractedAuthChallenge> {
    let url = find_string_field(
        value,
        &[
            "verificationUriComplete",
            "verification_uri_complete",
            "verificationUri",
            "verification_uri",
            "authorizeUrl",
            "authorize_url",
            "verificationUrl",
            "verification_url",
            "authUrl",
            "auth_url",
            "url",
        ],
    )?;
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return None;
    }
    if let Some(code) = find_string_field(value, &["userCode", "user_code", "pairingCode", "code"])
    {
        if !code.is_empty() && code.len() <= 32 {
            return Some(ExtractedAuthChallenge::DeviceCode {
                verification_url: url,
                user_code: code,
            });
        }
    }
    Some(ExtractedAuthChallenge::Browser { authorize_url: url })
}

pub fn extract_auth_challenge_from_text(line: &str) -> Option<ExtractedAuthChallenge> {
    let url = line
        .split_whitespace()
        .map(|tok| tok.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | '.' | ')' | '(' | '<' | '>')))
        .find(|tok| tok.starts_with("https://") || tok.starts_with("http://"))?
        .to_string();
    let user_code = line
        .split_whitespace()
        .map(|tok| tok.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | '.' | ')' | '(' | ':')))
        .find(|tok| is_device_code_token(tok))
        .map(str::to_string);
    match user_code {
        Some(code) => Some(ExtractedAuthChallenge::DeviceCode {
            verification_url: url,
            user_code: code,
        }),
        None => Some(ExtractedAuthChallenge::Browser { authorize_url: url }),
    }
}

fn is_device_code_token(token: &str) -> bool {
    let parts: Vec<&str> = token.split('-').collect();
    parts.len() == 2
        && parts.iter().all(|p| {
            (3..=6).contains(&p.len())
                && p.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
}

fn find_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(s) = map.get(*key).and_then(Value::as_str) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            for child in map.values() {
                if let Some(found) = find_string_field(child, keys) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_string_field(item, keys)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_initialize_and_auth_status() {
        let init_json = json!({
            "protocolVersion": 1,
            "agentCapabilities": {
                "loadSession": true,
                "sessionCapabilities": { "fork": {}, "close": {} }
            },
            "agentInfo": { "name": "@factory/cli", "title": "Factory Droid", "version": "1.21.0" },
            "authMethods": [
                { "id": "device-pairing", "name": "Login", "description": "Browser login" },
                { "id": "factory-api-key", "name": "Factory API Key", "description": "Env key" }
            ]
        });
        let init: InitializeResult = serde_json::from_value(init_json).unwrap();
        assert_eq!(init.protocol_version, 1);
        assert!(init.agent_capabilities.load_session);
        assert!(init.agent_capabilities.supports_fork());
        assert!(init.agent_capabilities.supports_close());
        assert_eq!(init.auth_methods.len(), 2);

        let status: AuthStatusResult =
            serde_json::from_value(json!({ "state": "authenticated" })).unwrap();
        assert!(status.is_authenticated());
    }

    #[test]
    fn parses_session_update_variants() {
        assert_eq!(
            parse_session_update(&json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "hello " }
            })),
            ParsedSessionUpdate::AgentMessageChunk {
                text: "hello ".into()
            }
        );

        assert_eq!(
            parse_session_update(&json!({
                "sessionUpdate": "agent_thought_chunk",
                "content": { "thought": "checking files" }
            })),
            ParsedSessionUpdate::AgentThoughtChunk {
                text: "checking files".into()
            }
        );

        assert_eq!(
            parse_session_update(&json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "call-1",
                "title": "Read file",
                "kind": "read",
                "status": "in_progress",
                "rawInput": { "path": "Cargo.toml" }
            })),
            ParsedSessionUpdate::ToolCall {
                tool_call_id: "call-1".into(),
                title: "Read file".into(),
                kind: Some("read".into()),
                status: Some("in_progress".into()),
                raw_input: Some(json!({ "path": "Cargo.toml" })),
            }
        );

        assert_eq!(
            parse_session_update(&json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call-1",
                "status": "completed",
                "rawOutput": { "ok": true },
                "content": [{ "type": "text", "text": "done" }]
            })),
            ParsedSessionUpdate::ToolCallUpdate {
                tool_call_id: "call-1".into(),
                title: None,
                kind: None,
                status: Some("completed".into()),
                raw_input: None,
                raw_output: Some(json!({ "ok": true })),
                content_text: Some("done".into()),
            }
        );
    }

    #[test]
    fn extracts_auth_challenges_from_json_and_text() {
        assert_eq!(
            extract_auth_challenge_from_value(&json!({
                "details": {
                    "verificationUri": "https://app.factory.ai/device",
                    "userCode": "ABCD-1234"
                }
            })),
            Some(ExtractedAuthChallenge::DeviceCode {
                verification_url: "https://app.factory.ai/device".into(),
                user_code: "ABCD-1234".into(),
            })
        );
        assert_eq!(
            extract_auth_challenge_from_text(
                "Visit https://app.factory.ai/device and enter code WXYZ-5678 to pair"
            ),
            Some(ExtractedAuthChallenge::DeviceCode {
                verification_url: "https://app.factory.ai/device".into(),
                user_code: "WXYZ-5678".into(),
            })
        );
    }
}
