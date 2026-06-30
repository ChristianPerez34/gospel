use keyring::Entry;
use serde::Deserialize;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum KeychainError {
    #[error("keyring error: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("provider {0} is not supported")]
    UnsupportedProvider(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

const SERVICE_NAME: &str = "gospel";

fn entry_for_provider(provider: &str) -> Result<Entry, KeychainError> {
    let supported = [
        "openai",
        "anthropic",
        "gemini",
        "groq",
        "mistral",
        "chatgpt",
        "github_copilot",
    ];
    if !supported.contains(&provider) {
        return Err(KeychainError::UnsupportedProvider(provider.to_string()));
    }
    Ok(Entry::new(SERVICE_NAME, provider)?)
}

pub fn store(provider: &str, api_key: &str) -> Result<(), KeychainError> {
    let entry = entry_for_provider(provider)?;
    entry.set_password(api_key)?;
    Ok(())
}

pub fn retrieve(provider: &str) -> Result<String, KeychainError> {
    let entry = entry_for_provider(provider)?;
    let key = entry.get_password()?;
    Ok(key)
}

pub fn delete(provider: &str) -> Result<(), KeychainError> {
    let entry = entry_for_provider(provider)?;
    entry.delete_credential()?;
    Ok(())
}

pub fn has_key(provider: &str) -> bool {
    entry_for_provider(provider)
        .map(|e| e.get_password().is_ok())
        .unwrap_or(false)
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

fn github_copilot_access_token_path() -> PathBuf {
    github_copilot_token_dir().join("access-token")
}

fn github_copilot_api_key_path() -> PathBuf {
    github_copilot_token_dir().join("api-key.json")
}

pub fn has_chatgpt_oauth_session() -> bool {
    let path = chatgpt_auth_file_path();
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

pub fn delete_chatgpt_auth_file() -> Result<(), KeychainError> {
    let path = chatgpt_auth_file_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
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
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn provider_has_credentials(provider: &str) -> bool {
    match provider {
        "chatgpt" => has_chatgpt_oauth_session(),
        "github_copilot" => has_github_copilot_oauth_session(),
        _ => has_key(provider),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keychain_roundtrip() {
        let provider = "openai";
        let test_key = "sk-test-roundtrip-12345";

        let entry = Entry::new(SERVICE_NAME, provider).unwrap();
        let _ = entry.delete_credential();

        entry.set_password(test_key).unwrap();
        assert!(entry.get_password().is_ok());
        assert_eq!(entry.get_password().unwrap(), test_key);
        entry.delete_credential().unwrap();
        assert!(entry.get_password().is_err());
    }
}
