use serde::Deserialize;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum KeychainError {
    #[error("provider {0} is not supported")]
    UnsupportedProvider(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("credential not found for provider {0}")]
    NotFound(String),
    #[error("API key cannot be empty for provider {0}")]
    EmptyApiKey(String),
    #[error("Provider {0} does not support OAuth")]
    NotOauthProvider(String),
}

fn ensure_known(provider: &str) -> Result<(), KeychainError> {
    if !crate::providers::is_known_provider(provider) {
        return Err(KeychainError::UnsupportedProvider(provider.to_string()));
    }
    Ok(())
}

fn env_api_key(provider: &str) -> Option<String> {
    let key = format!("{}_API_KEY", provider.to_uppercase());
    std::env::var(key)
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

fn credentials_file_path() -> PathBuf {
    gospel_config_dir().join("credentials.json")
}

fn read_credentials() -> Result<std::collections::BTreeMap<String, String>, KeychainError> {
    match std::fs::File::open(credentials_file_path()) {
        Ok(file) => Ok(serde_json::from_reader(file)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(KeychainError::Io(e)),
    }
}

fn write_credentials(
    credentials: &std::collections::BTreeMap<String, String>,
) -> Result<(), KeychainError> {
    let dir = gospel_config_dir();
    std::fs::create_dir_all(&dir)?;
    let mut temp = tempfile::Builder::new()
        .prefix(".credentials-")
        .suffix(".tmp")
        .tempfile_in(&dir)?;
    serde_json::to_writer_pretty(&mut temp, credentials)?;
    temp.persist(credentials_file_path()).map_err(|e| e.error)?;
    Ok(())
}

pub fn store(provider: &str, api_key: &str) -> Result<(), KeychainError> {
    ensure_known(provider)?;
    let key = api_key.trim();
    if key.is_empty() {
        return Err(KeychainError::EmptyApiKey(provider.to_string()));
    }
    let mut credentials = read_credentials()?;
    credentials.insert(provider.to_string(), key.to_string());
    write_credentials(&credentials)
}

/// Resolution order: `~/.config/gospel/credentials.json`, then `{PROVIDER}_API_KEY`.
pub fn retrieve(provider: &str) -> Result<String, KeychainError> {
    ensure_known(provider)?;
    if let Ok(credentials) = read_credentials() {
        if let Some(key) = credentials.get(provider).filter(|key| !key.is_empty()) {
            return Ok(key.clone());
        }
    }
    if let Some(key) = env_api_key(provider) {
        return Ok(key);
    }
    Err(KeychainError::NotFound(provider.to_string()))
}

pub fn delete(provider: &str) -> Result<(), KeychainError> {
    ensure_known(provider)?;
    let mut credentials = read_credentials()?;
    if credentials.remove(provider).is_some() {
        write_credentials(&credentials)?;
    }
    Ok(())
}

pub fn has_key(provider: &str) -> bool {
    retrieve(provider).is_ok()
}

#[derive(Deserialize)]
struct AuthRecord {
    access_token: Option<String>,
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct CopilotApiKeyRecord {
    token: Option<String>,
}

fn xdg_config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::config_dir().unwrap_or_else(std::env::temp_dir))
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|| dirs::config_dir().unwrap_or_else(std::env::temp_dir))
    }
}

pub(crate) fn chatgpt_auth_file_path() -> PathBuf {
    xdg_config_dir().join("chatgpt").join("auth.json")
}

fn gospel_config_dir() -> PathBuf {
    xdg_config_dir().join("gospel")
}

pub(crate) fn github_copilot_token_dir() -> PathBuf {
    gospel_config_dir().join("github_copilot")
}

pub(crate) fn github_copilot_access_token_path() -> PathBuf {
    github_copilot_token_dir().join("access-token")
}

pub(crate) fn github_copilot_api_key_path() -> PathBuf {
    github_copilot_token_dir().join("api-key.json")
}

pub(crate) fn grok_auth_file_path() -> PathBuf {
    gospel_config_dir().join("grok").join("auth.json")
}

pub fn has_chatgpt_oauth_session() -> bool {
    auth_record_has_token(chatgpt_auth_file_path())
}

pub fn delete_chatgpt_auth_file() -> Result<(), KeychainError> {
    delete_if_exists(chatgpt_auth_file_path())
}

pub fn has_github_copilot_oauth_session() -> bool {
    let access_token_path = github_copilot_access_token_path();
    if let Ok(token) = std::fs::read_to_string(access_token_path) {
        if !token.trim().is_empty() {
            return true;
        }
    }

    let api_key_path = github_copilot_api_key_path();
    let content = match std::fs::read_to_string(api_key_path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let record: CopilotApiKeyRecord = match serde_json::from_str(&content) {
        Ok(r) => r,
        Err(_) => return false,
    };
    record
        .token
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

pub fn delete_github_copilot_auth_files() -> Result<(), KeychainError> {
    for path in [
        github_copilot_access_token_path(),
        github_copilot_api_key_path(),
    ] {
        delete_if_exists(path)?;
    }
    Ok(())
}

fn auth_record_has_token(path: PathBuf) -> bool {
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let record: AuthRecord = match serde_json::from_str(&content) {
        Ok(r) => r,
        Err(_) => return false,
    };
    record
        .access_token
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
        || record
            .refresh_token
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
}

pub fn has_grok_oauth_session() -> bool {
    auth_record_has_token(grok_auth_file_path())
}

pub fn delete_grok_auth_file() -> Result<(), KeychainError> {
    delete_if_exists(grok_auth_file_path())
}

fn delete_if_exists(path: PathBuf) -> Result<(), KeychainError> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn provider_has_credentials(provider: &str) -> bool {
    crate::provider_credentials::is_credentialed(provider)
}

pub fn logout_oauth_provider(provider: &str) -> Result<(), KeychainError> {
    logout_oauth_provider_with(provider, delete)
}

fn logout_oauth_provider_with(
    provider: &str,
    delete_keyring: fn(&str) -> Result<(), KeychainError>,
) -> Result<(), KeychainError> {
    let hooks = crate::providers::provider(provider)
        .and_then(|entry| entry.oauth.as_ref())
        .ok_or_else(|| KeychainError::NotOauthProvider(provider.to_string()))?;
    (hooks.delete_session)()?;
    let _ = delete_keyring(provider);
    Ok(())
}

#[cfg(test)]
pub(crate) struct IsolatedConfigHome {
    previous: Option<std::ffi::OsString>,
}

#[cfg(test)]
impl Drop for IsolatedConfigHome {
    fn drop(&mut self) {
        restore_config_home(self.previous.take());
    }
}

#[cfg(test)]
pub(crate) fn lock_config_home() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
pub(crate) fn isolate_config_home(dir: &std::path::Path) -> IsolatedConfigHome {
    IsolatedConfigHome {
        previous: set_config_home(dir),
    }
}

#[cfg(test)]
fn set_config_home(dir: &std::path::Path) -> Option<std::ffi::OsString> {
    let key = config_home_env_key();
    let previous = std::env::var_os(key);
    std::env::set_var(key, dir);
    previous
}

#[cfg(test)]
fn restore_config_home(previous: Option<std::ffi::OsString>) {
    let key = config_home_env_key();
    match previous {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

#[cfg(test)]
fn config_home_env_key() -> &'static str {
    if cfg!(target_os = "windows") {
        "APPDATA"
    } else {
        "XDG_CONFIG_HOME"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_file_roundtrip() {
        let _lock = lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = isolate_config_home(dir.path());
        std::env::remove_var("XAI_API_KEY");

        store("xai", "sk-file-test").unwrap();
        assert_eq!(retrieve("xai").unwrap(), "sk-file-test");
        assert!(has_key("xai"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(credentials_file_path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        delete("xai").unwrap();
        assert!(retrieve("xai").is_err());
        assert!(!has_key("xai"));
    }

    #[test]
    fn store_trims_whitespace_and_rejects_empty_key() {
        let _lock = lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = isolate_config_home(dir.path());

        assert!(matches!(
            store("openai", ""),
            Err(KeychainError::EmptyApiKey(_))
        ));
        assert!(matches!(
            store("openai", "   "),
            Err(KeychainError::EmptyApiKey(_))
        ));

        store("openai", "  sk-openai-spaces  \n").unwrap();
        assert_eq!(retrieve("openai").unwrap(), "sk-openai-spaces");
    }

    #[test]
    fn corrupted_credentials_file_prevents_overwrite_on_store() {
        let _lock = lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = isolate_config_home(dir.path());

        let path = credentials_file_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ invalid json ").unwrap();

        assert!(matches!(
            store("openai", "sk-new"),
            Err(KeychainError::Json(_))
        ));

        // Confirm the file was not overwritten with an empty default
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ invalid json ");
    }

    #[test]
    fn env_api_key_rescues_missing_stored_credential() {
        let _lock = lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = isolate_config_home(dir.path());

        std::env::set_var("OPENCODE_GO_API_KEY", "sk-env-test");
        assert_eq!(retrieve("opencode_go").unwrap(), "sk-env-test");
        assert!(has_key("opencode_go"));

        std::env::set_var("OPENCODE_GO_API_KEY", "   ");
        assert!(env_api_key("opencode_go").is_none());
        std::env::remove_var("OPENCODE_GO_API_KEY");
    }

    #[test]
    fn oauth_provider_credential_gate_uses_local_sessions() {
        let _lock = lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = isolate_config_home(dir.path());

        assert!(!provider_has_credentials("chatgpt"));
        assert!(!provider_has_credentials("github_copilot"));
        assert!(!provider_has_credentials("grok"));

        let chatgpt_dir = dir.path().join("chatgpt");
        std::fs::create_dir_all(&chatgpt_dir).unwrap();
        std::fs::write(
            chatgpt_dir.join("auth.json"),
            r#"{"access_token":"chatgpt-session-token"}"#,
        )
        .unwrap();
        assert!(provider_has_credentials("chatgpt"));

        let copilot_dir = dir.path().join("gospel").join("github_copilot");
        std::fs::create_dir_all(&copilot_dir).unwrap();
        std::fs::write(copilot_dir.join("access-token"), "github-copilot-token").unwrap();
        assert!(provider_has_credentials("github_copilot"));

        let grok_dir = dir.path().join("gospel").join("grok");
        std::fs::create_dir_all(&grok_dir).unwrap();
        std::fs::write(
            grok_dir.join("auth.json"),
            r#"{"access_token":"grok-access-token","refresh_token":"grok-refresh-token"}"#,
        )
        .unwrap();
        assert!(provider_has_credentials("grok"));

        logout_oauth_provider_with("chatgpt", noop_delete_keyring).unwrap();
        assert!(!provider_has_credentials("chatgpt"));
        logout_oauth_provider_with("github_copilot", noop_delete_keyring).unwrap();
        assert!(!provider_has_credentials("github_copilot"));
        logout_oauth_provider_with("grok", noop_delete_keyring).unwrap();
        assert!(!provider_has_credentials("grok"));

        let error = logout_oauth_provider_with("openai", noop_delete_keyring).unwrap_err();
        assert!(error.to_string().contains("openai"));
        assert!(error.to_string().contains("does not support OAuth"));
    }

    fn noop_delete_keyring(_provider: &str) -> Result<(), KeychainError> {
        Ok(())
    }
}
