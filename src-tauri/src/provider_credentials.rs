//! Credential seam for Credentialed Providers — resolves API keys and OAuth sessions
//! behind one interface so inference, review, and availability paths stop branching on auth kind.

use thiserror::Error;

use crate::keychain::{delete, has_key, retrieve, retrieve_optional_custom, KeychainError};
use crate::providers::{is_custom_provider, provider as find_provider, ProviderAuthKind};

#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("provider {0} is not supported")]
    UnsupportedProvider(String),
    #[error("credentials not configured for provider {0}")]
    NotConfigured(String),
    #[error("provider {0} does not support API key storage")]
    ApiKeyStorageUnsupported(String),
    #[error("{0}")]
    Keychain(#[from] KeychainError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedAuth {
    ApiKey(String),
    OauthSession,
}

/// Whether Gospel can use this Credentialed Provider (keychain key or OAuth session).
/// A Custom Provider's API key is optional, so a registered one is always usable.
pub fn is_credentialed(provider_id: &str) -> bool {
    match find_provider(provider_id) {
        Some(entry) => match entry.auth_kind {
            ProviderAuthKind::Oauth => entry
                .oauth
                .as_ref()
                .is_some_and(|hooks| (hooks.has_session)()),
            ProviderAuthKind::ApiKey => has_key(provider_id),
        },
        None => is_custom_provider(provider_id),
    }
}

/// Resolve stored credentials for a Credentialed Provider.
pub fn resolve(provider_id: &str) -> Result<ResolvedAuth, CredentialError> {
    let Some(entry) = find_provider(provider_id) else {
        return if is_custom_provider(provider_id) {
            Ok(ResolvedAuth::ApiKey(retrieve_optional_custom(provider_id)?))
        } else {
            Err(CredentialError::UnsupportedProvider(provider_id.to_string()))
        };
    };

    match entry.auth_kind {
        ProviderAuthKind::Oauth => {
            let hooks = entry
                .oauth
                .as_ref()
                .expect("oauth registration must include session hooks");
            if (hooks.has_session)() {
                Ok(ResolvedAuth::OauthSession)
            } else {
                Err(CredentialError::NotConfigured(provider_id.to_string()))
            }
        }
        ProviderAuthKind::ApiKey => Ok(ResolvedAuth::ApiKey(retrieve(provider_id)?)),
    }
}

/// API key string for rig `provider_client` dispatch. OAuth providers return an empty string.
pub fn api_key_for_rig(provider_id: &str) -> Result<String, CredentialError> {
    match resolve(provider_id)? {
        ResolvedAuth::ApiKey(key) => Ok(key),
        ResolvedAuth::OauthSession => Ok(String::new()),
    }
}

/// API key for model-list fetches. OAuth providers return `None`; API-key providers return the key.
pub fn api_key_for_model_fetch(provider_id: &str) -> Result<Option<String>, CredentialError> {
    match resolve(provider_id)? {
        ResolvedAuth::ApiKey(key) => Ok(Some(key)),
        ResolvedAuth::OauthSession => Ok(None),
    }
}

/// Whether a non-empty API key is stored for the provider. A keyless Custom Provider has none.
pub fn has_api_key(provider_id: &str) -> bool {
    matches!(resolve(provider_id), Ok(ResolvedAuth::ApiKey(key)) if !key.is_empty())
}

/// Validates that inference can run: OAuth providers need a session; API-key providers need a
/// non-empty supplied key; Custom Providers run with or without one.
pub fn ensure_inference_ready(
    provider_id: &str,
    supplied_api_key: &str,
) -> Result<(), CredentialError> {
    match find_provider(provider_id) {
        None if is_custom_provider(provider_id) => Ok(()),
        None => Err(CredentialError::UnsupportedProvider(
            provider_id.to_string(),
        )),
        Some(entry) if entry.auth_kind == ProviderAuthKind::Oauth => {
            if is_credentialed(provider_id) {
                Ok(())
            } else {
                Err(CredentialError::NotConfigured(provider_id.to_string()))
            }
        }
        Some(_) => {
            if supplied_api_key.trim().is_empty() {
                Err(CredentialError::NotConfigured(provider_id.to_string()))
            } else {
                Ok(())
            }
        }
    }
}

/// OAuth providers must be credentialed before review/model-fetch work that depends on a session.
#[allow(dead_code)]
pub fn ensure_oauth_session(provider_id: &str) -> Result<(), CredentialError> {
    let entry = find_provider(provider_id)
        .ok_or_else(|| CredentialError::UnsupportedProvider(provider_id.to_string()))?;
    if entry.auth_kind != ProviderAuthKind::Oauth {
        return Ok(());
    }
    if is_credentialed(provider_id) {
        Ok(())
    } else {
        Err(CredentialError::NotConfigured(provider_id.to_string()))
    }
}

pub fn store_api_key(provider_id: &str, api_key: &str) -> Result<(), CredentialError> {
    if find_provider(provider_id).is_some_and(|entry| entry.auth_kind == ProviderAuthKind::Oauth) {
        return Err(CredentialError::ApiKeyStorageUnsupported(
            provider_id.to_string(),
        ));
    }
    crate::keychain::store(provider_id, api_key).map_err(CredentialError::from)
}

pub fn delete_api_key(provider_id: &str) -> Result<(), CredentialError> {
    delete(provider_id).map_err(CredentialError::from)
}

pub fn logout_oauth(provider_id: &str) -> Result<(), CredentialError> {
    crate::keychain::logout_oauth_provider(provider_id).map_err(CredentialError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inference_ready_allows_blank_key_for_oauth_when_credentialed() {
        if is_credentialed("chatgpt") {
            assert!(ensure_inference_ready("chatgpt", "").is_ok());
        }
    }

    #[test]
    fn inference_ready_rejects_blank_key_for_api_key_providers() {
        assert!(ensure_inference_ready("openai", "").is_err());
        assert!(ensure_inference_ready("xai", "").is_err());
    }

    #[test]
    fn oauth_session_check_skips_api_key_providers() {
        assert!(ensure_oauth_session("openai").is_ok());
        assert!(ensure_oauth_session("xai").is_ok());
    }

    #[test]
    fn xai_api_key_storage_is_supported_while_grok_rejects_api_keys() {
        // Isolate the credentials file so the probe key never touches the real store.
        let _lock = crate::keychain::lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = crate::keychain::isolate_config_home(dir.path());
        assert!(matches!(
            store_api_key("grok", "should-not-store"),
            Err(CredentialError::ApiKeyStorageUnsupported(_))
        ));
        // xAI is an API-key provider: the OAuth storage gate must not reject it.
        // Keychain backends may still fail in headless CI; only the unsupported
        // variant is a regression for this issue.
        match store_api_key("xai", "gospel-xai-credential-gate-probe") {
            Ok(()) | Err(CredentialError::Keychain(_)) | Err(CredentialError::NotConfigured(_)) => {}
            Err(CredentialError::ApiKeyStorageUnsupported(_)) => {
                panic!("xAI must support API-key storage like other pay-as-you-go providers")
            }
            Err(CredentialError::UnsupportedProvider(_)) => {
                panic!("xAI must be a registered Credentialed Provider")
            }
        }
    }

    #[test]
    fn api_key_for_rig_returns_empty_for_oauth_providers_when_credentialed() {
        if is_credentialed("chatgpt") {
            assert_eq!(api_key_for_rig("chatgpt").unwrap(), "");
        }
    }

    #[test]
    fn has_api_key_reflects_saved_key_for_custom_providers() {
        let _lock = crate::keychain::lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = crate::keychain::isolate_config_home(dir.path());
        let id = crate::providers::new_custom_provider_id();
        let _registry = crate::providers::RegisteredCustomProviders::new(vec![
            crate::providers::CustomProvider {
                id: id.clone(),
                name: "Local".into(),
                base_url: "https://example.test/v1".into(),
                models: Vec::new(),
            },
        ]);

        // A keyless Custom Provider is credentialed but reports no saved key.
        assert!(is_credentialed(&id));
        assert!(!has_api_key(&id));

        store_api_key(&id, "sk-local").unwrap();
        assert!(has_api_key(&id));

        delete_api_key(&id).unwrap();
        assert!(!has_api_key(&id));

        // Deleting a key for a Custom Provider that was already removed must still work.
        store_api_key(&id, "sk-local").unwrap();
        drop(_registry);
        assert!(delete_api_key(&id).is_ok());
        assert!(!has_api_key(&id));
    }
}
