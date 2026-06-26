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
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub model: String,
    pub status: String,
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
    pub status: String,
    pub workspace_id: Option<String>,
    pub display_transcript: String,
    pub model_history: Option<String>,
    pub created_at: String,
    pub updated_at: String,
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

const SESSION_STORE_SCHEMA: &str = "PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    workspace_id TEXT,
    display_transcript TEXT NOT NULL DEFAULT '[]',
    model_history TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_sessions_workspace ON sessions(workspace_id);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_updated ON sessions(updated_at DESC);

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
CREATE INDEX IF NOT EXISTS idx_notes_unresolved ON session_notes(session_id, resolved);";

impl SessionStore {
    pub fn new() -> Result<Self, SessionStoreError> {
        let dir = app_data_dir();
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(dir.join("sessions.sqlite3"))?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self, SessionStoreError> {
        conn.execute_batch(SESSION_STORE_SCHEMA)?;
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
        let id = Uuid::new_v4().to_string();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, title, provider, model, status, workspace_id)
             VALUES (?1, ?2, ?3, ?4, 'draft', ?5)",
            params![id, title, provider, model, workspace_id],
        )?;
        Ok(SessionRecord {
            id,
            title: title.to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            status: "draft".to_string(),
            workspace_id: workspace_id.map(|s| s.to_string()),
            created_at: String::new(),
            updated_at: String::new(),
        })
    }

    pub fn get_session(&self, id: &str) -> Result<Option<SessionDetail>, SessionStoreError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, title, provider, model, status, workspace_id,
                        display_transcript, model_history, created_at, updated_at
                 FROM sessions WHERE id = ?1",
                params![id],
                |row| {
                    Ok(SessionDetail {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        provider: row.get(2)?,
                        model: row.get(3)?,
                        status: row.get(4)?,
                        workspace_id: row.get(5)?,
                        display_transcript: row.get(6)?,
                        model_history: row.get(7)?,
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
            "SELECT id, title, provider, model, status, workspace_id, created_at, updated_at
             FROM sessions
             WHERE workspace_id IS ?1 AND status != 'draft'
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map(params![workspace_id], |row| {
            Ok(SessionRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                provider: row.get(2)?,
                model: row.get(3)?,
                status: row.get(4)?,
                workspace_id: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?;
        let mut sessions = Vec::new();
        for s in rows {
            sessions.push(s?);
        }
        Ok(sessions)
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
             WHERE workspace_id IS NOT NULL AND status != 'draft'
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
            Some(s) => match (&s.workspace_id, active_workspace_id) {
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
            },
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

    #[test]
    fn create_and_get_session() {
        let store = test_store();
        let session = store
            .create_session("Test Session", "openai", "gpt-4", None)
            .unwrap();
        assert_eq!(session.title, "Test Session");
        assert_eq!(session.status, "draft");

        let detail = store.get_session(&session.id).unwrap().unwrap();
        assert_eq!(detail.id, session.id);
        assert_eq!(detail.display_transcript, "[]");
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
            .create_session("Unscoped", "openai", "gpt-4", None)
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
            .create_session("Unscoped", "openai", "gpt-4", None)
            .unwrap();

        store.update_status(&active.id, "active").unwrap();
        store.update_status(&error.id, "error").unwrap();
        store.update_status(&other.id, "active").unwrap();
        store.update_status(&unscoped.id, "active").unwrap();

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
            .create_session("Unscoped", "openai", "gpt-4", None)
            .unwrap();

        assert!(store.validate_workspace_binding(&unscoped.id, None).is_ok());
        assert!(store
            .validate_workspace_binding(&unscoped.id, Some("ws1"))
            .is_err());
    }

    #[test]
    fn delete_session_removes_record() {
        let store = test_store();
        let s = store
            .create_session("To Delete", "openai", "gpt-4", None)
            .unwrap();
        store.delete_session(&s.id).unwrap();
        assert!(store.get_session(&s.id).unwrap().is_none());
    }

    #[test]
    fn delete_session_cascades_notes() {
        let store = test_store();
        let s = store
            .create_session("With Note", "openai", "gpt-4", None)
            .unwrap();
        let note = store
            .create_note(&s.id, "verification_concern", "Check this", None)
            .unwrap();

        store.delete_session(&s.id).unwrap();

        assert!(store.note_session_id(&note.id).unwrap().is_none());
    }

    #[test]
    fn persist_turn_stores_transcript_and_history() {
        let store = test_store();
        let s = store
            .create_session("Turn Test", "openai", "gpt-4", None)
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
            .create_session("Round Trip", "openai", "gpt-4", None)
            .unwrap();

        let original: Vec<Message> = vec![
            Message::User {
                content: rig::one_or_many::OneOrMany::one(UserContent::Text(Text {
                    text: "hi".to_string(),
                })),
            },
            Message::Assistant {
                id: None,
                content: rig::one_or_many::OneOrMany::one(AssistantContent::Text(Text {
                    text: "hello back".to_string(),
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
