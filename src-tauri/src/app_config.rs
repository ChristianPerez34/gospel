use crate::models::ModelRegistry;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::PathBuf;
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
}

#[derive(Debug, Clone, Serialize)]
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

impl AppConfigStore {
    pub fn new() -> Result<Self, AppConfigError> {
        let dir = app_data_dir();
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(dir.join("app-config.sqlite3"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS provider_settings (
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
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
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
}

fn app_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("gospel")
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
        let store = AppConfigStore::new().unwrap();
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
            store
                .get_config_value("does_not_exist")
                .unwrap()
                .as_deref(),
            None
        );
    }
}
