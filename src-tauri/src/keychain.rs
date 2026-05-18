use keyring::Entry;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum KeychainError {
    #[error("keyring error: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("provider {0} is not supported")]
    UnsupportedProvider(String),
}

const SERVICE_NAME: &str = "gospel";

fn entry_for_provider(provider: &str) -> Result<Entry, KeychainError> {
    let supported = ["openai", "anthropic", "gemini", "groq", "mistral", "chatgpt"];
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
