//! Per-MCP-server secret storage: secret env values, secret header values, and
//! OAuth credentials. Mirrors `keychain.rs`'s file-based model — one JSON file
//! under the Gospel config dir, written atomically with 0600 permissions.
//!
//! The MCP server definition in SQLite stores only secret *key names*; values
//! live here so the config database never holds bearer tokens.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum McpCredentialError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct McpServerSecrets {
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub oauth: Option<rmcp::transport::auth::StoredCredentials>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CredentialFile {
    #[serde(default)]
    servers: BTreeMap<String, McpServerSecrets>,
}

fn credentials_file_path() -> PathBuf {
    crate::keychain::gospel_config_dir().join("mcp-credentials.json")
}

fn read_file() -> Result<CredentialFile, McpCredentialError> {
    match std::fs::File::open(credentials_file_path()) {
        Ok(file) => Ok(serde_json::from_reader(file)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(CredentialFile::default()),
        Err(e) => Err(McpCredentialError::Io(e)),
    }
}

fn write_file(file: &CredentialFile) -> Result<(), McpCredentialError> {
    let dir = crate::keychain::gospel_config_dir();
    std::fs::create_dir_all(&dir)?;
    let mut temp = tempfile::Builder::new()
        .prefix(".mcp-credentials-")
        .suffix(".tmp")
        .tempfile_in(&dir)?;
    serde_json::to_writer_pretty(&mut temp, file)?;
    temp.persist(credentials_file_path()).map_err(|e| e.error)?;
    Ok(())
}

pub fn load_secrets(server_id: &str) -> Result<McpServerSecrets, McpCredentialError> {
    Ok(read_file()?
        .servers
        .get(server_id)
        .cloned()
        .unwrap_or_default())
}

/// Reconcile stored values with a form submission: non-empty values are
/// stored, declared keys submitted with an empty value keep any previously
/// stored value, and keys no longer declared are deleted.
pub fn sync_secret_values(
    server_id: &str,
    kind: &str,
    declared: &[(String, String)],
) -> Result<(), McpCredentialError> {
    let mut file = read_file()?;
    let entry = file.servers.entry(server_id.to_string()).or_default();
    let target = match kind {
        "env" => &mut entry.env,
        "headers" => &mut entry.headers,
        other => {
            return Err(McpCredentialError::Json(serde::de::Error::custom(format!(
                "unknown secret kind: {other}"
            ))))
        }
    };
    let previous = std::mem::take(target);
    for (key, value) in declared {
        if !value.is_empty() {
            target.insert(key.clone(), value.clone());
        } else if let Some(existing) = previous.get(key) {
            target.insert(key.clone(), existing.clone());
        }
    }
    write_file(&file)
}

pub fn delete_secrets(server_id: &str) -> Result<(), McpCredentialError> {
    let mut file = read_file()?;
    if file.servers.remove(server_id).is_some() {
        write_file(&file)?;
    }
    Ok(())
}

/// Declared secret keys with no stored value — surfaced in the UI so the user
/// knows what still needs to be filled in.
pub fn missing_keys(server_id: &str, kind: &str, declared: &[String]) -> Vec<String> {
    let Ok(secrets) = load_secrets(server_id) else {
        return declared.to_vec();
    };
    let stored = match kind {
        "env" => &secrets.env,
        _ => &secrets.headers,
    };
    declared
        .iter()
        .filter(|key| !stored.contains_key(*key))
        .cloned()
        .collect()
}

/// rmcp `CredentialStore` backed by this file. The AuthorizationManager calls
/// `save` after token exchange and `load` on `initialize_from_store`, so OAuth
/// tokens persist exactly like the manual secret values.
pub struct FileCredentialStore {
    server_id: String,
}

impl FileCredentialStore {
    pub fn new(server_id: impl Into<String>) -> Self {
        Self {
            server_id: server_id.into(),
        }
    }
}

#[async_trait::async_trait]
impl rmcp::transport::auth::CredentialStore for FileCredentialStore {
    async fn load(
        &self,
    ) -> Result<Option<rmcp::transport::auth::StoredCredentials>, rmcp::transport::auth::AuthError>
    {
        load_secrets(&self.server_id)
            .map(|secrets| secrets.oauth)
            .map_err(|e| rmcp::transport::auth::AuthError::InternalError(e.to_string()))
    }

    async fn save(
        &self,
        credentials: rmcp::transport::auth::StoredCredentials,
    ) -> Result<(), rmcp::transport::auth::AuthError> {
        let mut file = read_file()
            .map_err(|e| rmcp::transport::auth::AuthError::InternalError(e.to_string()))?;
        file.servers
            .entry(self.server_id.clone())
            .or_default()
            .oauth = Some(credentials);
        write_file(&file)
            .map_err(|e| rmcp::transport::auth::AuthError::InternalError(e.to_string()))
    }

    async fn clear(&self) -> Result<(), rmcp::transport::auth::AuthError> {
        let mut file = read_file()
            .map_err(|e| rmcp::transport::auth::AuthError::InternalError(e.to_string()))?;
        if let Some(entry) = file.servers.get_mut(&self.server_id) {
            entry.oauth = None;
            write_file(&file)
                .map_err(|e| rmcp::transport::auth::AuthError::InternalError(e.to_string()))?;
        }
        Ok(())
    }
}

pub fn store_oauth_credentials(
    server_id: &str,
    credentials: rmcp::transport::auth::StoredCredentials,
) -> Result<(), McpCredentialError> {
    let mut file = read_file()?;
    file.servers.entry(server_id.to_string()).or_default().oauth = Some(credentials);
    write_file(&file)
}

pub fn has_oauth_credentials(server_id: &str) -> bool {
    load_secrets(server_id)
        .map(|secrets| secrets.oauth.is_some())
        .unwrap_or(false)
}

pub fn delete_oauth_credentials(server_id: &str) -> Result<(), McpCredentialError> {
    let mut file = read_file()?;
    if let Some(entry) = file.servers.get_mut(server_id) {
        entry.oauth = None;
        write_file(&file)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keychain::{isolate_config_home, lock_config_home};

    #[test]
    fn secret_values_roundtrip_and_missing_keys() {
        let _lock = lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = isolate_config_home(dir.path());

        assert_eq!(
            missing_keys("srv1", "env", &["TOKEN".to_string()]),
            vec!["TOKEN".to_string()]
        );

        sync_secret_values("srv1", "env", &[("TOKEN".to_string(), "abc".to_string())]).unwrap();
        let secrets = load_secrets("srv1").unwrap();
        assert_eq!(secrets.env.get("TOKEN").unwrap(), "abc");
        assert!(missing_keys("srv1", "env", &["TOKEN".to_string()]).is_empty());

        sync_secret_values(
            "srv1",
            "headers",
            &[("Authorization".to_string(), "Bearer x".to_string())],
        )
        .unwrap();
        let secrets = load_secrets("srv1").unwrap();
        assert_eq!(secrets.headers.get("Authorization").unwrap(), "Bearer x");

        delete_secrets("srv1").unwrap();
        assert!(load_secrets("srv1").unwrap().env.is_empty());
        assert!(load_secrets("srv1").unwrap().headers.is_empty());
    }

    #[test]
    fn file_permissions_are_owner_only() {
        let _lock = lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = isolate_config_home(dir.path());

        sync_secret_values("srv1", "env", &[("K".to_string(), "v".to_string())]).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(credentials_file_path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
