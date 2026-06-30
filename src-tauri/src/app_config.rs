use crate::mcp::{
    self, CreateMcpServerRequest, McpApplyImportRequest, McpApplyImportResult,
    McpDiagnosticsRecord, McpImportPreview, McpInventoryRecord, McpRefreshUpdate, McpServer,
    McpServerStateRecord, UpdateMcpServerRequest, MCP_HEALTH_NOT_CONNECTED, MCP_KIND_BUILT_IN,
    MCP_KIND_CUSTOM, MCP_READINESS_AWAITING_FIRST_CONNECTION,
};
use crate::models::ModelRegistry;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum AppConfigError {
    #[error("app config IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("app config database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("provider {0} is not supported")]
    UnsupportedProvider(String),
    #[error("workspace not found: {0}")]
    WorkspaceNotFound(String),
    #[error("workspace path already exists: {0}")]
    WorkspacePathExists(String),
    #[error("MCP server not found: {0}")]
    McpServerNotFound(String),
    #[error("invalid MCP server: {0}")]
    InvalidMcpServer(String),
    #[error("MCP import error: {0}")]
    McpImport(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub path: String,
    pub session_count: i64,
    pub created_at: String,
}

pub struct AppConfigStore {
    conn: Mutex<Connection>,
}

pub struct AppConfigState {
    pub store: Option<AppConfigStore>,
    pub init_warning: Option<String>,
}

const APP_CONFIG_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS provider_settings (
    provider_id TEXT PRIMARY KEY NOT NULL,
    visible INTEGER NOT NULL CHECK (visible IN (0, 1)),
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    session_count INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS app_config (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS mcp_custom_servers (
    id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL,
    command TEXT NOT NULL,
    args_json TEXT NOT NULL DEFAULT '[]',
    env_json TEXT NOT NULL DEFAULT '[]',
    secret_env_keys_json TEXT NOT NULL DEFAULT '[]',
    safety_class TEXT NOT NULL DEFAULT 'unknown',
    scope TEXT NOT NULL DEFAULT 'main_and_exploration',
    external_fingerprint TEXT UNIQUE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS mcp_server_state (
    server_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    trusted INTEGER NOT NULL DEFAULT 0 CHECK (trusted IN (0, 1)),
    trust_revoked_reason TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (server_id, kind)
);
CREATE TABLE IF NOT EXISTS mcp_server_inventory (
    server_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    tools_json TEXT NOT NULL DEFAULT '[]',
    refreshed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (server_id, kind)
);
CREATE TABLE IF NOT EXISTS mcp_server_diagnostics (
    server_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    readiness TEXT NOT NULL DEFAULT 'awaiting_first_connection',
    health TEXT NOT NULL DEFAULT 'not_connected',
    last_error_summary TEXT,
    last_success_at TEXT,
    last_resolved_executable_path TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (server_id, kind)
);
CREATE TABLE IF NOT EXISTS mcp_import_previews (
    token TEXT PRIMARY KEY NOT NULL,
    source_path TEXT NOT NULL,
    preview_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);";

impl AppConfigStore {
    pub fn new() -> Result<Self, AppConfigError> {
        let dir = app_data_dir();
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(dir.join("app-config.sqlite3"))?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self, AppConfigError> {
        conn.execute_batch(APP_CONFIG_SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    pub(crate) fn in_memory_for_test() -> Result<Self, AppConfigError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    pub fn provider_visibility(&self, provider: &str) -> Result<bool, AppConfigError> {
        validate_provider(provider)?;
        let conn = self.conn.lock().unwrap();
        let visible = conn
            .query_row(
                "SELECT visible FROM provider_settings WHERE provider_id = ?1",
                params![provider],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        Ok(visible.map(|v| v != 0).unwrap_or(true))
    }

    pub fn set_provider_visibility(
        &self,
        provider: &str,
        visible: bool,
    ) -> Result<(), AppConfigError> {
        validate_provider(provider)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO provider_settings (provider_id, visible, updated_at)
             VALUES (?1, ?2, CURRENT_TIMESTAMP)
             ON CONFLICT(provider_id) DO UPDATE SET
                visible = excluded.visible,
                updated_at = CURRENT_TIMESTAMP",
            params![provider, if visible { 1 } else { 0 }],
        )?;
        Ok(())
    }

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>, AppConfigError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, session_count, created_at FROM workspaces ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![], |row| {
            Ok(Workspace {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                session_count: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        let mut workspaces = Vec::new();
        for ws in rows {
            workspaces.push(ws?);
        }
        Ok(workspaces)
    }

    pub fn add_workspace(&self, path: &str) -> Result<Workspace, AppConfigError> {
        let canonical = std::fs::canonicalize(path).map_err(AppConfigError::Io)?;
        let canonical_str = canonical.to_string_lossy().to_string();

        if !canonical.is_dir() {
            return Err(AppConfigError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Path is not a directory: {}", canonical_str),
            )));
        }

        let name = canonical
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".to_string());

        let id = Uuid::new_v4().to_string();

        let conn = self.conn.lock().unwrap();

        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM workspaces WHERE path = ?1",
                params![canonical_str],
                |row| row.get(0),
            )
            .optional()?;
        if existing.is_some() {
            return Err(AppConfigError::WorkspacePathExists(canonical_str));
        }

        conn.execute(
            "INSERT INTO workspaces (id, name, path) VALUES (?1, ?2, ?3)",
            params![id, name, canonical_str],
        )?;

        let workspace = Workspace {
            id,
            name,
            path: canonical_str,
            session_count: 0,
            created_at: String::new(),
        };

        Ok(workspace)
    }

    pub fn remove_workspace(&self, id: &str) -> Result<(), AppConfigError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute("DELETE FROM workspaces WHERE id = ?1", params![id])?;
        if rows == 0 {
            return Err(AppConfigError::WorkspaceNotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn get_active_workspace(&self) -> Result<Option<Workspace>, AppConfigError> {
        let conn = self.conn.lock().unwrap();
        let active_id: Option<String> = conn
            .query_row(
                "SELECT value FROM app_config WHERE key = 'active_workspace'",
                params![],
                |row| row.get(0),
            )
            .optional()?;

        match active_id {
            Some(id) => {
                let ws = conn
                    .query_row(
                        "SELECT id, name, path, session_count, created_at FROM workspaces WHERE id = ?1",
                        params![id],
                        |row| {
                            Ok(Workspace {
                                id: row.get(0)?,
                                name: row.get(1)?,
                                path: row.get(2)?,
                                session_count: row.get(3)?,
                                created_at: row.get(4)?,
                            })
                        },
                    )
                    .optional()?;
                Ok(ws)
            }
            None => Ok(None),
        }
    }

    pub fn set_active_workspace(&self, id: &str) -> Result<(), AppConfigError> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) FROM workspaces WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0).map(|c| c > 0),
        )?;
        if !exists {
            return Err(AppConfigError::WorkspaceNotFound(id.to_string()));
        }
        conn.execute(
            "INSERT INTO app_config (key, value, updated_at)
             VALUES ('active_workspace', ?1, CURRENT_TIMESTAMP)
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = CURRENT_TIMESTAMP",
            params![id],
        )?;
        Ok(())
    }

    pub fn clear_active_workspace(&self) -> Result<(), AppConfigError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM app_config WHERE key = 'active_workspace'",
            params![],
        )?;
        Ok(())
    }

    pub fn get_config_value(&self, key: &str) -> Result<Option<String>, AppConfigError> {
        let conn = self.conn.lock().unwrap();
        let value = conn
            .query_row(
                "SELECT value FROM app_config WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    }

    pub fn set_config_value(&self, key: &str, value: &str) -> Result<(), AppConfigError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO app_config (key, value, updated_at)
             VALUES (?1, ?2, CURRENT_TIMESTAMP)
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = CURRENT_TIMESTAMP",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_workspace_path(&self) -> Result<Option<String>, AppConfigError> {
        let active = self.get_active_workspace()?;
        Ok(active.map(|ws| ws.path))
    }

    pub fn list_mcp_servers(&self) -> Result<Vec<McpServer>, AppConfigError> {
        let conn = self.conn.lock().unwrap();
        list_mcp_servers_locked(&conn)
    }

    pub fn create_mcp_server(
        &self,
        request: CreateMcpServerRequest,
    ) -> Result<McpServer, AppConfigError> {
        mcp::validate_create_request(&request).map_err(AppConfigError::InvalidMcpServer)?;
        let conn = self.conn.lock().unwrap();
        let id = mcp::new_custom_server_id();
        insert_custom_mcp_server_locked(&conn, &id, &request, None)?;
        query_mcp_server_locked(&conn, MCP_KIND_CUSTOM, &id)
    }

    pub fn update_mcp_server(
        &self,
        id: &str,
        request: UpdateMcpServerRequest,
    ) -> Result<McpServer, AppConfigError> {
        mcp::validate_update_request(&request).map_err(AppConfigError::InvalidMcpServer)?;
        let conn = self.conn.lock().unwrap();
        update_custom_mcp_server_locked(&conn, id, &request, None)?;
        query_mcp_server_locked(&conn, MCP_KIND_CUSTOM, id)
    }

    pub fn delete_mcp_server(&self, id: &str) -> Result<(), AppConfigError> {
        let conn = self.conn.lock().unwrap();
        if query_custom_mcp_record_locked(&conn, id)?.is_none() {
            return Err(AppConfigError::McpServerNotFound(id.to_string()));
        }
        conn.execute("DELETE FROM mcp_custom_servers WHERE id = ?1", params![id])?;
        conn.execute(
            "DELETE FROM mcp_server_state WHERE server_id = ?1 AND kind = ?2",
            params![id, MCP_KIND_CUSTOM],
        )?;
        conn.execute(
            "DELETE FROM mcp_server_inventory WHERE server_id = ?1 AND kind = ?2",
            params![id, MCP_KIND_CUSTOM],
        )?;
        conn.execute(
            "DELETE FROM mcp_server_diagnostics WHERE server_id = ?1 AND kind = ?2",
            params![id, MCP_KIND_CUSTOM],
        )?;
        Ok(())
    }

    pub fn set_mcp_server_enabled(
        &self,
        kind: &str,
        id: &str,
        enabled: bool,
    ) -> Result<McpServer, AppConfigError> {
        let conn = self.conn.lock().unwrap();
        ensure_mcp_server_exists_locked(&conn, kind, id)?;
        let default_trusted = if kind == MCP_KIND_BUILT_IN { 1 } else { 0 };
        conn.execute(
            "INSERT INTO mcp_server_state (
                server_id, kind, enabled, trusted, trust_revoked_reason, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, NULL, CURRENT_TIMESTAMP)
             ON CONFLICT(server_id, kind) DO UPDATE SET
                enabled = excluded.enabled,
                updated_at = CURRENT_TIMESTAMP",
            params![id, kind, bool_int(enabled), default_trusted],
        )?;
        query_mcp_server_locked(&conn, kind, id)
    }

    pub fn trust_mcp_server(&self, id: &str) -> Result<McpServer, AppConfigError> {
        let conn = self.conn.lock().unwrap();
        ensure_mcp_server_exists_locked(&conn, MCP_KIND_CUSTOM, id)?;
        conn.execute(
            "INSERT INTO mcp_server_state (
                server_id, kind, enabled, trusted, trust_revoked_reason, updated_at
             )
             VALUES (?1, ?2, 0, 1, NULL, CURRENT_TIMESTAMP)
             ON CONFLICT(server_id, kind) DO UPDATE SET
                trusted = 1,
                trust_revoked_reason = NULL,
                updated_at = CURRENT_TIMESTAMP",
            params![id, MCP_KIND_CUSTOM],
        )?;
        query_mcp_server_locked(&conn, MCP_KIND_CUSTOM, id)
    }

    pub fn revoke_trust_mcp_server(&self, id: &str) -> Result<McpServer, AppConfigError> {
        let conn = self.conn.lock().unwrap();
        ensure_mcp_server_exists_locked(&conn, MCP_KIND_CUSTOM, id)?;
        conn.execute(
            "INSERT INTO mcp_server_state (
                server_id, kind, enabled, trusted, trust_revoked_reason, updated_at
             )
             VALUES (?1, ?2, 0, 0, 'manual_revoke', CURRENT_TIMESTAMP)
             ON CONFLICT(server_id, kind) DO UPDATE SET
                trusted = 0,
                trust_revoked_reason = 'manual_revoke',
                updated_at = CURRENT_TIMESTAMP",
            params![id, MCP_KIND_CUSTOM],
        )?;
        query_mcp_server_locked(&conn, MCP_KIND_CUSTOM, id)
    }

    pub fn refresh_mcp_server(
        &self,
        kind: &str,
        id: &str,
        active_workspace_path: Option<&Path>,
    ) -> Result<McpServer, AppConfigError> {
        let server = {
            let conn = self.conn.lock().unwrap();
            query_mcp_server_locked(&conn, kind, id)?
        };

        let update = if kind == MCP_KIND_BUILT_IN {
            mcp::refresh_built_in(active_workspace_path)
        } else {
            mcp::refresh_custom(&server)
        };

        let conn = self.conn.lock().unwrap();
        persist_mcp_refresh_locked(&conn, kind, id, &update)?;
        query_mcp_server_locked(&conn, kind, id)
    }

    pub fn preview_import_mcp_servers(
        &self,
        source_path: &str,
    ) -> Result<McpImportPreview, AppConfigError> {
        let content = std::fs::read_to_string(source_path)?;
        let existing_servers = self.list_mcp_servers()?;
        let preview = mcp::parse_opencode_import(source_path, &content, &existing_servers)
            .map_err(AppConfigError::McpImport)?;
        let preview_json = mcp::serialize_json(&preview).map_err(AppConfigError::McpImport)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO mcp_import_previews (token, source_path, preview_json, created_at)
             VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)",
            params![&preview.token, source_path, preview_json],
        )?;
        Ok(preview)
    }

    pub fn apply_import_mcp_servers(
        &self,
        request: McpApplyImportRequest,
    ) -> Result<McpApplyImportResult, AppConfigError> {
        let conn = self.conn.lock().unwrap();
        let preview_json: String = conn
            .query_row(
                "SELECT preview_json FROM mcp_import_previews WHERE token = ?1",
                params![&request.token],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| {
                AppConfigError::McpImport("Import preview expired or was not found".to_string())
            })?;
        let preview: McpImportPreview = serde_json::from_str(&preview_json)
            .map_err(|e| AppConfigError::McpImport(e.to_string()))?;
        let selected: HashSet<String> = request.selected_external_ids.into_iter().collect();
        let mut result = McpApplyImportResult {
            created: Vec::new(),
            updated: Vec::new(),
            skipped: Vec::new(),
            warnings: preview.warnings.clone(),
        };

        for item in preview.servers {
            if !selected.contains(&item.external_id) {
                result.skipped.push(item.name);
                continue;
            }

            if let Some(existing_id) = item.matched_server_id {
                if request.overwrite_existing {
                    let update = UpdateMcpServerRequest {
                        display_name: item.proposed.display_name,
                        command: item.proposed.command,
                        args: item.proposed.args,
                        env: item.proposed.env,
                        secret_env_keys: item.proposed.secret_env_keys,
                        safety_class: item.proposed.safety_class,
                        scope: item.proposed.scope,
                    };
                    update_custom_mcp_server_locked(
                        &conn,
                        &existing_id,
                        &update,
                        Some(&item.external_id),
                    )?;
                    result.updated.push(existing_id);
                } else {
                    result.skipped.push(item.name);
                }
                result.warnings.extend(item.warnings);
                continue;
            }

            let id = mcp::new_custom_server_id();
            insert_custom_mcp_server_locked(&conn, &id, &item.proposed, Some(&item.external_id))?;
            result.created.push(id);
            result.warnings.extend(item.warnings);
        }

        conn.execute(
            "DELETE FROM mcp_import_previews WHERE token = ?1",
            params![&request.token],
        )?;

        Ok(result)
    }
}

fn app_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("gospel")
}

fn bool_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn list_mcp_servers_locked(conn: &Connection) -> Result<Vec<McpServer>, AppConfigError> {
    let mut servers = Vec::new();
    for definition in mcp::built_in_mcp_servers() {
        servers.push(mcp::compose_built_in_server(
            definition.clone(),
            query_mcp_state_locked(conn, MCP_KIND_BUILT_IN, definition.id)?,
            query_mcp_diagnostics_locked(conn, MCP_KIND_BUILT_IN, definition.id)?,
            query_mcp_inventory_locked(conn, MCP_KIND_BUILT_IN, definition.id)?,
        ));
    }

    for record in query_custom_mcp_records_locked(conn)? {
        let id = record.id.clone();
        servers.push(mcp::compose_custom_server(
            record,
            query_mcp_state_locked(conn, MCP_KIND_CUSTOM, &id)?,
            query_mcp_diagnostics_locked(conn, MCP_KIND_CUSTOM, &id)?,
            query_mcp_inventory_locked(conn, MCP_KIND_CUSTOM, &id)?,
        ));
    }

    Ok(servers)
}

fn query_mcp_server_locked(
    conn: &Connection,
    kind: &str,
    id: &str,
) -> Result<McpServer, AppConfigError> {
    if kind == MCP_KIND_BUILT_IN {
        let definition = mcp::built_in_definition(id)
            .ok_or_else(|| AppConfigError::McpServerNotFound(id.to_string()))?;
        return Ok(mcp::compose_built_in_server(
            definition,
            query_mcp_state_locked(conn, MCP_KIND_BUILT_IN, id)?,
            query_mcp_diagnostics_locked(conn, MCP_KIND_BUILT_IN, id)?,
            query_mcp_inventory_locked(conn, MCP_KIND_BUILT_IN, id)?,
        ));
    }

    if kind != MCP_KIND_CUSTOM {
        return Err(AppConfigError::InvalidMcpServer(format!(
            "unsupported MCP server kind: {kind}"
        )));
    }

    let record = query_custom_mcp_record_locked(conn, id)?
        .ok_or_else(|| AppConfigError::McpServerNotFound(id.to_string()))?;
    Ok(mcp::compose_custom_server(
        record,
        query_mcp_state_locked(conn, MCP_KIND_CUSTOM, id)?,
        query_mcp_diagnostics_locked(conn, MCP_KIND_CUSTOM, id)?,
        query_mcp_inventory_locked(conn, MCP_KIND_CUSTOM, id)?,
    ))
}

fn ensure_mcp_server_exists_locked(
    conn: &Connection,
    kind: &str,
    id: &str,
) -> Result<(), AppConfigError> {
    query_mcp_server_locked(conn, kind, id).map(|_| ())
}

fn query_custom_mcp_records_locked(
    conn: &Connection,
) -> Result<Vec<mcp::CustomMcpServerRecord>, AppConfigError> {
    let mut stmt = conn.prepare(
        "SELECT id, display_name, command, args_json, env_json, secret_env_keys_json,
                safety_class, scope, external_fingerprint, created_at, updated_at
         FROM mcp_custom_servers
         ORDER BY display_name COLLATE NOCASE, created_at",
    )?;
    let rows = stmt.query_map(params![], custom_mcp_record_from_row)?;
    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    Ok(records)
}

fn query_custom_mcp_record_locked(
    conn: &Connection,
    id: &str,
) -> Result<Option<mcp::CustomMcpServerRecord>, AppConfigError> {
    conn.query_row(
        "SELECT id, display_name, command, args_json, env_json, secret_env_keys_json,
                safety_class, scope, external_fingerprint, created_at, updated_at
         FROM mcp_custom_servers
         WHERE id = ?1",
        params![id],
        custom_mcp_record_from_row,
    )
    .optional()
    .map_err(AppConfigError::Database)
}

fn custom_mcp_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<mcp::CustomMcpServerRecord> {
    Ok(mcp::CustomMcpServerRecord {
        id: row.get(0)?,
        display_name: row.get(1)?,
        command: row.get(2)?,
        args_json: row.get(3)?,
        env_json: row.get(4)?,
        secret_env_keys_json: row.get(5)?,
        safety_class: row.get(6)?,
        scope: row.get(7)?,
        external_fingerprint: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn query_mcp_state_locked(
    conn: &Connection,
    kind: &str,
    id: &str,
) -> Result<Option<McpServerStateRecord>, AppConfigError> {
    conn.query_row(
        "SELECT enabled, trusted, trust_revoked_reason
         FROM mcp_server_state
         WHERE server_id = ?1 AND kind = ?2",
        params![id, kind],
        |row| {
            Ok(McpServerStateRecord {
                enabled: row.get::<_, i64>(0)? != 0,
                trusted: row.get::<_, i64>(1)? != 0,
                trust_revoked_reason: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(AppConfigError::Database)
}

fn query_mcp_diagnostics_locked(
    conn: &Connection,
    kind: &str,
    id: &str,
) -> Result<Option<McpDiagnosticsRecord>, AppConfigError> {
    conn.query_row(
        "SELECT readiness, health, last_error_summary, last_success_at, last_resolved_executable_path
         FROM mcp_server_diagnostics
         WHERE server_id = ?1 AND kind = ?2",
        params![id, kind],
        |row| {
            Ok(McpDiagnosticsRecord {
                readiness: row.get(0)?,
                health: row.get(1)?,
                last_error_summary: row.get(2)?,
                last_success_at: row.get(3)?,
                last_resolved_executable_path: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(AppConfigError::Database)
}

fn query_mcp_inventory_locked(
    conn: &Connection,
    kind: &str,
    id: &str,
) -> Result<Option<McpInventoryRecord>, AppConfigError> {
    conn.query_row(
        "SELECT tools_json
         FROM mcp_server_inventory
         WHERE server_id = ?1 AND kind = ?2",
        params![id, kind],
        |row| {
            Ok(McpInventoryRecord {
                tools_json: row.get(0)?,
            })
        },
    )
    .optional()
    .map_err(AppConfigError::Database)
}

fn insert_custom_mcp_server_locked(
    conn: &Connection,
    id: &str,
    request: &CreateMcpServerRequest,
    external_fingerprint: Option<&str>,
) -> Result<(), AppConfigError> {
    let safety_class = mcp::default_safety_class(request.safety_class.clone());
    let scope = mcp::default_custom_scope(request.scope.clone());
    let args_json = mcp::serialize_json(&request.args).map_err(AppConfigError::InvalidMcpServer)?;
    let env_json = mcp::serialize_json(&request.env).map_err(AppConfigError::InvalidMcpServer)?;
    let secret_env_keys_json =
        mcp::serialize_json(&request.secret_env_keys).map_err(AppConfigError::InvalidMcpServer)?;

    conn.execute(
        "INSERT INTO mcp_custom_servers (
            id, display_name, command, args_json, env_json, secret_env_keys_json,
            safety_class, scope, external_fingerprint, created_at, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        params![
            id,
            request.display_name.trim(),
            request.command.trim(),
            args_json,
            env_json,
            secret_env_keys_json,
            safety_class,
            scope,
            external_fingerprint,
        ],
    )?;
    conn.execute(
        "INSERT INTO mcp_server_state (
            server_id, kind, enabled, trusted, trust_revoked_reason, created_at, updated_at
         )
         VALUES (?1, ?2, 0, 0, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        params![id, MCP_KIND_CUSTOM],
    )?;
    conn.execute(
        "INSERT INTO mcp_server_diagnostics (
            server_id, kind, readiness, health, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)",
        params![
            id,
            MCP_KIND_CUSTOM,
            MCP_READINESS_AWAITING_FIRST_CONNECTION,
            MCP_HEALTH_NOT_CONNECTED,
        ],
    )?;
    Ok(())
}

fn update_custom_mcp_server_locked(
    conn: &Connection,
    id: &str,
    request: &UpdateMcpServerRequest,
    external_fingerprint: Option<&str>,
) -> Result<(), AppConfigError> {
    let existing = query_custom_mcp_record_locked(conn, id)?
        .ok_or_else(|| AppConfigError::McpServerNotFound(id.to_string()))?;
    let command_changed = existing.command != request.command.trim();
    let safety_class = mcp::default_safety_class(request.safety_class.clone());
    let scope = mcp::default_custom_scope(request.scope.clone());
    let args_json = mcp::serialize_json(&request.args).map_err(AppConfigError::InvalidMcpServer)?;
    let env_json = mcp::serialize_json(&request.env).map_err(AppConfigError::InvalidMcpServer)?;
    let secret_env_keys_json =
        mcp::serialize_json(&request.secret_env_keys).map_err(AppConfigError::InvalidMcpServer)?;

    conn.execute(
        "UPDATE mcp_custom_servers SET
            display_name = ?2,
            command = ?3,
            args_json = ?4,
            env_json = ?5,
            secret_env_keys_json = ?6,
            safety_class = ?7,
            scope = ?8,
            external_fingerprint = COALESCE(?9, external_fingerprint),
            updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![
            id,
            request.display_name.trim(),
            request.command.trim(),
            args_json,
            env_json,
            secret_env_keys_json,
            safety_class,
            scope,
            external_fingerprint,
        ],
    )?;

    if command_changed {
        conn.execute(
            "INSERT INTO mcp_server_state (
                server_id, kind, enabled, trusted, trust_revoked_reason, created_at, updated_at
             )
             VALUES (?1, ?2, 0, 0, 'command_changed', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
             ON CONFLICT(server_id, kind) DO UPDATE SET
                trusted = 0,
                trust_revoked_reason = 'command_changed',
                updated_at = CURRENT_TIMESTAMP",
            params![id, MCP_KIND_CUSTOM],
        )?;
        conn.execute(
            "INSERT INTO mcp_server_diagnostics (
                server_id, kind, readiness, health, last_error_summary,
                last_success_at, last_resolved_executable_path, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL, CURRENT_TIMESTAMP)
             ON CONFLICT(server_id, kind) DO UPDATE SET
                readiness = excluded.readiness,
                health = excluded.health,
                last_error_summary = NULL,
                last_success_at = NULL,
                last_resolved_executable_path = NULL,
                updated_at = CURRENT_TIMESTAMP",
            params![
                id,
                MCP_KIND_CUSTOM,
                MCP_READINESS_AWAITING_FIRST_CONNECTION,
                MCP_HEALTH_NOT_CONNECTED,
            ],
        )?;
    }

    Ok(())
}

fn persist_mcp_refresh_locked(
    conn: &Connection,
    kind: &str,
    id: &str,
    update: &McpRefreshUpdate,
) -> Result<(), AppConfigError> {
    let inventory_json =
        mcp::serialize_json(&update.inventory).map_err(AppConfigError::InvalidMcpServer)?;
    if update.last_success_at.as_deref() == Some("CURRENT_TIMESTAMP") {
        conn.execute(
            "INSERT INTO mcp_server_diagnostics (
                server_id, kind, readiness, health, last_error_summary,
                last_success_at, last_resolved_executable_path, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP, ?6, CURRENT_TIMESTAMP)
             ON CONFLICT(server_id, kind) DO UPDATE SET
                readiness = excluded.readiness,
                health = excluded.health,
                last_error_summary = excluded.last_error_summary,
                last_success_at = CURRENT_TIMESTAMP,
                last_resolved_executable_path = excluded.last_resolved_executable_path,
                updated_at = CURRENT_TIMESTAMP",
            params![
                id,
                kind,
                &update.readiness,
                &update.health,
                update.last_error_summary.as_deref(),
                update.last_resolved_executable_path.as_deref(),
            ],
        )?;
    } else {
        conn.execute(
            "INSERT INTO mcp_server_diagnostics (
                server_id, kind, readiness, health, last_error_summary,
                last_success_at, last_resolved_executable_path, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP)
             ON CONFLICT(server_id, kind) DO UPDATE SET
                readiness = excluded.readiness,
                health = excluded.health,
                last_error_summary = excluded.last_error_summary,
                last_success_at = excluded.last_success_at,
                last_resolved_executable_path = excluded.last_resolved_executable_path,
                updated_at = CURRENT_TIMESTAMP",
            params![
                id,
                kind,
                &update.readiness,
                &update.health,
                update.last_error_summary.as_deref(),
                update.last_success_at.as_deref(),
                update.last_resolved_executable_path.as_deref(),
            ],
        )?;
    }

    conn.execute(
        "INSERT INTO mcp_server_inventory (server_id, kind, tools_json, refreshed_at)
         VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
         ON CONFLICT(server_id, kind) DO UPDATE SET
            tools_json = excluded.tools_json,
            refreshed_at = CURRENT_TIMESTAMP",
        params![id, kind, inventory_json],
    )?;

    Ok(())
}

fn validate_provider(provider: &str) -> Result<(), AppConfigError> {
    if ModelRegistry::all_providers().contains(&provider) {
        Ok(())
    } else {
        Err(AppConfigError::UnsupportedProvider(provider.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_provider() {
        assert!(matches!(
            validate_provider("openrouter"),
            Err(AppConfigError::UnsupportedProvider(_))
        ));
    }

    #[test]
    fn can_get_and_set_generic_config_values() {
        let store = AppConfigStore::in_memory_for_test().unwrap();
        store
            .set_config_value("delegate_provider", "openai")
            .unwrap();
        assert_eq!(
            store
                .get_config_value("delegate_provider")
                .unwrap()
                .as_deref(),
            Some("openai")
        );
        assert_eq!(
            store.get_config_value("does_not_exist").unwrap().as_deref(),
            None
        );
    }

    #[test]
    fn workspace_serializes_with_frontend_field_names() {
        let workspace = Workspace {
            id: "ws1".to_string(),
            name: "gospel".to_string(),
            path: "/tmp/gospel".to_string(),
            session_count: 3,
            created_at: "2026-06-26 12:00:00".to_string(),
        };

        let value = serde_json::to_value(workspace).unwrap();

        assert_eq!(value["sessionCount"], 3);
        assert_eq!(value["createdAt"], "2026-06-26 12:00:00");
        assert!(value.get("session_count").is_none());
    }

    #[test]
    fn built_in_mcp_server_is_present_disabled_and_trusted() {
        let store = AppConfigStore::in_memory_for_test().unwrap();

        let servers = store.list_mcp_servers().unwrap();
        let built_in = servers
            .iter()
            .find(|server| server.id == mcp::BUILT_IN_CODEBASE_KNOWLEDGE_ID)
            .unwrap();

        assert_eq!(built_in.kind, mcp::MCP_KIND_BUILT_IN);
        assert!(!built_in.enabled);
        assert!(built_in.trusted);
        assert_eq!(built_in.safety_class, mcp::MCP_SAFETY_READ_ONLY);
        assert_eq!(built_in.inventory.len(), 4);
    }

    #[test]
    fn command_change_revokes_custom_mcp_trust() {
        let store = AppConfigStore::in_memory_for_test().unwrap();
        let created = store
            .create_mcp_server(CreateMcpServerRequest {
                display_name: "Local".to_string(),
                command: "node".to_string(),
                args: vec!["server.js".to_string()],
                env: Vec::new(),
                secret_env_keys: Vec::new(),
                safety_class: Some(mcp::MCP_SAFETY_UNKNOWN.to_string()),
                scope: None,
            })
            .unwrap();

        let trusted = store.trust_mcp_server(&created.id).unwrap();
        assert!(trusted.trusted);

        let updated = store
            .update_mcp_server(
                &created.id,
                UpdateMcpServerRequest {
                    display_name: "Local".to_string(),
                    command: "python".to_string(),
                    args: vec!["server.py".to_string()],
                    env: Vec::new(),
                    secret_env_keys: Vec::new(),
                    safety_class: Some(mcp::MCP_SAFETY_UNKNOWN.to_string()),
                    scope: None,
                },
            )
            .unwrap();

        assert!(!updated.trusted);
        assert_eq!(
            updated.trust_revoked_reason.as_deref(),
            Some("command_changed")
        );
    }

    #[test]
    fn import_preview_apply_creates_disabled_untrusted_custom_server() {
        let dir = tempfile::tempdir().unwrap();
        let import_path = dir.path().join("opencode.json");
        std::fs::write(
            &import_path,
            r#"{
              "mcp": {
                "servers": {
                  "docs": {
                    "command": "node",
                    "args": ["docs-mcp.js"],
                    "env": { "LOG_LEVEL": "info" }
                  }
                }
              }
            }"#,
        )
        .unwrap();
        let store = AppConfigStore::in_memory_for_test().unwrap();
        let preview = store
            .preview_import_mcp_servers(import_path.to_str().unwrap())
            .unwrap();
        assert_eq!(preview.servers.len(), 1);

        let result = store
            .apply_import_mcp_servers(McpApplyImportRequest {
                token: preview.token,
                selected_external_ids: vec![preview.servers[0].external_id.clone()],
                overwrite_existing: false,
            })
            .unwrap();

        assert_eq!(result.created.len(), 1);
        let servers = store.list_mcp_servers().unwrap();
        let imported = servers
            .iter()
            .find(|server| server.id == result.created[0])
            .unwrap();
        assert_eq!(imported.display_name, "docs");
        assert!(!imported.enabled);
        assert!(!imported.trusted);
        assert!(imported.external_fingerprint.is_some());
    }
}
