//! Credential seam for Credentialed Providers — resolves API keys and OAuth sessions
//! behind one interface so inference, review, and availability paths stop branching on auth kind.

use thiserror::Error;

use crate::keychain::{delete, has_key, retrieve, KeychainError};
use crate::providers::{provider as find_provider, ProviderAuthKind};

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
pub fn is_credentialed(provider_id: &str) -> bool {
    match find_provider(provider_id) {
        Some(entry) => match entry.auth_kind {
            ProviderAuthKind::Oauth => entry
                .oauth
                .as_ref()
                .is_some_and(|hooks| (hooks.has_session)()),
            ProviderAuthKind::ApiKey => has_key(provider_id),
        },
        None => false,
    }
}

/// Resolve stored credentials for a Credentialed Provider.
pub fn resolve(provider_id: &str) -> Result<ResolvedAuth, CredentialError> {
    let entry = find_provider(provider_id)
        .ok_or_else(|| CredentialError::UnsupportedProvider(provider_id.to_string()))?;

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

/// Validates that inference can run: OAuth providers need a session; API-key providers need a non-empty supplied key.
pub fn ensure_inference_ready(
    provider_id: &str,
    supplied_api_key: &str,
) -> Result<(), CredentialError> {
    match find_provider(provider_id) {
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
    }

    #[test]
    fn oauth_session_check_skips_api_key_providers() {
        assert!(ensure_oauth_session("openai").is_ok());
    }

    #[test]
    fn api_key_for_rig_returns_empty_for_oauth_providers_when_credentialed() {
        if is_credentialed("chatgpt") {
            assert_eq!(api_key_for_rig("chatgpt").unwrap(), "");
        }
    }
}
