use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const BUILT_IN_CODEBASE_KNOWLEDGE_ID: &str = "codebase_knowledge";
pub const MCP_KIND_BUILT_IN: &str = "built_in";
pub const MCP_KIND_CUSTOM: &str = "custom";
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
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<McpEnvValue>,
    pub secret_env_keys: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMcpServerRequest {
    pub display_name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<McpEnvValue>,
    #[serde(default)]
    pub secret_env_keys: Vec<String>,
    #[serde(default)]
    pub safety_class: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMcpServerRequest {
    pub display_name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<McpEnvValue>,
    #[serde(default)]
    pub secret_env_keys: Vec<String>,
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
    pub command: String,
    pub args_json: String,
    pub env_json: String,
    pub secret_env_keys_json: String,
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
    vec![BuiltInMcpServerDefinition {
        id: BUILT_IN_CODEBASE_KNOWLEDGE_ID,
        display_name: "Codebase knowledge",
        description: "Read-only MCP server for corpus and context-search knowledge.",
        safety_class: MCP_SAFETY_READ_ONLY,
        scope: "main",
        tool_names: &[
            "corpus_summary",
            "corpus_query",
            "corpus_neighbors",
            "context_search",
        ],
    }]
}

pub fn built_in_definition(id: &str) -> Option<BuiltInMcpServerDefinition> {
    built_in_mcp_servers()
        .into_iter()
        .find(|definition| definition.id == id)
}

pub fn validate_create_request(request: &CreateMcpServerRequest) -> Result<(), String> {
    validate_custom_fields(
        &request.display_name,
        &request.command,
        &request.env,
        &request.secret_env_keys,
        request
            .safety_class
            .as_deref()
            .unwrap_or(MCP_SAFETY_UNKNOWN),
    )
}

pub fn validate_update_request(request: &UpdateMcpServerRequest) -> Result<(), String> {
    validate_custom_fields(
        &request.display_name,
        &request.command,
        &request.env,
        &request.secret_env_keys,
        request
            .safety_class
            .as_deref()
            .unwrap_or(MCP_SAFETY_UNKNOWN),
    )
}

fn validate_custom_fields(
    display_name: &str,
    command: &str,
    env: &[McpEnvValue],
    secret_env_keys: &[String],
    safety_class: &str,
) -> Result<(), String> {
    if display_name.trim().is_empty() {
        return Err("display_name is required".to_string());
    }
    if command.trim().is_empty() {
        return Err("command is required".to_string());
    }
    validate_safety_class(safety_class)?;
    validate_env_keys(env, secret_env_keys)?;
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
        command: None,
        args: Vec::new(),
        env: Vec::new(),
        secret_env_keys: Vec::new(),
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
    McpServer {
        id: record.id,
        kind: MCP_KIND_CUSTOM.to_string(),
        display_name: record.display_name,
        description: None,
        enabled: state.enabled,
        trusted: state.trusted,
        trust_revoked_reason: state.trust_revoked_reason,
        safety_class: record.safety_class,
        scope: record.scope,
        command: Some(record.command),
        args: parse_json_vec(&record.args_json),
        env: parse_json_vec(&record.env_json),
        secret_env_keys: parse_json_vec(&record.secret_env_keys_json),
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

pub fn refresh_built_in(active_workspace_path: Option<&Path>) -> McpRefreshUpdate {
    let inventory = built_in_definition(BUILT_IN_CODEBASE_KNOWLEDGE_ID)
        .map(|definition| {
            definition
                .tool_names
                .iter()
                .map(|name| McpToolInventoryItem {
                    name: (*name).to_string(),
                    description: None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if active_workspace_path.is_none() {
        return McpRefreshUpdate {
            readiness: MCP_READINESS_UNAVAILABLE.to_string(),
            health: MCP_HEALTH_WORKSPACE_REQUIRED.to_string(),
            last_error_summary: Some(
                "Select a workspace before testing this built-in server.".to_string(),
            ),
            last_success_at: None,
            last_resolved_executable_path: None,
            inventory,
        };
    }

    McpRefreshUpdate {
        readiness: MCP_READINESS_READY.to_string(),
        health: MCP_HEALTH_CONNECTED.to_string(),
        last_error_summary: None,
        last_success_at: Some("CURRENT_TIMESTAMP".to_string()),
        last_resolved_executable_path: None,
        inventory,
    }
}

pub fn refresh_custom(server: &McpServer) -> McpRefreshUpdate {
    if !server.trusted {
        return McpRefreshUpdate {
            readiness: MCP_READINESS_UNAVAILABLE.to_string(),
            health: MCP_HEALTH_UNTRUSTED.to_string(),
            last_error_summary: Some(
                "Custom MCP server must be trusted before discovery or execution.".to_string(),
            ),
            last_success_at: None,
            last_resolved_executable_path: server.last_resolved_executable_path.clone(),
            inventory: Vec::new(),
        };
    }

    let command = server.command.as_deref().unwrap_or_default();
    match resolve_executable(command) {
        Some(path) => McpRefreshUpdate {
            readiness: MCP_READINESS_AWAITING_FIRST_CONNECTION.to_string(),
            health: MCP_HEALTH_NOT_CONNECTED.to_string(),
            last_error_summary: Some(
                "Executable resolved. Protocol handshake is not connected yet.".to_string(),
            ),
            last_success_at: None,
            last_resolved_executable_path: Some(path.display().to_string()),
            inventory: server.inventory.clone(),
        },
        None => McpRefreshUpdate {
            readiness: MCP_READINESS_UNAVAILABLE.to_string(),
            health: MCP_HEALTH_NOT_FOUND.to_string(),
            last_error_summary: Some(format!("Executable was not found: {command}")),
            last_success_at: None,
            last_resolved_executable_path: None,
            inventory: Vec::new(),
        },
    }
}

fn resolve_executable(command: &str) -> Option<PathBuf> {
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

pub fn parse_opencode_import(
    source_path: &str,
    content: &str,
    existing_servers: &[McpServer],
) -> Result<McpImportPreview, String> {
    let value: Value = serde_json::from_str(content)
        .map_err(|e| format!("Failed to parse OpenCode MCP config JSON: {e}"))?;
    let server_map = extract_server_map(&value)
        .ok_or_else(|| "No MCP server map found in OpenCode config".to_string())?;

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

fn extract_server_map(value: &Value) -> Option<BTreeMap<String, &Value>> {
    let candidates = [
        value.get("mcp").and_then(|mcp| mcp.get("servers")),
        value.get("mcp").and_then(|mcp| mcp.get("mcpServers")),
        value.get("mcpServers"),
        value.get("servers"),
        Some(value),
    ];

    for candidate in candidates.into_iter().flatten() {
        let Some(object) = candidate.as_object() else {
            continue;
        };
        let mut result = BTreeMap::new();
        for (name, config) in object {
            if config.get("command").is_some() {
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
    let command = config
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "command is required".to_string())?
        .to_string();
    let args = match config.get("args") {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|value| value.as_str().map(ToString::to_string))
            .collect(),
        Some(Value::String(value)) => vec![value.clone()],
        _ => Vec::new(),
    };

    let mut env = Vec::new();
    let mut secret_env_keys = Vec::new();
    let mut warnings = Vec::new();
    if let Some(env_object) = config.get("env").and_then(Value::as_object) {
        for (key, value) in env_object {
            if likely_secret_key(key) {
                secret_env_keys.push(key.clone());
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
        secret_env_keys,
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

        let preview = parse_opencode_import("/tmp/opencode.json", content, &[]).unwrap();
        let server = preview.servers.first().unwrap();

        assert_eq!(server.proposed.command, "gh");
        assert_eq!(server.proposed.env[0].key, "LOG_LEVEL");
        assert_eq!(server.proposed.secret_env_keys, vec!["GITHUB_TOKEN"]);
        assert!(server.warnings[0].contains("without its value"));
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
            command: Some("definitely-not-a-command".to_string()),
            args: Vec::new(),
            env: Vec::new(),
            secret_env_keys: Vec::new(),
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

        let update = refresh_custom(&server);

        assert_eq!(update.health, MCP_HEALTH_UNTRUSTED);
        assert!(update.last_resolved_executable_path.is_none());
    }
}
