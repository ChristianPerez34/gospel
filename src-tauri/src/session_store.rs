use crate::session_mode::{is_valid_session_mode, normalize_session_mode, SESSION_MODE_BUILD};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum SessionStoreError {
    #[error("session store IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("session store database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("invalid session status for operation: {0}")]
    InvalidStatus(String),
    #[error("invalid session mode: {0}")]
    InvalidMode(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub model: String,
    pub variant: Option<String>,
    pub status: String,
    pub mode: String,
    pub workspace_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionDetail {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub model: String,
    pub variant: Option<String>,
    pub status: String,
    pub mode: String,
    pub workspace_id: Option<String>,
    pub display_transcript: String,
    #[serde(skip_serializing)]
    pub model_history: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchivedSessionRecord {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub model: String,
    pub variant: Option<String>,
    pub status: String,
    pub mode: String,
    pub workspace_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchivedSessionDetail {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub model: String,
    pub variant: Option<String>,
    pub status: String,
    pub mode: String,
    pub workspace_id: Option<String>,
    pub display_transcript: String,
    #[serde(skip_serializing)]
    pub model_history: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivePolicy {
    pub workspace_id: Option<String>,
    pub retention_days: i64,
    pub auto_archive_hours: i64,
    pub uses_workspace_override: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveStats {
    pub workspace_id: Option<String>,
    pub live_count: i64,
    pub archived_count: i64,
    pub expired_count: i64,
    pub archived_bytes: i64,
    pub oldest_archived_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveMaintenanceResult {
    pub archived_count: usize,
    pub deleted_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedSessionExport {
    pub version: u8,
    pub exported_at: String,
    pub sessions: Vec<ArchivedSessionExportItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedSessionExportItem {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub variant: Option<String>,
    pub status: String,
    #[serde(default = "default_session_mode")]
    pub mode: String,
    pub workspace_id: Option<String>,
    pub display_transcript: String,
    pub model_history: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: String,
    pub notes: Vec<ArchivedSessionExportNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedSessionExportNote {
    pub id: String,
    pub note_type: String,
    pub content: String,
    pub source_message_id: Option<String>,
    pub resolved: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionNote {
    pub id: String,
    pub session_id: String,
    pub note_type: String,
    pub content: String,
    pub source_message_id: Option<String>,
    pub resolved: bool,
    pub created_at: String,
}

pub struct SessionStore {
    conn: Mutex<Connection>,
}

pub struct SessionStoreState {
    pub store: Option<SessionStore>,
    pub init_warning: Option<String>,
}

fn default_session_mode() -> String {
    SESSION_MODE_BUILD.to_string()
}

const SESSION_STORE_SCHEMA: &str = "PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    variant TEXT,
    status TEXT NOT NULL DEFAULT 'draft',
    mode TEXT NOT NULL DEFAULT 'Build',
    workspace_id TEXT,
    display_transcript TEXT NOT NULL DEFAULT '[]',
    model_history TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_sessions_workspace ON sessions(workspace_id);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_updated ON sessions(updated_at DESC);

CREATE TABLE IF NOT EXISTS archived_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    variant TEXT,
    status TEXT NOT NULL,
    mode TEXT NOT NULL DEFAULT 'Build',
    workspace_id TEXT,
    display_transcript TEXT NOT NULL DEFAULT '[]',
    model_history TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_archived_sessions_workspace ON archived_sessions(workspace_id);
CREATE INDEX IF NOT EXISTS idx_archived_sessions_archived_at ON archived_sessions(archived_at DESC);

CREATE TABLE IF NOT EXISTS session_archive_policies (
    workspace_id TEXT PRIMARY KEY NOT NULL,
    retention_days INTEGER NOT NULL DEFAULT 90,
    auto_archive_hours INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS session_notes (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    note_type TEXT NOT NULL,
    content TEXT NOT NULL,
    source_message_id TEXT,
    resolved INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_notes_session ON session_notes(session_id);
CREATE INDEX IF NOT EXISTS idx_notes_unresolved ON session_notes(session_id, resolved);

CREATE TABLE IF NOT EXISTS archived_session_notes (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    note_type TEXT NOT NULL,
    content TEXT NOT NULL,
    source_message_id TEXT,
    resolved INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES archived_sessions(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_archived_notes_session ON archived_session_notes(session_id);";

impl SessionStore {
    pub fn new() -> Result<Self, SessionStoreError> {
        let dir = app_data_dir();
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(dir.join("sessions.sqlite3"))?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self, SessionStoreError> {
        conn.execute_batch(SESSION_STORE_SCHEMA)?;
        ensure_session_schema_columns(&conn)?;
        // Unscoped sessions (workspace_id IS NULL) are now first-class and must
        // survive reopening the store. Do not delete them on startup.
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    pub(crate) fn in_memory_for_test() -> Result<Self, SessionStoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    pub fn create_session(
        &self,
        title: &str,
        provider: &str,
        model: &str,
        workspace_id: Option<&str>,
    ) -> Result<SessionRecord, SessionStoreError> {
        self.create_session_with_mode(title, provider, model, workspace_id, SESSION_MODE_BUILD)
    }

    pub fn create_session_with_mode(
        &self,
        title: &str,
        provider: &str,
        model: &str,
        workspace_id: Option<&str>,
        mode: &str,
    ) -> Result<SessionRecord, SessionStoreError> {
        self.insert_session(title, provider, model, None, workspace_id, mode)
    }

    pub fn create_session_with_selection(
        &self,
        title: &str,
        provider: &str,
        model: &str,
        variant: Option<&str>,
        workspace_id: Option<&str>,
        mode: &str,
    ) -> Result<SessionRecord, SessionStoreError> {
        self.insert_session(title, provider, model, variant, workspace_id, mode)
    }

    fn insert_session(
        &self,
        title: &str,
        provider: &str,
        model: &str,
        variant: Option<&str>,
        workspace_id: Option<&str>,
        mode: &str,
    ) -> Result<SessionRecord, SessionStoreError> {
        if !is_valid_session_mode(mode) {
            return Err(SessionStoreError::InvalidMode(mode.to_string()));
        }
        let id = Uuid::new_v4().to_string();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, title, provider, model, variant, status, mode, workspace_id)
             VALUES (?1, ?2, ?3, ?4, ?5, 'draft', ?6, ?7)",
            params![id, title, provider, model, variant, mode, workspace_id],
        )?;
        Ok(SessionRecord {
            id,
            title: title.to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            variant: variant.map(|s| s.to_string()),
            status: "draft".to_string(),
            mode: mode.to_string(),
            workspace_id: workspace_id.map(|s| s.to_string()),
            created_at: String::new(),
            updated_at: String::new(),
        })
    }

    #[cfg(test)]
    fn create_unscoped_session_for_test(
        &self,
        title: &str,
        provider: &str,
        model: &str,
    ) -> Result<SessionRecord, SessionStoreError> {
        self.insert_session(title, provider, model, None, None, SESSION_MODE_BUILD)
    }

    pub fn get_session(&self, id: &str) -> Result<Option<SessionDetail>, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, title, provider, model, variant, status, mode, workspace_id,
                        display_transcript, model_history, created_at, updated_at
                 FROM sessions WHERE id = ?1",
                params![id],
                |row| {
                    Ok(SessionDetail {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        provider: row.get(2)?,
                        model: row.get(3)?,
                        variant: row.get(4)?,
                        status: row.get(5)?,
                        mode: normalize_session_mode(Some(row.get::<_, String>(6)?.as_str()))
                            .to_string(),
                        workspace_id: row.get(7)?,
                        display_transcript: row.get(8)?,
                        model_history: row.get(9)?,
                        created_at: row.get(10)?,
                        updated_at: row.get(11)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn get_session_record(&self, id: &str) -> Result<Option<SessionRecord>, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, title, provider, model, variant, status, mode, workspace_id, created_at, updated_at
                 FROM sessions WHERE id = ?1",
                params![id],
                |row| {
                    Ok(SessionRecord {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        provider: row.get(2)?,
                        model: row.get(3)?,
                        variant: row.get(4)?,
                        status: row.get(5)?,
                        mode: normalize_session_mode(Some(row.get::<_, String>(6)?.as_str()))
                            .to_string(),
                        workspace_id: row.get(7)?,
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn list_sessions_for_workspace(
        &self,
        workspace_id: Option<&str>,
    ) -> Result<Vec<SessionRecord>, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, provider, model, variant, status, mode, workspace_id, created_at, updated_at
             FROM sessions
             WHERE ((?1 IS NULL AND workspace_id IS NULL) OR workspace_id = ?1)
                AND status != 'draft'
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map(params![workspace_id], |row| {
            Ok(SessionRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                provider: row.get(2)?,
                model: row.get(3)?,
                variant: row.get(4)?,
                status: row.get(5)?,
                mode: normalize_session_mode(Some(row.get::<_, String>(6)?.as_str())).to_string(),
                workspace_id: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?;
        let mut sessions = Vec::new();
        for s in rows {
            sessions.push(s?);
        }
        Ok(sessions)
    }

    pub fn list_archived_sessions_for_workspace(
        &self,
        workspace_id: Option<&str>,
    ) -> Result<Vec<ArchivedSessionRecord>, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, provider, model, variant, status, mode, workspace_id,
                    created_at, updated_at, archived_at
             FROM archived_sessions
             WHERE deleted_at IS NULL
                AND ((?1 IS NULL AND workspace_id IS NULL) OR workspace_id = ?1)
             ORDER BY archived_at DESC",
        )?;
        let rows = stmt.query_map(params![workspace_id], |row| {
            Ok(ArchivedSessionRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                provider: row.get(2)?,
                model: row.get(3)?,
                variant: row.get(4)?,
                status: row.get(5)?,
                mode: normalize_session_mode(Some(row.get::<_, String>(6)?.as_str())).to_string(),
                workspace_id: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
                archived_at: row.get(10)?,
            })
        })?;
        let mut sessions = Vec::new();
        for s in rows {
            sessions.push(s?);
        }
        Ok(sessions)
    }

    pub fn get_archive_policy(
        &self,
        workspace_id: Option<&str>,
    ) -> Result<ArchivePolicy, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let requested_workspace_id = normalize_workspace_id(workspace_id);
        let scoped = if !requested_workspace_id.is_empty() {
            conn.query_row(
                "SELECT retention_days, auto_archive_hours
                 FROM session_archive_policies
                 WHERE workspace_id = ?1",
                params![requested_workspace_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
        } else {
            None
        };

        if let Some((retention_days, auto_archive_hours)) = scoped {
            return Ok(ArchivePolicy {
                workspace_id: workspace_id.map(str::to_string),
                retention_days,
                auto_archive_hours,
                uses_workspace_override: true,
            });
        }

        let global = conn
            .query_row(
                "SELECT retention_days, auto_archive_hours
                 FROM session_archive_policies
                 WHERE workspace_id = ''",
                params![],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .unwrap_or((90, 1));

        Ok(ArchivePolicy {
            workspace_id: workspace_id.map(str::to_string),
            retention_days: global.0,
            auto_archive_hours: global.1,
            uses_workspace_override: false,
        })
    }

    pub fn set_archive_policy(
        &self,
        workspace_id: Option<&str>,
        retention_days: i64,
        auto_archive_hours: i64,
    ) -> Result<ArchivePolicy, SessionStoreError> {
        let normalized_workspace_id = normalize_workspace_id(workspace_id);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO session_archive_policies (
                workspace_id, retention_days, auto_archive_hours, updated_at
             )
             VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(workspace_id) DO UPDATE SET
                retention_days = excluded.retention_days,
                auto_archive_hours = excluded.auto_archive_hours,
                updated_at = datetime('now')",
            params![normalized_workspace_id, retention_days, auto_archive_hours],
        )?;
        drop(conn);
        self.get_archive_policy(workspace_id)
    }

    pub fn clear_workspace_archive_policy(
        &self,
        workspace_id: &str,
    ) -> Result<ArchivePolicy, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM session_archive_policies WHERE workspace_id = ?1",
            params![workspace_id],
        )?;
        drop(conn);
        self.get_archive_policy(Some(workspace_id))
    }

    pub fn update_status(&self, id: &str, status: &str) -> Result<(), SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE sessions SET status = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![status, id],
        )?;
        if rows == 0 {
            return Err(SessionStoreError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn update_session_mode(&self, id: &str, mode: &str) -> Result<(), SessionStoreError> {
        if !is_valid_session_mode(mode) {
            return Err(SessionStoreError::InvalidMode(mode.to_string()));
        }
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE sessions SET mode = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![mode, id],
        )?;
        if rows == 0 {
            return Err(SessionStoreError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn update_model_selection(
        &self,
        id: &str,
        provider: &str,
        model: &str,
        variant: Option<&str>,
    ) -> Result<(), SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE sessions SET provider = ?1, model = ?2, variant = ?3,
                    updated_at = datetime('now')
             WHERE id = ?4",
            params![provider, model, variant, id],
        )?;
        if rows == 0 {
            return Err(SessionStoreError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn persist_turn(
        &self,
        id: &str,
        display_transcript: &str,
        model_history: Option<&str>,
    ) -> Result<(), SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE sessions SET display_transcript = ?1, model_history = ?2,
                    status = 'active', updated_at = datetime('now')
             WHERE id = ?3",
            params![display_transcript, model_history, id],
        )?;
        if rows == 0 {
            return Err(SessionStoreError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn delete_session(&self, id: &str) -> Result<(), SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        if rows == 0 {
            return Err(SessionStoreError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn archive_session(&self, id: &str) -> Result<ArchivedSessionRecord, SessionStoreError> {
        {
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.transaction()?;
            let status = tx
                .query_row(
                    "SELECT status FROM sessions WHERE id = ?1",
                    params![id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or_else(|| SessionStoreError::NotFound(id.to_string()))?;

            if status == "draft" {
                return Err(SessionStoreError::InvalidStatus(format!(
                    "draft sessions cannot be archived: {}",
                    id
                )));
            }

            tx.execute(
                "INSERT INTO archived_sessions (
                    id, title, provider, model, variant, status, mode, workspace_id,
                    display_transcript, model_history, created_at, updated_at, archived_at
                 )
                 SELECT id, title, provider, model, variant, status, mode, workspace_id,
                        display_transcript, model_history, created_at, updated_at, datetime('now')
                 FROM sessions
                 WHERE id = ?1",
                params![id],
            )?;
            tx.execute(
                "INSERT INTO archived_session_notes (
                    id, session_id, note_type, content, source_message_id, resolved, created_at
                 )
                 SELECT id, session_id, note_type, content, source_message_id, resolved, created_at
                 FROM session_notes
                 WHERE session_id = ?1",
                params![id],
            )?;
            tx.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
            tx.commit()?;
        }

        self.get_archived_session_record(id)?
            .ok_or_else(|| SessionStoreError::NotFound(id.to_string()))
    }

    pub fn restore_archived_session(&self, id: &str) -> Result<SessionRecord, SessionStoreError> {
        {
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.transaction()?;
            let exists = tx
                .query_row(
                    "SELECT 1 FROM archived_sessions WHERE id = ?1 AND deleted_at IS NULL",
                    params![id],
                    |_| Ok(()),
                )
                .optional()?;
            if exists.is_none() {
                return Err(SessionStoreError::NotFound(id.to_string()));
            }

            tx.execute(
                "INSERT INTO sessions (
                    id, title, provider, model, variant, status, mode, workspace_id,
                    display_transcript, model_history, created_at, updated_at
                 )
                 SELECT id, title, provider, model, variant, status, mode, workspace_id,
                        display_transcript, model_history, created_at, updated_at
                 FROM archived_sessions
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![id],
            )?;
            tx.execute(
                "INSERT OR REPLACE INTO session_notes (
                    id, session_id, note_type, content, source_message_id, resolved, created_at
                 )
                 SELECT id, session_id, note_type, content, source_message_id, resolved, created_at
                 FROM archived_session_notes
                 WHERE session_id = ?1",
                params![id],
            )?;
            tx.execute("DELETE FROM archived_sessions WHERE id = ?1", params![id])?;
            tx.commit()?;
        }

        self.get_session_record(id)?
            .ok_or_else(|| SessionStoreError::NotFound(id.to_string()))
    }

    pub fn delete_archived_session(&self, id: &str) -> Result<(), SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM archived_sessions WHERE id = ?1 AND deleted_at IS NULL",
            params![id],
        )?;
        if rows == 0 {
            return Err(SessionStoreError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn archive_sessions_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<ArchivedSessionRecord>, SessionStoreError> {
        let mut archived = Vec::new();
        for id in ids {
            archived.push(self.archive_session(id)?);
        }
        Ok(archived)
    }

    pub fn restore_archived_sessions_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<SessionRecord>, SessionStoreError> {
        let mut restored = Vec::new();
        for id in ids {
            restored.push(self.restore_archived_session(id)?);
        }
        Ok(restored)
    }

    pub fn delete_archived_sessions_by_ids(
        &self,
        ids: &[String],
    ) -> Result<usize, SessionStoreError> {
        let mut deleted = 0;
        for id in ids {
            self.delete_archived_session(id)?;
            deleted += 1;
        }
        Ok(deleted)
    }

    pub fn archive_sessions_older_than_hours(
        &self,
        workspace_id: Option<&str>,
        hours: i64,
    ) -> Result<Vec<ArchivedSessionRecord>, SessionStoreError> {
        if hours <= 0 {
            return Ok(Vec::new());
        }

        let ids = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT id
                 FROM sessions
                 WHERE status != 'draft'
                    AND updated_at < datetime('now', ?2)
                    AND ((?1 IS NULL AND workspace_id IS NULL) OR workspace_id = ?1)
                 ORDER BY updated_at ASC",
            )?;
            let age_modifier = format!("-{} hours", hours);
            let rows = stmt.query_map(params![workspace_id, age_modifier], |row| {
                row.get::<_, String>(0)
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        self.archive_sessions_by_ids(&ids)
    }

    pub fn delete_archived_sessions_older_than_days(
        &self,
        workspace_id: Option<&str>,
        days: i64,
    ) -> Result<usize, SessionStoreError> {
        if days <= 0 {
            return Ok(0);
        }

        let conn = self.conn.lock().unwrap();
        let age_modifier = format!("-{} days", days);
        let rows = conn.execute(
            "DELETE FROM archived_sessions
             WHERE deleted_at IS NULL
                AND archived_at < datetime('now', ?2)
                AND ((?1 IS NULL AND workspace_id IS NULL) OR workspace_id = ?1)",
            params![workspace_id, age_modifier],
        )?;
        Ok(rows)
    }

    pub fn run_archive_maintenance(
        &self,
        workspace_id: Option<&str>,
    ) -> Result<ArchiveMaintenanceResult, SessionStoreError> {
        let policy = self.get_archive_policy(workspace_id)?;
        let archived_count = self
            .archive_sessions_older_than_hours(workspace_id, policy.auto_archive_hours)?
            .len();
        let deleted_count =
            self.delete_archived_sessions_older_than_days(workspace_id, policy.retention_days)?;
        Ok(ArchiveMaintenanceResult {
            archived_count,
            deleted_count,
        })
    }

    pub fn archive_stats(
        &self,
        workspace_id: Option<&str>,
    ) -> Result<ArchiveStats, SessionStoreError> {
        let policy = self.get_archive_policy(workspace_id)?;
        let conn = self.conn.lock().unwrap();
        let live_count = conn.query_row(
            "SELECT COUNT(*)
             FROM sessions
             WHERE status != 'draft'
                AND ((?1 IS NULL AND workspace_id IS NULL) OR workspace_id = ?1)",
            params![workspace_id],
            |row| row.get::<_, i64>(0),
        )?;
        let archived_count = conn.query_row(
            "SELECT COUNT(*)
             FROM archived_sessions
             WHERE deleted_at IS NULL
                AND ((?1 IS NULL AND workspace_id IS NULL) OR workspace_id = ?1)",
            params![workspace_id],
            |row| row.get::<_, i64>(0),
        )?;
        let expired_count = conn.query_row(
            "SELECT COUNT(*)
             FROM archived_sessions
             WHERE deleted_at IS NULL
                AND archived_at < datetime('now', ?2)
                AND ((?1 IS NULL AND workspace_id IS NULL) OR workspace_id = ?1)",
            params![workspace_id, format!("-{} days", policy.retention_days)],
            |row| row.get::<_, i64>(0),
        )?;
        let (archived_bytes, oldest_archived_at) = conn.query_row(
            "SELECT COALESCE(SUM(
                    length(display_transcript)
                    + COALESCE(length(model_history), 0)
                    + length(title)
                    + length(provider)
                    + length(model)
                    + COALESCE(length(variant), 0)
                ), 0),
                    MIN(archived_at)
             FROM archived_sessions
             WHERE deleted_at IS NULL
                AND ((?1 IS NULL AND workspace_id IS NULL) OR workspace_id = ?1)",
            params![workspace_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
        )?;

        Ok(ArchiveStats {
            workspace_id: workspace_id.map(str::to_string),
            live_count,
            archived_count,
            expired_count,
            archived_bytes,
            oldest_archived_at,
        })
    }

    pub fn get_archived_session(
        &self,
        id: &str,
    ) -> Result<Option<ArchivedSessionDetail>, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, title, provider, model, variant, status, mode, workspace_id,
                        display_transcript, model_history, created_at, updated_at,
                        archived_at, deleted_at
                 FROM archived_sessions WHERE id = ?1 AND deleted_at IS NULL",
                params![id],
                |row| {
                    Ok(ArchivedSessionDetail {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        provider: row.get(2)?,
                        model: row.get(3)?,
                        variant: row.get(4)?,
                        status: row.get(5)?,
                        mode: normalize_session_mode(Some(row.get::<_, String>(6)?.as_str()))
                            .to_string(),
                        workspace_id: row.get(7)?,
                        display_transcript: row.get(8)?,
                        model_history: row.get(9)?,
                        created_at: row.get(10)?,
                        updated_at: row.get(11)?,
                        archived_at: row.get(12)?,
                        deleted_at: row.get(13)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn get_archived_session_record(
        &self,
        id: &str,
    ) -> Result<Option<ArchivedSessionRecord>, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, title, provider, model, variant, status, mode, workspace_id,
                        created_at, updated_at, archived_at
                 FROM archived_sessions WHERE id = ?1 AND deleted_at IS NULL",
                params![id],
                |row| {
                    Ok(ArchivedSessionRecord {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        provider: row.get(2)?,
                        model: row.get(3)?,
                        variant: row.get(4)?,
                        status: row.get(5)?,
                        mode: normalize_session_mode(Some(row.get::<_, String>(6)?.as_str()))
                            .to_string(),
                        workspace_id: row.get(7)?,
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                        archived_at: row.get(10)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn export_archived_sessions(&self, ids: &[String]) -> Result<String, SessionStoreError> {
        let mut sessions = Vec::new();
        for id in ids {
            let detail = self
                .get_archived_session(id)?
                .ok_or_else(|| SessionStoreError::NotFound(id.to_string()))?;
            sessions.push(ArchivedSessionExportItem {
                id: detail.id,
                title: detail.title,
                provider: detail.provider,
                model: detail.model,
                variant: detail.variant,
                status: detail.status,
                mode: detail.mode,
                workspace_id: detail.workspace_id,
                display_transcript: detail.display_transcript,
                model_history: detail.model_history,
                created_at: detail.created_at,
                updated_at: detail.updated_at,
                archived_at: detail.archived_at,
                notes: self.list_archived_notes_for_export(id)?,
            });
        }

        let exported_at = {
            let conn = self.conn.lock().unwrap();
            conn.query_row("SELECT datetime('now')", params![], |row| {
                row.get::<_, String>(0)
            })?
        };
        let payload = ArchivedSessionExport {
            version: 1,
            exported_at,
            sessions,
        };
        serde_json::to_string_pretty(&payload)
            .map_err(|e| SessionStoreError::InvalidStatus(e.to_string()))
    }

    pub fn import_archived_sessions(
        &self,
        payload: &str,
        workspace_id_override: Option<&str>,
    ) -> Result<Vec<ArchivedSessionRecord>, SessionStoreError> {
        let export: ArchivedSessionExport = serde_json::from_str(payload)
            .map_err(|e| SessionStoreError::InvalidStatus(e.to_string()))?;
        let mut imported_ids = Vec::new();
        {
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.transaction()?;
            for session in export.sessions {
                let live_exists = tx
                    .query_row(
                        "SELECT 1 FROM sessions WHERE id = ?1",
                        params![session.id],
                        |_| Ok(()),
                    )
                    .optional()?;
                if live_exists.is_some() {
                    return Err(SessionStoreError::InvalidStatus(format!(
                        "session already exists outside archive: {}",
                        session.id
                    )));
                }

                let target_workspace_id = workspace_id_override
                    .map(str::to_string)
                    .or_else(|| session.workspace_id.clone());
                tx.execute(
                    "INSERT INTO archived_sessions (
                        id, title, provider, model, variant, status, mode, workspace_id,
                        display_transcript, model_history, created_at, updated_at,
                        archived_at, deleted_at
                     )
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL)
                     ON CONFLICT(id) DO UPDATE SET
                        title = excluded.title,
                        provider = excluded.provider,
                        model = excluded.model,
                        variant = excluded.variant,
                        status = excluded.status,
                        mode = excluded.mode,
                        workspace_id = excluded.workspace_id,
                        display_transcript = excluded.display_transcript,
                        model_history = excluded.model_history,
                        created_at = excluded.created_at,
                        updated_at = excluded.updated_at,
                        archived_at = excluded.archived_at,
                        deleted_at = NULL",
                    params![
                        session.id,
                        session.title,
                        session.provider,
                        session.model,
                        session.variant,
                        session.status,
                        normalize_session_mode(Some(session.mode.as_str())),
                        target_workspace_id,
                        session.display_transcript,
                        session.model_history,
                        session.created_at,
                        session.updated_at,
                        session.archived_at
                    ],
                )?;
                tx.execute(
                    "DELETE FROM archived_session_notes WHERE session_id = ?1",
                    params![session.id],
                )?;
                for note in session.notes {
                    tx.execute(
                        "INSERT INTO archived_session_notes (
                            id, session_id, note_type, content,
                            source_message_id, resolved, created_at
                         )
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            note.id,
                            session.id,
                            note.note_type,
                            note.content,
                            note.source_message_id,
                            if note.resolved { 1 } else { 0 },
                            note.created_at
                        ],
                    )?;
                }
                imported_ids.push(session.id);
            }
            tx.commit()?;
        }

        let mut imported = Vec::new();
        for id in imported_ids {
            imported.push(
                self.get_archived_session_record(&id)?
                    .ok_or_else(|| SessionStoreError::NotFound(id.to_string()))?,
            );
        }
        Ok(imported)
    }

    fn list_archived_notes_for_export(
        &self,
        session_id: &str,
    ) -> Result<Vec<ArchivedSessionExportNote>, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, note_type, content, source_message_id, resolved, created_at
             FROM archived_session_notes
             WHERE session_id = ?1
             ORDER BY created_at ASC",
        )?;
        let notes = stmt
            .query_map(params![session_id], |row| {
                Ok(ArchivedSessionExportNote {
                    id: row.get(0)?,
                    note_type: row.get(1)?,
                    content: row.get(2)?,
                    source_message_id: row.get(3)?,
                    resolved: row.get::<_, i32>(4)? != 0,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(notes)
    }

    pub fn clean_stale_drafts(&self) -> Result<usize, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM sessions WHERE status = 'draft' AND display_transcript = '[]'
             AND updated_at < datetime('now', '-1 hour')",
            params![],
        )?;
        Ok(rows)
    }

    pub fn workspace_session_count(&self, workspace_id: &str) -> Result<i64, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE workspace_id = ?1 AND status != 'draft'",
            params![workspace_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn workspace_session_counts(&self) -> Result<HashMap<String, i64>, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT workspace_id, COUNT(*)
             FROM sessions
             WHERE workspace_id IS NOT NULL AND workspace_id != '' AND status != 'draft'
             GROUP BY workspace_id",
        )?;
        let rows = stmt.query_map(params![], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut counts = HashMap::new();
        for row in rows {
            let (workspace_id, count) = row?;
            counts.insert(workspace_id, count);
        }
        Ok(counts)
    }

    pub fn validate_workspace_binding(
        &self,
        session_id: &str,
        active_workspace_id: Option<&str>,
    ) -> Result<(), SessionStoreError> {
        let session = self.get_session(session_id)?;
        match session {
            Some(s) => validate_workspace_binding_parts(
                session_id,
                s.workspace_id.as_deref(),
                active_workspace_id,
            ),
            None => Err(SessionStoreError::NotFound(session_id.to_string())),
        }
    }

    pub fn validate_archived_workspace_binding(
        &self,
        session_id: &str,
        active_workspace_id: Option<&str>,
    ) -> Result<(), SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let workspace_id = conn
            .query_row(
                "SELECT workspace_id FROM archived_sessions WHERE id = ?1 AND deleted_at IS NULL",
                params![session_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;
        match workspace_id {
            Some(workspace_id) => validate_workspace_binding_parts(
                session_id,
                workspace_id.as_deref(),
                active_workspace_id,
            ),
            None => Err(SessionStoreError::NotFound(session_id.to_string())),
        }
    }

    pub fn create_note(
        &self,
        session_id: &str,
        note_type: &str,
        content: &str,
        source_message_id: Option<&str>,
    ) -> Result<SessionNote, SessionStoreError> {
        let id = Uuid::new_v4().to_string();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO session_notes (id, session_id, note_type, content, source_message_id)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, session_id, note_type, content, source_message_id],
        )?;
        Ok(SessionNote {
            id,
            session_id: session_id.to_string(),
            note_type: note_type.to_string(),
            content: content.to_string(),
            source_message_id: source_message_id.map(|s| s.to_string()),
            resolved: false,
            created_at: String::new(),
        })
    }

    pub fn list_unresolved_notes(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionNote>, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, note_type, content, source_message_id, resolved, created_at
             FROM session_notes
             WHERE session_id = ?1 AND resolved = 0
             ORDER BY created_at ASC",
        )?;
        let notes = stmt
            .query_map(params![session_id], |row| {
                Ok(SessionNote {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    note_type: row.get(2)?,
                    content: row.get(3)?,
                    source_message_id: row.get(4)?,
                    resolved: row.get::<_, i32>(5)? != 0,
                    created_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(notes)
    }

    pub fn resolve_note(&self, note_id: &str) -> Result<(), SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE session_notes SET resolved = 1 WHERE id = ?1",
            params![note_id],
        )?;
        Ok(())
    }

    pub fn note_session_id(&self, note_id: &str) -> Result<Option<String>, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let session_id = conn
            .query_row(
                "SELECT session_id FROM session_notes WHERE id = ?1",
                params![note_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(session_id)
    }

    pub fn resolve_notes_for_message(
        &self,
        session_id: &str,
        source_message_id: &str,
    ) -> Result<(), SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE session_notes SET resolved = 1
             WHERE session_id = ?1 AND source_message_id = ?2",
            params![session_id, source_message_id],
        )?;
        Ok(())
    }
}

fn ensure_session_schema_columns(conn: &Connection) -> Result<(), SessionStoreError> {
    ensure_column(
        conn,
        "sessions",
        "mode",
        "ALTER TABLE sessions ADD COLUMN mode TEXT NOT NULL DEFAULT 'Build'",
    )?;
    ensure_column(
        conn,
        "archived_sessions",
        "mode",
        "ALTER TABLE archived_sessions ADD COLUMN mode TEXT NOT NULL DEFAULT 'Build'",
    )?;
    ensure_column(
        conn,
        "sessions",
        "variant",
        "ALTER TABLE sessions ADD COLUMN variant TEXT",
    )?;
    ensure_column(
        conn,
        "archived_sessions",
        "variant",
        "ALTER TABLE archived_sessions ADD COLUMN variant TEXT",
    )?;
    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    alter_sql: &str,
) -> Result<(), SessionStoreError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let columns = stmt.query_map(params![], |row| row.get::<_, String>(1))?;
    for existing in columns {
        if existing? == column {
            return Ok(());
        }
    }
    conn.execute(alter_sql, params![])?;
    Ok(())
}

fn normalize_workspace_id(workspace_id: Option<&str>) -> String {
    workspace_id.unwrap_or("").to_string()
}

fn validate_workspace_binding_parts(
    session_id: &str,
    session_workspace_id: Option<&str>,
    active_workspace_id: Option<&str>,
) -> Result<(), SessionStoreError> {
    match (session_workspace_id, active_workspace_id) {
        (Some(session_ws), Some(active_ws)) if session_ws == active_ws => Ok(()),
        (Some(session_ws), Some(active_ws)) => Err(SessionStoreError::NotFound(format!(
            "Session {} belongs to workspace {}, but active workspace is {}",
            session_id, session_ws, active_ws
        ))),
        (Some(_), None) => Err(SessionStoreError::NotFound(format!(
            "Session {} is workspace-bound but no workspace is active",
            session_id
        ))),
        (None, None) => Ok(()),
        (None, Some(active_ws)) => Err(SessionStoreError::NotFound(format!(
            "Session {} is unscoped, but active workspace is {}",
            session_id, active_ws
        ))),
    }
}

fn app_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("gospel")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> SessionStore {
        SessionStore::in_memory_for_test().unwrap()
    }

    fn set_session_updated_at(store: &SessionStore, id: &str, modifier: &str) {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET updated_at = datetime('now', ?2) WHERE id = ?1",
            params![id, modifier],
        )
        .unwrap();
    }

    fn set_archived_at(store: &SessionStore, id: &str, modifier: &str) {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "UPDATE archived_sessions SET archived_at = datetime('now', ?2) WHERE id = ?1",
            params![id, modifier],
        )
        .unwrap();
    }

    #[test]
    fn create_and_get_session() {
        let store = test_store();
        let session = store
            .create_session("Test Session", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        assert_eq!(session.title, "Test Session");
        assert_eq!(session.status, "draft");
        assert_eq!(session.mode, "Build");
        assert_eq!(session.variant, None);
        assert_eq!(session.workspace_id.as_deref(), Some("ws1"));

        let detail = store.get_session(&session.id).unwrap().unwrap();
        assert_eq!(detail.id, session.id);
        assert_eq!(detail.mode, "Build");
        assert_eq!(detail.variant, None);
        assert_eq!(detail.workspace_id.as_deref(), Some("ws1"));
        assert_eq!(detail.display_transcript, "[]");
    }

    #[test]
    fn model_variant_round_trips_through_get_list_and_update() {
        let store = test_store();
        let session = store
            .create_session_with_selection(
                "Reasoning",
                "openai",
                "gpt-5.2",
                Some("reasoning-high"),
                Some("ws1"),
                SESSION_MODE_BUILD,
            )
            .unwrap();
        store.update_status(&session.id, "active").unwrap();

        let detail = store.get_session(&session.id).unwrap().unwrap();
        assert_eq!(detail.variant.as_deref(), Some("reasoning-high"));

        let listed = store.list_sessions_for_workspace(Some("ws1")).unwrap();
        assert_eq!(listed[0].variant.as_deref(), Some("reasoning-high"));

        store
            .update_model_selection(&session.id, "openai", "gpt-5.2", Some("reasoning-low"))
            .unwrap();
        let updated = store.get_session(&session.id).unwrap().unwrap();
        assert_eq!(updated.provider, "openai");
        assert_eq!(updated.model, "gpt-5.2");
        assert_eq!(updated.variant.as_deref(), Some("reasoning-low"));
    }

    #[test]
    fn update_session_mode_round_trips_through_get_and_list() {
        let store = test_store();
        let session = store
            .create_session("Read only", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        store.update_session_mode(&session.id, "ReadOnly").unwrap();
        store.update_status(&session.id, "active").unwrap();

        let detail = store.get_session(&session.id).unwrap().unwrap();
        assert_eq!(detail.mode, "ReadOnly");

        let listed = store.list_sessions_for_workspace(Some("ws1")).unwrap();
        assert_eq!(listed[0].mode, "ReadOnly");
    }

    #[test]
    fn from_connection_preserves_unscoped_sessions() {
        // Unscoped sessions (workspace_id IS NULL) are first-class and must NOT
        // be deleted when the store is reopened.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SESSION_STORE_SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, title, provider, model, status, workspace_id)
             VALUES (?1, ?2, ?3, ?4, 'active', ?5)",
            params![
                "orphan-session",
                "Orphan",
                "openai",
                "gpt-4",
                Option::<&str>::None
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, title, provider, model, status, workspace_id)
             VALUES (?1, ?2, ?3, ?4, 'active', ?5)",
            params!["workspace-session", "Scoped", "openai", "gpt-4", "ws1"],
        )
        .unwrap();

        let store = SessionStore::from_connection(conn).unwrap();

        assert!(
            store.get_session("orphan-session").unwrap().is_some(),
            "unscoped sessions must survive from_connection"
        );
        assert!(store.get_session("workspace-session").unwrap().is_some());
        assert_eq!(
            store
                .get_session("orphan-session")
                .unwrap()
                .unwrap()
                .workspace_id,
            None
        );
    }

    #[test]
    fn list_filters_drafts() {
        let store = test_store();
        store
            .create_session("Draft", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        let s = store
            .create_session("Active", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        store.update_status(&s.id, "active").unwrap();

        let sessions = store.list_sessions_for_workspace(Some("ws1")).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "Active");
    }

    #[test]
    fn list_workspace_excludes_unscoped_sessions() {
        let store = test_store();
        let unscoped = store
            .create_unscoped_session_for_test("Unscoped", "openai", "gpt-4")
            .unwrap();
        let scoped = store
            .create_session("Scoped", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        let other = store
            .create_session("Other", "openai", "gpt-4", Some("ws2"))
            .unwrap();

        store.update_status(&unscoped.id, "active").unwrap();
        store.update_status(&scoped.id, "active").unwrap();
        store.update_status(&other.id, "active").unwrap();

        let titles: Vec<_> = store
            .list_sessions_for_workspace(Some("ws1"))
            .unwrap()
            .into_iter()
            .map(|session| session.title)
            .collect();

        assert!(!titles.contains(&"Unscoped".to_string()));
        assert!(titles.contains(&"Scoped".to_string()));
        assert!(!titles.contains(&"Other".to_string()));

        let unscoped_titles: Vec<_> = store
            .list_sessions_for_workspace(None)
            .unwrap()
            .into_iter()
            .map(|session| session.title)
            .collect();
        assert!(unscoped_titles.contains(&"Unscoped".to_string()));
        assert!(!unscoped_titles.contains(&"Scoped".to_string()));
    }

    #[test]
    fn workspace_session_counts_include_only_non_draft_workspace_sessions() {
        let store = test_store();
        let draft = store
            .create_session("Draft", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        let active = store
            .create_session("Active", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        let error = store
            .create_session("Error", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        let other = store
            .create_session("Other", "openai", "gpt-4", Some("ws2"))
            .unwrap();
        let unscoped = store
            .create_unscoped_session_for_test("Unscoped", "openai", "gpt-4")
            .unwrap();
        let empty_ws = store
            .create_session("EmptyWs", "openai", "gpt-4", Some(""))
            .unwrap();

        store.update_status(&active.id, "active").unwrap();
        store.update_status(&error.id, "error").unwrap();
        store.update_status(&other.id, "active").unwrap();
        store.update_status(&unscoped.id, "active").unwrap();
        store.update_status(&empty_ws.id, "active").unwrap();

        let counts = store.workspace_session_counts().unwrap();

        assert_eq!(counts.get("ws1"), Some(&2));
        assert_eq!(counts.get("ws2"), Some(&1));
        assert!(counts.get("ws3").is_none());
        assert!(counts.get("").is_none());
        assert_eq!(store.workspace_session_count("ws1").unwrap(), 2);
        assert_eq!(
            store.get_session(&draft.id).unwrap().unwrap().status,
            "draft"
        );
    }

    #[test]
    fn unscoped_sessions_cannot_continue_inside_workspace() {
        let store = test_store();
        let unscoped = store
            .create_unscoped_session_for_test("Unscoped", "openai", "gpt-4")
            .unwrap();

        assert!(store.validate_workspace_binding(&unscoped.id, None).is_ok());
        assert!(store
            .validate_workspace_binding(&unscoped.id, Some("ws1"))
            .is_err());
    }

    #[test]
    fn unscoped_sessions_survive_store_reopen() {
        // Regression: `from_connection` used to run
        // `DELETE FROM sessions WHERE workspace_id IS NULL`, silently dropping
        // unscoped sessions on the next app restart.
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("sessions.sqlite3");

        let store = SessionStore::from_connection(rusqlite::Connection::open(&db_path).unwrap())
            .unwrap();
        let unscoped = store
            .create_unscoped_session_for_test("Unscoped", "openai", "gpt-4")
            .unwrap();
        let scoped = store
            .create_session("Scoped", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        drop(store);

        let reopened =
            SessionStore::from_connection(rusqlite::Connection::open(&db_path).unwrap()).unwrap();
        assert!(
            reopened.get_session(&unscoped.id).unwrap().is_some(),
            "unscoped session must survive reopening the store"
        );
        assert!(
            reopened.get_session(&scoped.id).unwrap().is_some(),
            "scoped session must survive reopening the store"
        );
        assert_eq!(
            reopened
                .get_session(&unscoped.id)
                .unwrap()
                .unwrap()
                .workspace_id,
            None
        );
    }

    #[test]
    fn delete_session_removes_record() {
        let store = test_store();
        let s = store
            .create_session("To Delete", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        store.delete_session(&s.id).unwrap();
        assert!(store.get_session(&s.id).unwrap().is_none());
    }

    #[test]
    fn delete_session_cascades_notes() {
        let store = test_store();
        let s = store
            .create_session("With Note", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        let note = store
            .create_note(&s.id, "verification_concern", "Check this", None)
            .unwrap();

        store.delete_session(&s.id).unwrap();

        assert!(store.note_session_id(&note.id).unwrap().is_none());
    }

    #[test]
    fn archive_session_moves_payload_and_removes_live_record() {
        let store = test_store();
        let s = store
            .create_session("Archive Me", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        store
            .persist_turn(
                &s.id,
                "[{\"role\":\"user\",\"content\":\"hi\"}]",
                Some("[{\"role\":\"user\",\"content\":\"hi\"}]"),
            )
            .unwrap();

        let archived = store.archive_session(&s.id).unwrap();

        assert_eq!(archived.id, s.id);
        assert_eq!(archived.workspace_id.as_deref(), Some("ws1"));
        assert!(store.get_session(&s.id).unwrap().is_none());

        let archived_detail = store.get_archived_session(&s.id).unwrap().unwrap();
        assert_eq!(
            archived_detail.display_transcript,
            "[{\"role\":\"user\",\"content\":\"hi\"}]"
        );
        assert_eq!(
            archived_detail.model_history,
            Some("[{\"role\":\"user\",\"content\":\"hi\"}]".to_string())
        );
        assert_eq!(store.workspace_session_count("ws1").unwrap(), 0);
    }

    #[test]
    fn archive_rejects_draft_sessions() {
        let store = test_store();
        let s = store
            .create_session("Draft", "openai", "gpt-4", Some("ws1"))
            .unwrap();

        let err = store.archive_session(&s.id).unwrap_err();

        assert!(matches!(err, SessionStoreError::InvalidStatus(_)));
        assert!(store.get_session(&s.id).unwrap().is_some());
    }

    #[test]
    fn list_archived_sessions_filters_workspace() {
        let store = test_store();
        let first = store
            .create_session("First", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        let second = store
            .create_session("Second", "openai", "gpt-4", Some("ws2"))
            .unwrap();
        store.update_status(&first.id, "active").unwrap();
        store.update_status(&second.id, "active").unwrap();
        store.archive_session(&first.id).unwrap();
        store.archive_session(&second.id).unwrap();

        let archived = store
            .list_archived_sessions_for_workspace(Some("ws1"))
            .unwrap();

        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].title, "First");
    }

    #[test]
    fn restore_archived_session_round_trips_notes() {
        let store = test_store();
        let s = store
            .create_session("With Note", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        store
            .persist_turn(
                &s.id,
                "[{\"role\":\"user\"}]",
                Some("[{\"role\":\"user\"}]"),
            )
            .unwrap();
        let note = store
            .create_note(&s.id, "verification_concern", "Check this", None)
            .unwrap();
        store.archive_session(&s.id).unwrap();

        assert!(store.note_session_id(&note.id).unwrap().is_none());

        let restored = store.restore_archived_session(&s.id).unwrap();

        assert_eq!(restored.id, s.id);
        assert!(store.get_archived_session(&s.id).unwrap().is_none());
        assert!(store.get_session(&s.id).unwrap().is_some());
        assert_eq!(
            store.note_session_id(&note.id).unwrap().as_deref(),
            Some(s.id.as_str())
        );
    }

    #[test]
    fn delete_archived_session_removes_archive_record() {
        let store = test_store();
        let s = store
            .create_session("Archive Me", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        store.update_status(&s.id, "active").unwrap();
        store.archive_session(&s.id).unwrap();

        store.delete_archived_session(&s.id).unwrap();

        assert!(store.get_archived_session(&s.id).unwrap().is_none());
    }

    #[test]
    fn archive_policy_defaults_and_workspace_overrides() {
        let store = test_store();

        let default_policy = store.get_archive_policy(Some("ws1")).unwrap();
        assert_eq!(default_policy.retention_days, 90);
        assert_eq!(default_policy.auto_archive_hours, 1);
        assert!(!default_policy.uses_workspace_override);

        store.set_archive_policy(None, 30, 24).unwrap();
        let global_policy = store.get_archive_policy(Some("ws1")).unwrap();
        assert_eq!(global_policy.retention_days, 30);
        assert_eq!(global_policy.auto_archive_hours, 24);
        assert!(!global_policy.uses_workspace_override);

        store.set_archive_policy(Some("ws1"), 7, 1).unwrap();
        let scoped_policy = store.get_archive_policy(Some("ws1")).unwrap();
        assert_eq!(scoped_policy.retention_days, 7);
        assert_eq!(scoped_policy.auto_archive_hours, 1);
        assert!(scoped_policy.uses_workspace_override);

        let cleared = store.clear_workspace_archive_policy("ws1").unwrap();
        assert_eq!(cleared.retention_days, 30);
        assert!(!cleared.uses_workspace_override);
    }

    #[test]
    fn archive_maintenance_archives_old_live_and_deletes_expired_archives() {
        let store = test_store();
        store.set_archive_policy(Some("ws1"), 7, 1).unwrap();
        let old = store
            .create_session("Old", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        let recent = store
            .create_session("Recent", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        store.update_status(&old.id, "active").unwrap();
        store.update_status(&recent.id, "active").unwrap();
        set_session_updated_at(&store, &old.id, "-2 hours");

        let result = store.run_archive_maintenance(Some("ws1")).unwrap();

        assert_eq!(result.archived_count, 1);
        assert_eq!(result.deleted_count, 0);
        assert!(store.get_archived_session(&old.id).unwrap().is_some());
        assert!(store.get_session(&recent.id).unwrap().is_some());

        set_archived_at(&store, &old.id, "-8 days");
        let stats = store.archive_stats(Some("ws1")).unwrap();
        assert_eq!(stats.archived_count, 1);
        assert_eq!(stats.expired_count, 1);

        let cleanup = store.run_archive_maintenance(Some("ws1")).unwrap();
        assert_eq!(cleanup.deleted_count, 1);
        assert!(store.get_archived_session(&old.id).unwrap().is_none());
    }

    #[test]
    fn bulk_restore_and_delete_archived_sessions() {
        let store = test_store();
        let first = store
            .create_session("First", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        let second = store
            .create_session("Second", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        store.update_status(&first.id, "active").unwrap();
        store.update_status(&second.id, "active").unwrap();
        let archived = store
            .archive_sessions_by_ids(&[first.id.clone(), second.id.clone()])
            .unwrap();
        assert_eq!(archived.len(), 2);

        let restored = store
            .restore_archived_sessions_by_ids(&[first.id.clone()])
            .unwrap();
        assert_eq!(restored.len(), 1);
        assert!(store.get_session(&first.id).unwrap().is_some());

        let deleted = store
            .delete_archived_sessions_by_ids(&[second.id.clone()])
            .unwrap();
        assert_eq!(deleted, 1);
        assert!(store.get_archived_session(&second.id).unwrap().is_none());
    }

    #[test]
    fn export_import_archived_sessions_round_trips_notes() {
        let store = test_store();
        let s = store
            .create_session_with_selection(
                "Portable",
                "openai",
                "gpt-5.2",
                Some("reasoning-high"),
                Some("ws1"),
                SESSION_MODE_BUILD,
            )
            .unwrap();
        store
            .persist_turn(
                &s.id,
                "[{\"role\":\"user\"}]",
                Some("[{\"role\":\"user\"}]"),
            )
            .unwrap();
        let note = store
            .create_note(&s.id, "verification_concern", "Check this", None)
            .unwrap();
        store.archive_session(&s.id).unwrap();

        let export = store.export_archived_sessions(&[s.id.clone()]).unwrap();
        store.delete_archived_session(&s.id).unwrap();
        let imported = store
            .import_archived_sessions(&export, Some("ws2"))
            .unwrap();

        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].workspace_id.as_deref(), Some("ws2"));
        assert_eq!(imported[0].variant.as_deref(), Some("reasoning-high"));
        let restored = store.restore_archived_session(&s.id).unwrap();
        assert_eq!(restored.workspace_id.as_deref(), Some("ws2"));
        assert_eq!(restored.variant.as_deref(), Some("reasoning-high"));
        assert_eq!(
            store.note_session_id(&note.id).unwrap().as_deref(),
            Some(s.id.as_str())
        );
    }

    #[test]
    fn persist_turn_stores_transcript_and_history() {
        let store = test_store();
        let s = store
            .create_session("Turn Test", "openai", "gpt-4", Some("ws1"))
            .unwrap();
        store
            .persist_turn(
                &s.id,
                "[{\"role\":\"user\"}]",
                Some("[{\"role\":\"user\"}]"),
            )
            .unwrap();

        let detail = store.get_session(&s.id).unwrap().unwrap();
        assert_eq!(detail.display_transcript, "[{\"role\":\"user\"}]");
        assert_eq!(
            detail.model_history,
            Some("[{\"role\":\"user\"}]".to_string())
        );
        assert_eq!(detail.status, "active");
    }

    #[test]
    fn model_history_round_trips_into_rig_messages() {
        use rig::completion::message::{AssistantContent, Message, Text, UserContent};

        let store = test_store();
        let s = store
            .create_session("Round Trip", "openai", "gpt-4", Some("ws1"))
            .unwrap();

        let original: Vec<Message> = vec![
            Message::User {
                content: rig::one_or_many::OneOrMany::one(UserContent::Text(Text {
                    text: "hi".to_string(),
                    additional_params: Some(serde_json::json!({})),
                })),
            },
            Message::Assistant {
                id: None,
                content: rig::one_or_many::OneOrMany::one(AssistantContent::Text(Text {
                    text: "hello back".to_string(),
                    additional_params: Some(serde_json::json!({})),
                })),
            },
        ];
        let history_json = serde_json::to_string(&original).unwrap();
        store
            .persist_turn(&s.id, "[]", Some(&history_json))
            .unwrap();

        let detail = store.get_session(&s.id).unwrap().unwrap();
        let restored: Vec<Message> = serde_json::from_str(
            detail
                .model_history
                .as_deref()
                .expect("model_history stored"),
        )
        .expect("model_history must deserialize into Vec<Message>");

        assert_eq!(restored, original);
    }
}
