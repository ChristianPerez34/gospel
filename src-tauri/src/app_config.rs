use crate::models::ModelRegistry;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppConfigError {
    #[error("app config IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("app config database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("provider {0} is not supported")]
    UnsupportedProvider(String),
}

pub struct AppConfigStore {
    conn: Mutex<Connection>,
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
}
