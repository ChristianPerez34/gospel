use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const BUILT_IN_CODEBASE_KNOWLEDGE_ID: &str = "codebase_knowledge";
pub const MCP_KIND_BUILT_IN: &str = "built_in";
pub const MCP_KIND_CUSTOM: &str = "custom";
pub const MCP_TRANSPORT_STDIO: &str = "stdio";
pub const MCP_TRANSPORT_HTTP: &str = "http";
pub const MCP_SAFETY_READ_ONLY: &str = "read_only";
pub const MCP_SAFETY_MUTATING: &str = "mutating";
pub const MCP_SAFETY_UNKNOWN: &str = "unknown";
pub const MCP_READINESS_AWAITING_FIRST_CONNECTION: &str = "awaiting_first_connection";
pub const MCP_READINESS_READY: &str = "ready";
pub const MCP_READINESS_UNAVAILABLE: &str = "unavailable";
pub const MCP_HEALTH_CONNECTED: &str = "connected";
pub const MCP_HEALTH_NOT_CONNECTED: &str = "not_connected";
pub const MCP_HEALTH_UNTRUSTED: &str = "untrusted";
pub const MCP_HEALTH_NOT_FOUND: &str = "not_found";
pub const MCP_HEALTH_WORKSPACE_REQUIRED: &str = "workspace_required";
pub const MCP_HEALTH_AUTH_REQUIRED: &str = "auth_required";
pub const MCP_AUTH_NONE: &str = "none";
pub const MCP_AUTH_REQUIRED: &str = "required";
pub const MCP_AUTH_AUTHORIZED: &str = "authorized";

#[derive(Debug, Clone)]
pub struct BuiltInMcpServerDefinition {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub safety_class: &'static str,
    pub scope: &'static str,
    pub tool_names: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpEnvValue {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInventoryItem {
    pub name: String,
    pub description: Option<String>,
    /// True when the server declares `readOnlyHint` for this tool — the only
    /// tools that run without an approval prompt.
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub trusted: bool,
    pub trust_revoked_reason: Option<String>,
    pub safety_class: String,
    pub scope: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<McpEnvValue>,
    pub url: Option<String>,
    pub headers: Vec<McpEnvValue>,
    /// Secret key names only — values live in the credential store, never in
    /// the SQLite config or this payload.
    pub secret_env_keys: Vec<String>,
    pub secret_header_keys: Vec<String>,
    /// Declared secrets the user still has to supply a value for.
    pub missing_secret_env_keys: Vec<String>,
    pub missing_secret_header_keys: Vec<String>,
    /// "none" | "required" | "authorized" — derived per request, not stored.
    pub auth_status: String,
    pub readiness: String,
    pub health: String,
    pub inventory: Vec<McpToolInventoryItem>,
    pub last_error_summary: Option<String>,
    pub last_success_at: Option<String>,
    pub last_resolved_executable_path: Option<String>,
    pub external_fingerprint: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMcpServerRequest {
    pub display_name: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<McpEnvValue>,
    /// Declared secret env names (legacy shape; values are never expected).
    #[serde(default)]
    pub secret_env_keys: Vec<String>,
    /// Secret env key/value pairs. Values are stored in the credential file
    /// and never reach SQLite; an empty value declares the key without a value.
    #[serde(default)]
    pub secret_env: Vec<McpEnvValue>,
    /// "stdio" (default) or "http".
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: Vec<McpEnvValue>,
    /// Secret header key/value pairs, same storage contract as `secret_env`.
    #[serde(default)]
    pub secret_headers: Vec<McpEnvValue>,
    #[serde(default)]
    pub safety_class: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMcpServerRequest {
    pub display_name: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<McpEnvValue>,
    #[serde(default)]
    pub secret_env_keys: Vec<String>,
    #[serde(default)]
    pub secret_env: Vec<McpEnvValue>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: Vec<McpEnvValue>,
    #[serde(default)]
    pub secret_headers: Vec<McpEnvValue>,
    #[serde(default)]
    pub safety_class: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpImportPreviewRequest {
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpApplyImportRequest {
    pub token: String,
    pub selected_external_ids: Vec<String>,
    #[serde(default)]
    pub overwrite_existing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpImportPreview {
    pub token: String,
    pub source_path: String,
    pub servers: Vec<McpImportPreviewServer>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpImportPreviewServer {
    pub external_id: String,
    pub name: String,
    pub proposed: CreateMcpServerRequest,
    pub matched_server_id: Option<String>,
    pub conflict: bool,
    pub field_diffs: Vec<McpImportFieldDiff>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpImportFieldDiff {
    pub field: String,
    pub current: Option<String>,
    pub incoming: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpApplyImportResult {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CustomMcpServerRecord {
    pub id: String,
    pub display_name: String,
    pub transport: String,
    pub command: String,
    pub args_json: String,
    pub env_json: String,
    pub url: String,
    pub headers_json: String,
    pub secret_env_keys_json: String,
    pub secret_header_keys_json: String,
    pub safety_class: String,
    pub scope: String,
    pub external_fingerprint: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct McpServerStateRecord {
    pub enabled: bool,
    pub trusted: bool,
    pub trust_revoked_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct McpDiagnosticsRecord {
    pub readiness: String,
    pub health: String,
    pub last_error_summary: Option<String>,
    pub last_success_at: Option<String>,
    pub last_resolved_executable_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct McpInventoryRecord {
    pub tools_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRefreshUpdate {
    pub readiness: String,
    pub health: String,
    pub last_error_summary: Option<String>,
    pub last_success_at: Option<String>,
    pub last_resolved_executable_path: Option<String>,
    pub inventory: Vec<McpToolInventoryItem>,
}

pub fn built_in_mcp_servers() -> Vec<BuiltInMcpServerDefinition> {
    vec![]
}

pub fn built_in_definition(id: &str) -> Option<BuiltInMcpServerDefinition> {
    built_in_mcp_servers()
        .into_iter()
        .find(|definition| definition.id == id)
}

pub fn default_transport(transport: Option<String>) -> String {
    transport
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| MCP_TRANSPORT_STDIO.to_string())
}

pub fn request_transport(transport: Option<&str>, url: Option<&str>) -> String {
    match transport {
        Some(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => {
            if url.map(|u| !u.trim().is_empty()).unwrap_or(false) {
                MCP_TRANSPORT_HTTP.to_string()
            } else {
                MCP_TRANSPORT_STDIO.to_string()
            }
        }
    }
}

pub fn validate_create_request(request: &CreateMcpServerRequest) -> Result<(), String> {
    validate_custom_fields(
        &request.display_name,
        &request_transport(request.transport.as_deref(), request.url.as_deref()),
        &request.command,
        request.url.as_deref().unwrap_or_default(),
        &request.env,
        &request.secret_env_keys,
        &request.secret_env,
        &request.headers,
        &request.secret_headers,
        request
            .safety_class
            .as_deref()
            .unwrap_or(MCP_SAFETY_UNKNOWN),
    )
}

pub fn validate_update_request(request: &UpdateMcpServerRequest) -> Result<(), String> {
    validate_custom_fields(
        &request.display_name,
        &request_transport(request.transport.as_deref(), request.url.as_deref()),
        &request.command,
        request.url.as_deref().unwrap_or_default(),
        &request.env,
        &request.secret_env_keys,
        &request.secret_env,
        &request.headers,
        &request.secret_headers,
        request
            .safety_class
            .as_deref()
            .unwrap_or(MCP_SAFETY_UNKNOWN),
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_custom_fields(
    display_name: &str,
    transport: &str,
    command: &str,
    url: &str,
    env: &[McpEnvValue],
    secret_env_keys: &[String],
    secret_env: &[McpEnvValue],
    headers: &[McpEnvValue],
    secret_headers: &[McpEnvValue],
    safety_class: &str,
) -> Result<(), String> {
    if display_name.trim().is_empty() {
        return Err("display_name is required".to_string());
    }
    match transport {
        MCP_TRANSPORT_STDIO => {
            if command.trim().is_empty() {
                return Err("command is required".to_string());
            }
        }
        MCP_TRANSPORT_HTTP => {
            validate_http_url(url)?;
        }
        other => return Err(format!("unsupported MCP transport: {other}")),
    }
    validate_safety_class(safety_class)?;

    let mut all_secret_env: Vec<String> = secret_env_keys.to_vec();
    all_secret_env.extend(secret_env.iter().map(|pair| pair.key.clone()));
    validate_env_keys(env, &all_secret_env)?;

    validate_header_keys(headers, &secret_names(secret_headers))?;
    Ok(())
}

fn validate_http_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("url is required for HTTP MCP servers".to_string());
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("url must start with https:// or http://".to_string());
    }
    Ok(())
}

fn secret_names(pairs: &[McpEnvValue]) -> Vec<String> {
    pairs.iter().map(|pair| pair.key.clone()).collect()
}

fn validate_header_keys(headers: &[McpEnvValue], secret_keys: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for entry in headers {
        validate_header_key(&entry.key)?;
        if !seen.insert(entry.key.to_ascii_lowercase()) {
            return Err(format!("duplicate header: {}", entry.key));
        }
    }
    for key in secret_keys {
        validate_header_key(key)?;
        if !seen.insert(key.to_ascii_lowercase()) {
            return Err(format!(
                "header cannot be both secret and non-secret: {key}"
            ));
        }
    }
    Ok(())
}

fn validate_header_key(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("header name cannot be empty".to_string());
    }
    if !key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(format!("invalid header name: {key}"));
    }
    Ok(())
}

pub fn validate_safety_class(value: &str) -> Result<(), String> {
    match value {
        MCP_SAFETY_READ_ONLY | MCP_SAFETY_MUTATING | MCP_SAFETY_UNKNOWN => Ok(()),
        _ => Err(format!("unsupported MCP safety class: {value}")),
    }
}

pub fn validate_env_keys(env: &[McpEnvValue], secret_env_keys: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for entry in env {
        validate_env_key(&entry.key)?;
        if !seen.insert(entry.key.clone()) {
            return Err(format!("duplicate env key: {}", entry.key));
        }
    }

    let mut secret_seen = HashSet::new();
    for key in secret_env_keys {
        validate_env_key(key)?;
        if !secret_seen.insert(key.clone()) {
            return Err(format!("duplicate secret env key: {key}"));
        }
        if seen.contains(key) {
            return Err(format!(
                "env key cannot be both secret and non-secret: {key}"
            ));
        }
    }

    Ok(())
}

fn validate_env_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("env key cannot be empty".to_string());
    }
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return Err("env key cannot be empty".to_string());
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return Err(format!("invalid env key: {key}"));
    }
    if !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
        return Err(format!("invalid env key: {key}"));
    }
    Ok(())
}

pub fn new_custom_server_id() -> String {
    format!("custom_{}", Uuid::new_v4())
}

pub fn default_custom_scope(scope: Option<String>) -> String {
    scope
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "main_and_exploration".to_string())
}

pub fn default_safety_class(safety_class: Option<String>) -> String {
    safety_class
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| MCP_SAFETY_UNKNOWN.to_string())
}

pub fn serialize_json<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

pub fn parse_json_vec<T: for<'de> Deserialize<'de>>(value: &str) -> Vec<T> {
    serde_json::from_str(value).unwrap_or_default()
}

pub fn compose_built_in_server(
    definition: BuiltInMcpServerDefinition,
    state: Option<McpServerStateRecord>,
    diagnostics: Option<McpDiagnosticsRecord>,
    inventory: Option<McpInventoryRecord>,
) -> McpServer {
    let state = state.unwrap_or(McpServerStateRecord {
        enabled: false,
        trusted: true,
        trust_revoked_reason: None,
    });
    let diagnostics = diagnostics.unwrap_or(McpDiagnosticsRecord {
        readiness: MCP_READINESS_AWAITING_FIRST_CONNECTION.to_string(),
        health: MCP_HEALTH_NOT_CONNECTED.to_string(),
        last_error_summary: None,
        last_success_at: None,
        last_resolved_executable_path: None,
    });
    let fallback_inventory = definition
        .tool_names
        .iter()
        .map(|name| McpToolInventoryItem {
            name: (*name).to_string(),
            description: None,
            read_only: true,
        })
        .collect::<Vec<_>>();
    let inventory = inventory
        .map(|record| parse_json_vec(&record.tools_json))
        .filter(|tools: &Vec<McpToolInventoryItem>| !tools.is_empty())
        .unwrap_or(fallback_inventory);

    McpServer {
        id: definition.id.to_string(),
        kind: MCP_KIND_BUILT_IN.to_string(),
        display_name: definition.display_name.to_string(),
        description: Some(definition.description.to_string()),
        enabled: state.enabled,
        trusted: true,
        trust_revoked_reason: None,
        safety_class: definition.safety_class.to_string(),
        scope: definition.scope.to_string(),
        transport: MCP_TRANSPORT_STDIO.to_string(),
        command: None,
        args: Vec::new(),
        env: Vec::new(),
        url: None,
        headers: Vec::new(),
        secret_env_keys: Vec::new(),
        secret_header_keys: Vec::new(),
        missing_secret_env_keys: Vec::new(),
        missing_secret_header_keys: Vec::new(),
        auth_status: MCP_AUTH_NONE.to_string(),
        readiness: diagnostics.readiness,
        health: diagnostics.health,
        inventory,
        last_error_summary: diagnostics.last_error_summary,
        last_success_at: diagnostics.last_success_at,
        last_resolved_executable_path: diagnostics.last_resolved_executable_path,
        external_fingerprint: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

pub fn compose_custom_server(
    record: CustomMcpServerRecord,
    state: Option<McpServerStateRecord>,
    diagnostics: Option<McpDiagnosticsRecord>,
    inventory: Option<McpInventoryRecord>,
) -> McpServer {
    let state = state.unwrap_or(McpServerStateRecord {
        enabled: false,
        trusted: false,
        trust_revoked_reason: None,
    });
    let diagnostics = diagnostics.unwrap_or(McpDiagnosticsRecord {
        readiness: MCP_READINESS_AWAITING_FIRST_CONNECTION.to_string(),
        health: MCP_HEALTH_NOT_CONNECTED.to_string(),
        last_error_summary: None,
        last_success_at: None,
        last_resolved_executable_path: None,
    });
    let transport = if record.transport.is_empty() {
        MCP_TRANSPORT_STDIO.to_string()
    } else {
        record.transport
    };
    let url = if record.url.is_empty() {
        None
    } else {
        Some(record.url)
    };
    let secret_env_keys: Vec<String> = parse_json_vec(&record.secret_env_keys_json);
    let secret_header_keys: Vec<String> = parse_json_vec(&record.secret_header_keys_json);
    let auth_status = if transport != MCP_TRANSPORT_HTTP {
        MCP_AUTH_NONE
    } else if crate::mcp_credentials::has_oauth_credentials(&record.id) {
        MCP_AUTH_AUTHORIZED
    } else if diagnostics.health == MCP_HEALTH_AUTH_REQUIRED {
        MCP_AUTH_REQUIRED
    } else {
        MCP_AUTH_NONE
    };
    let command = if record.command.is_empty() {
        None
    } else {
        Some(record.command)
    };
    McpServer {
        id: record.id.clone(),
        kind: MCP_KIND_CUSTOM.to_string(),
        display_name: record.display_name,
        description: None,
        enabled: state.enabled,
        trusted: state.trusted,
        trust_revoked_reason: state.trust_revoked_reason,
        safety_class: record.safety_class,
        scope: record.scope,
        transport,
        command,
        args: parse_json_vec(&record.args_json),
        env: parse_json_vec(&record.env_json),
        url,
        headers: parse_json_vec(&record.headers_json),
        missing_secret_env_keys: crate::mcp_credentials::missing_keys(
            &record.id,
            "env",
            &secret_env_keys,
        ),
        missing_secret_header_keys: crate::mcp_credentials::missing_keys(
            &record.id,
            "headers",
            &secret_header_keys,
        ),
        secret_env_keys,
        secret_header_keys,
        auth_status: auth_status.to_string(),
        readiness: diagnostics.readiness,
        health: diagnostics.health,
        inventory: inventory
            .map(|record| parse_json_vec(&record.tools_json))
            .unwrap_or_default(),
        last_error_summary: diagnostics.last_error_summary,
        last_success_at: diagnostics.last_success_at,
        last_resolved_executable_path: diagnostics.last_resolved_executable_path,
        external_fingerprint: record.external_fingerprint,
        created_at: record.created_at,
        updated_at: record.updated_at,
    }
}

pub fn refresh_built_in(_active_workspace_path: Option<&Path>) -> McpRefreshUpdate {
    McpRefreshUpdate {
        readiness: MCP_READINESS_UNAVAILABLE.to_string(),
        health: MCP_HEALTH_NOT_FOUND.to_string(),
        last_error_summary: Some("No built-in MCP server found.".to_string()),
        last_success_at: None,
        last_resolved_executable_path: None,
        inventory: Vec::new(),
    }
}

/// Diagnostics for a custom server the user has not approved. No process is
/// spawned and no network request is made — untrusted config is never touched.
pub fn refresh_untrusted(server: &McpServer) -> McpRefreshUpdate {
    McpRefreshUpdate {
        readiness: MCP_READINESS_UNAVAILABLE.to_string(),
        health: MCP_HEALTH_UNTRUSTED.to_string(),
        last_error_summary: Some(
            "Custom MCP server must be approved before it can connect.".to_string(),
        ),
        last_success_at: None,
        last_resolved_executable_path: server.last_resolved_executable_path.clone(),
        inventory: Vec::new(),
    }
}

pub(crate) fn resolve_executable(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    let path = PathBuf::from(command);
    if path.is_absolute() || command.contains(std::path::MAIN_SEPARATOR) {
        return executable_path(&path);
    }

    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).find_map(|dir| executable_path(&dir.join(command)))
}

fn executable_path(path: &Path) -> Option<PathBuf> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Some(path.to_path_buf()),
        _ => None,
    }
}

pub fn parse_mcp_import(
    source_path: &str,
    content: &str,
    existing_servers: &[McpServer],
) -> Result<McpImportPreview, String> {
    let value: Value = serde_json::from_str(content)
        .map_err(|e| format!("Failed to parse MCP config JSON: {e}"))?;
    let server_map = extract_server_map(&value)
        .ok_or_else(|| "No MCP server map found in config".to_string())?;

    let token = format!("preview_{}", Uuid::new_v4());
    let mut warnings = Vec::new();
    let mut servers = Vec::new();
    let canonical_source = canonical_source_path(source_path);

    for (name, config) in server_map {
        match parse_import_server(&canonical_source, &name, config) {
            Ok(mut item) => {
                if let Some(existing) = existing_servers.iter().find(|server| {
                    server.external_fingerprint.as_deref() == Some(item.external_id.as_str())
                }) {
                    item.matched_server_id = Some(existing.id.clone());
                    item.field_diffs = import_diffs(existing, &item.proposed);
                    item.conflict = !item.field_diffs.is_empty();
                }
                servers.push(item);
            }
            Err(error) => warnings.push(format!("{name}: {error}")),
        }
    }

    if servers.is_empty() && warnings.is_empty() {
        warnings.push("No importable MCP servers were found.".to_string());
    }

    Ok(McpImportPreview {
        token,
        source_path: source_path.to_string(),
        servers,
        warnings,
    })
}

fn canonical_source_path(source_path: &str) -> String {
    std::fs::canonicalize(source_path)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| source_path.to_string())
}

/// A config entry counts as a server definition if it carries a command
/// (string or array) or a URL. Anything else (e.g. sibling config keys under
/// a shared map like `"mcp"`) is ignored by the map detectors below.
fn is_server_entry(config: &Value) -> bool {
    config.get("command").is_some() || config.get("url").is_some()
}

fn extract_server_map(value: &Value) -> Option<BTreeMap<String, &Value>> {
    let candidates = [
        value.get("mcp").and_then(|mcp| mcp.get("servers")),
        value.get("mcp").and_then(|mcp| mcp.get("mcpServers")),
        value.get("mcpServers"),
        value.get("servers"),
        // OpenCode nests entries directly under "mcp".
        value.get("mcp"),
        Some(value),
    ];

    for candidate in candidates.into_iter().flatten() {
        let Some(object) = candidate.as_object() else {
            continue;
        };
        let mut result = BTreeMap::new();
        for (name, config) in object {
            if is_server_entry(config) {
                result.insert(name.clone(), config);
            }
        }
        if !result.is_empty() {
            return Some(result);
        }
    }

    None
}

fn parse_import_server(
    canonical_source: &str,
    name: &str,
    config: &Value,
) -> Result<McpImportPreviewServer, String> {
    let mut warnings = Vec::new();
    let type_hint = config
        .get("type")
        .or_else(|| config.get("transport"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let url = config
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    let is_remote = url.is_some() && !matches!(type_hint.as_str(), "local" | "stdio");
    let (transport, command, args, resolved_url) = if is_remote {
        if type_hint == "sse" {
            warnings.push(
                "This config uses the legacy SSE transport; Gospel connects over Streamable HTTP. If the server only supports SSE it will fail to connect."
                    .to_string(),
            );
        }
        (
            MCP_TRANSPORT_HTTP.to_string(),
            String::new(),
            Vec::new(),
            url,
        )
    } else {
        // Local definitions come in two shapes: `"command": "prog"` plus a
        // separate `args` list, or `"command": ["prog", "arg", ...]` (OpenCode
        // local and some VS Code configs).
        let (command, mut args) = match config.get("command") {
            Some(Value::String(value)) => (value.trim().to_string(), Vec::new()),
            Some(Value::Array(values)) => {
                let mut parts = values
                    .iter()
                    .filter_map(|value| value.as_str().map(ToString::to_string));
                let program = parts.next().unwrap_or_default();
                (program.trim().to_string(), parts.collect::<Vec<_>>())
            }
            _ => (String::new(), Vec::new()),
        };
        match config.get("args") {
            Some(Value::Array(values)) => args.extend(
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(ToString::to_string)),
            ),
            Some(Value::String(value)) => args.push(value.clone()),
            _ => {}
        }
        if command.is_empty() {
            return Err("command is required".to_string());
        }
        (MCP_TRANSPORT_STDIO.to_string(), command, args, None)
    };

    let mut env = Vec::new();
    let mut secret_env = Vec::new();
    // OpenCode calls it "environment"; Claude/Cursor/VS Code call it "env".
    let env_object = config
        .get("env")
        .or_else(|| config.get("environment"))
        .and_then(Value::as_object);
    if let Some(env_object) = env_object {
        for (key, value) in env_object {
            if likely_secret_key(key) {
                secret_env.push(McpEnvValue {
                    key: key.clone(),
                    value: String::new(),
                });
                warnings.push(format!(
                    "{key} was imported as a required secret key without its value."
                ));
                continue;
            }
            env.push(McpEnvValue {
                key: key.clone(),
                value: value.as_str().unwrap_or_default().to_string(),
            });
        }
    }

    let mut headers = Vec::new();
    let mut secret_headers = Vec::new();
    if let Some(header_object) = config.get("headers").and_then(Value::as_object) {
        for (key, value) in header_object {
            if likely_secret_key(key) || key.eq_ignore_ascii_case("authorization") {
                secret_headers.push(McpEnvValue {
                    key: key.clone(),
                    value: String::new(),
                });
                warnings.push(format!(
                    "{key} header was imported as a required secret without its value."
                ));
                continue;
            }
            headers.push(McpEnvValue {
                key: key.clone(),
                value: value.as_str().unwrap_or_default().to_string(),
            });
        }
    }

    let safety_class = config
        .get("safetyClass")
        .or_else(|| config.get("safety_class"))
        .and_then(Value::as_str)
        .unwrap_or(MCP_SAFETY_UNKNOWN)
        .to_string();

    let proposed = CreateMcpServerRequest {
        display_name: name.to_string(),
        command,
        args,
        env,
        secret_env_keys: Vec::new(),
        secret_env,
        transport: Some(transport),
        url: resolved_url,
        headers,
        secret_headers,
        safety_class: Some(safety_class),
        scope: Some("main_and_exploration".to_string()),
    };
    validate_create_request(&proposed)?;

    Ok(McpImportPreviewServer {
        external_id: external_fingerprint(canonical_source, name),
        name: name.to_string(),
        proposed,
        matched_server_id: None,
        conflict: false,
        field_diffs: Vec::new(),
        warnings,
    })
}

fn likely_secret_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    ["TOKEN", "SECRET", "PASSWORD", "API_KEY", "ACCESS_KEY"]
        .iter()
        .any(|needle| upper.contains(needle))
}

fn external_fingerprint(canonical_source: &str, name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update("opencode".as_bytes());
    hasher.update([0]);
    hasher.update(canonical_source.as_bytes());
    hasher.update([0]);
    hasher.update(name.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn import_diffs(
    existing: &McpServer,
    proposed: &CreateMcpServerRequest,
) -> Vec<McpImportFieldDiff> {
    let mut diffs = Vec::new();
    push_diff(
        &mut diffs,
        "displayName",
        Some(existing.display_name.clone()),
        Some(proposed.display_name.clone()),
    );
    let proposed_transport =
        request_transport(proposed.transport.as_deref(), proposed.url.as_deref());
    push_diff(
        &mut diffs,
        "transport",
        Some(existing.transport.clone()),
        Some(proposed_transport.clone()),
    );
    if proposed_transport == MCP_TRANSPORT_HTTP {
        push_diff(
            &mut diffs,
            "url",
            existing.url.clone(),
            proposed.url.clone(),
        );
    } else {
        push_diff(
            &mut diffs,
            "command",
            existing.command.clone(),
            Some(proposed.command.clone()),
        );
        push_diff(
            &mut diffs,
            "args",
            Some(existing.args.join(" ")),
            Some(proposed.args.join(" ")),
        );
    }
    push_diff(
        &mut diffs,
        "safetyClass",
        Some(existing.safety_class.clone()),
        proposed.safety_class.clone(),
    );
    diffs
}

fn push_diff(
    diffs: &mut Vec<McpImportFieldDiff>,
    field: &str,
    current: Option<String>,
    incoming: Option<String>,
) {
    if current == incoming {
        return;
    }
    diffs.push(McpImportFieldDiff {
        field: field.to_string(),
        current,
        incoming,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_validation_rejects_secret_overlap() {
        let error = validate_env_keys(
            &[McpEnvValue {
                key: "TOKEN".to_string(),
                value: "plain".to_string(),
            }],
            &["TOKEN".to_string()],
        )
        .unwrap_err();

        assert!(error.contains("both secret and non-secret"));
    }

    #[test]
    fn opencode_import_drops_secret_values() {
        let content = r#"{
          "mcp": {
            "servers": {
              "github": {
                "command": "gh",
                "args": ["mcp", "serve"],
                "env": {
                  "GITHUB_TOKEN": "secret",
                  "LOG_LEVEL": "debug"
                }
              }
            }
          }
        }"#;

        let preview = parse_mcp_import("/tmp/opencode.json", content, &[]).unwrap();
        let server = preview.servers.first().unwrap();

        assert_eq!(server.proposed.command, "gh");
        assert_eq!(server.proposed.env[0].key, "LOG_LEVEL");
        assert_eq!(server.proposed.secret_env[0].key, "GITHUB_TOKEN");
        assert_eq!(server.proposed.secret_env[0].value, "");
        assert!(server.warnings[0].contains("without its value"));
    }

    #[test]
    fn import_accepts_remote_url_entries() {
        let content = r#"{
          "mcpServers": {
            "github": { "command": "npx", "args": ["-y", "server-github"] },
            "mobbin": { "type": "remote", "url": "https://api.mobbin.com/mcp" },
            "api": {
              "url": "https://api.example.com/mcp",
              "headers": { "Authorization": "Bearer tok", "X-Team": "core" }
            }
          }
        }"#;

        let preview = parse_mcp_import("/tmp/claude.json", content, &[]).unwrap();
        assert_eq!(preview.servers.len(), 3);
        let mobbin = preview
            .servers
            .iter()
            .find(|server| server.name == "mobbin")
            .unwrap();
        assert_eq!(
            mobbin.proposed.transport.as_deref(),
            Some(MCP_TRANSPORT_HTTP)
        );
        assert_eq!(
            mobbin.proposed.url.as_deref(),
            Some("https://api.mobbin.com/mcp")
        );
        let api = preview
            .servers
            .iter()
            .find(|server| server.name == "api")
            .unwrap();
        assert_eq!(api.proposed.secret_headers[0].key, "Authorization");
        assert_eq!(api.proposed.headers[0].key, "X-Team");
    }

    #[test]
    fn import_accepts_opencode_local_remote_shapes() {
        let content = r#"{
          "mcp": {
            "everything": {
              "type": "local",
              "command": ["npx", "-y", "@modelcontextprotocol/server-everything"],
              "environment": { "LOG": "1" }
            },
            "mobbin": { "type": "remote", "url": "https://api.mobbin.com/mcp" }
          }
        }"#;

        let preview = parse_mcp_import("/tmp/opencode.json", content, &[]).unwrap();
        assert_eq!(preview.servers.len(), 2);
        let local = preview
            .servers
            .iter()
            .find(|server| server.name == "everything")
            .unwrap();
        assert_eq!(local.proposed.command, "npx");
        assert_eq!(
            local.proposed.args,
            vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-everything".to_string()
            ]
        );
        assert_eq!(local.proposed.env[0].key, "LOG");
        let remote = preview
            .servers
            .iter()
            .find(|server| server.name == "mobbin")
            .unwrap();
        assert_eq!(
            remote.proposed.transport.as_deref(),
            Some(MCP_TRANSPORT_HTTP)
        );
    }

    #[test]
    fn sse_type_imports_as_http_with_warning() {
        let content = r#"{
          "mcpServers": {
            "legacy": { "type": "sse", "url": "https://old.example.com/sse" }
          }
        }"#;

        let preview = parse_mcp_import("/tmp/vscode.json", content, &[]).unwrap();
        let server = preview.servers.first().unwrap();
        assert_eq!(
            server.proposed.transport.as_deref(),
            Some(MCP_TRANSPORT_HTTP)
        );
        assert!(server.warnings[0].contains("SSE"));
    }

    #[test]
    fn untrusted_custom_refresh_does_not_resolve_or_execute() {
        let server = McpServer {
            id: "custom_1".to_string(),
            kind: MCP_KIND_CUSTOM.to_string(),
            display_name: "Local".to_string(),
            description: None,
            enabled: true,
            trusted: false,
            trust_revoked_reason: None,
            safety_class: MCP_SAFETY_UNKNOWN.to_string(),
            scope: "main_and_exploration".to_string(),
            transport: MCP_TRANSPORT_STDIO.to_string(),
            command: Some("definitely-not-a-command".to_string()),
            args: Vec::new(),
            env: Vec::new(),
            url: None,
            headers: Vec::new(),
            secret_env_keys: Vec::new(),
            secret_header_keys: Vec::new(),
            missing_secret_env_keys: Vec::new(),
            missing_secret_header_keys: Vec::new(),
            auth_status: MCP_AUTH_NONE.to_string(),
            readiness: MCP_READINESS_AWAITING_FIRST_CONNECTION.to_string(),
            health: MCP_HEALTH_NOT_CONNECTED.to_string(),
            inventory: Vec::new(),
            last_error_summary: None,
            last_success_at: None,
            last_resolved_executable_path: None,
            external_fingerprint: None,
            created_at: String::new(),
            updated_at: String::new(),
        };

        let update = refresh_untrusted(&server);

        assert_eq!(update.health, MCP_HEALTH_UNTRUSTED);
        assert!(update.last_resolved_executable_path.is_none());
    }
}
