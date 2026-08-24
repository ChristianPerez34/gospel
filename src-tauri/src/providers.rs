//! Credentialed Provider registry — single source of truth for supported provider identity,
//! auth kind, OAuth session hooks, and model-fetch strategy.

use crate::keychain::KeychainError;

pub struct OauthProviderHooks {
    pub auth_complete_event: &'static str,
    pub has_session: fn() -> bool,
    pub delete_session: fn() -> Result<(), KeychainError>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderAuthKind {
    ApiKey,
    Oauth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelFetchKind {
    RigOpenAI,
    RigAnthropic,
    RigGemini,
    RigMistral,
    CustomChatGpt,
    RigOauthCopilot,
    CustomGrok,
    StaticHardcoded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelCacheScope {
    ApiKey,
    Shared,
    Account,
}

pub struct CredentialedProviderRegistration {
    pub id: &'static str,
    pub display_name: &'static str,
    pub auth_kind: ProviderAuthKind,
    pub model_fetch: ModelFetchKind,
    pub model_cache_scope: ModelCacheScope,
    pub oauth: Option<OauthProviderHooks>,
}

pub const CREDENTIALED_PROVIDERS: &[CredentialedProviderRegistration] = &[
    CredentialedProviderRegistration {
        id: "openai",
        display_name: "OpenAI",
        auth_kind: ProviderAuthKind::ApiKey,
        model_fetch: ModelFetchKind::RigOpenAI,
        model_cache_scope: ModelCacheScope::ApiKey,
        oauth: None,
    },
    CredentialedProviderRegistration {
        id: "chatgpt",
        display_name: "ChatGPT Plus/Pro",
        auth_kind: ProviderAuthKind::Oauth,
        model_fetch: ModelFetchKind::CustomChatGpt,
        model_cache_scope: ModelCacheScope::Shared,
        oauth: Some(OauthProviderHooks {
            auth_complete_event: "chatgpt-auth-complete",
            has_session: crate::keychain::has_chatgpt_oauth_session,
            delete_session: crate::keychain::delete_chatgpt_auth_file,
        }),
    },
    CredentialedProviderRegistration {
        id: "github_copilot",
        display_name: "GitHub Copilot",
        auth_kind: ProviderAuthKind::Oauth,
        model_fetch: ModelFetchKind::RigOauthCopilot,
        model_cache_scope: ModelCacheScope::Shared,
        oauth: Some(OauthProviderHooks {
            auth_complete_event: "github-copilot-auth-complete",
            has_session: crate::keychain::has_github_copilot_oauth_session,
            delete_session: crate::keychain::delete_github_copilot_auth_files,
        }),
    },
    CredentialedProviderRegistration {
        id: "grok",
        display_name: "Grok",
        auth_kind: ProviderAuthKind::Oauth,
        model_fetch: ModelFetchKind::CustomGrok,
        model_cache_scope: ModelCacheScope::Account,
        oauth: Some(OauthProviderHooks {
            auth_complete_event: "grok-auth-complete",
            has_session: crate::keychain::has_grok_oauth_session,
            delete_session: crate::keychain::delete_grok_auth_file,
        }),
    },
    CredentialedProviderRegistration {
        id: "anthropic",
        display_name: "Anthropic",
        auth_kind: ProviderAuthKind::ApiKey,
        model_fetch: ModelFetchKind::RigAnthropic,
        model_cache_scope: ModelCacheScope::ApiKey,
        oauth: None,
    },
    CredentialedProviderRegistration {
        id: "gemini",
        display_name: "Gemini",
        auth_kind: ProviderAuthKind::ApiKey,
        model_fetch: ModelFetchKind::RigGemini,
        model_cache_scope: ModelCacheScope::ApiKey,
        oauth: None,
    },
    CredentialedProviderRegistration {
        id: "groq",
        display_name: "Groq",
        auth_kind: ProviderAuthKind::ApiKey,
        model_fetch: ModelFetchKind::StaticHardcoded,
        model_cache_scope: ModelCacheScope::Shared,
        oauth: None,
    },
    CredentialedProviderRegistration {
        id: "mistral",
        display_name: "Mistral",
        auth_kind: ProviderAuthKind::ApiKey,
        model_fetch: ModelFetchKind::RigMistral,
        model_cache_scope: ModelCacheScope::ApiKey,
        oauth: None,
    },
];

pub fn provider(id: &str) -> Option<&'static CredentialedProviderRegistration> {
    CREDENTIALED_PROVIDERS.iter().find(|entry| entry.id == id)
}

pub fn provider_ids() -> Vec<&'static str> {
    CREDENTIALED_PROVIDERS.iter().map(|entry| entry.id).collect()
}

pub fn oauth_provider_ids() -> Vec<&'static str> {
    CREDENTIALED_PROVIDERS
        .iter()
        .filter(|entry| entry.auth_kind == ProviderAuthKind::Oauth)
        .map(|entry| entry.id)
        .collect()
}

pub fn is_known_provider(id: &str) -> bool {
    provider(id).is_some()
}

pub fn is_oauth_provider(id: &str) -> bool {
    provider(id).is_some_and(|entry| entry.auth_kind == ProviderAuthKind::Oauth)
}

pub fn provider_display_name(id: &str) -> &'static str {
    provider(id)
        .map(|entry| entry.display_name)
        .unwrap_or("Unknown Provider")
}

pub fn provider_auth_type(id: &str) -> &'static str {
    match provider(id).map(|entry| entry.auth_kind) {
        Some(ProviderAuthKind::Oauth) => "oauth",
        Some(ProviderAuthKind::ApiKey) => "api_key",
        None => "api_key",
    }
}

pub fn model_cache_scope_key(
    entry: &CredentialedProviderRegistration,
    api_key: Option<&str>,
) -> String {
    match entry.model_cache_scope {
        ModelCacheScope::ApiKey => api_key.unwrap_or("").to_string(),
        ModelCacheScope::Shared => "shared".to_string(),
        ModelCacheScope::Account => account_cache_scope_key(entry, api_key),
    }
}

fn account_cache_scope_key(
    entry: &CredentialedProviderRegistration,
    api_key: Option<&str>,
) -> String {
    if let Some(key) = api_key.map(str::trim).filter(|key| !key.is_empty()) {
        return hashed_account_scope(entry.id, key);
    }
    match entry.id {
        "grok" => crate::grok_oauth::refresh_token(&crate::keychain::grok_auth_file_path())
            .map(|token| hashed_account_scope(entry.id, &token))
            .unwrap_or_else(|_| "account".to_string()),
        _ => "account".to_string(),
    }
}

fn hashed_account_scope(provider_id: &str, secret: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(provider_id.as_bytes());
    hasher.update([0]);
    hasher.update(secret.as_bytes());
    format!(
        "account:{}",
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentialed_provider_registry_lists_all_supported_providers() {
        assert_eq!(
            provider_ids(),
            [
                "openai",
                "chatgpt",
                "github_copilot",
                "grok",
                "anthropic",
                "gemini",
                "groq",
                "mistral"
            ]
        );
    }

    #[test]
    fn oauth_provider_ids_match_oauth_registrations() {
        assert_eq!(oauth_provider_ids(), ["chatgpt", "github_copilot", "grok"]);
        for id in oauth_provider_ids() {
            assert!(is_oauth_provider(id));
            assert_eq!(provider_auth_type(id), "oauth");
            assert!(provider(id).unwrap().oauth.is_some());
        }
    }

    #[test]
    fn api_key_providers_are_not_oauth() {
        for id in ["openai", "anthropic", "gemini", "groq", "mistral"] {
            assert!(!is_oauth_provider(id));
            assert_eq!(provider_auth_type(id), "api_key");
            assert!(provider(id).unwrap().oauth.is_none());
        }
    }

    #[test]
    fn display_names_are_registered_for_every_provider() {
        for entry in CREDENTIALED_PROVIDERS {
            assert_eq!(provider_display_name(entry.id), entry.display_name);
            assert_ne!(entry.display_name, "Unknown Provider");
        }
    }

    #[test]
    fn grok_uses_account_model_cache_scope() {
        let grok = provider("grok").unwrap();
        assert_eq!(grok.model_fetch, ModelFetchKind::CustomGrok);
        assert_eq!(grok.model_cache_scope, ModelCacheScope::Account);
        assert!(grok.oauth.is_some());
    }

    #[test]
    fn grok_account_cache_scope_changes_when_session_account_changes() {
        let grok = provider("grok").unwrap();
        let first = model_cache_scope_key(grok, Some("account-one-token"));
        let second = model_cache_scope_key(grok, Some("account-two-token"));

        assert_ne!(first, second);
        assert!(first.starts_with("account:"));
        assert!(second.starts_with("account:"));
        assert!(!first.contains("account-one-token"));
        assert!(!second.contains("account-two-token"));
    }

    #[test]
    fn grok_oauth_session_cache_scope_follows_refresh_token() {
        let _lock = crate::keychain::lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = crate::keychain::isolate_config_home(dir.path());
        let grok = provider("grok").unwrap();
        let grok_dir = dir.path().join("gospel").join("grok");
        std::fs::create_dir_all(&grok_dir).unwrap();
        let auth_path = grok_dir.join("auth.json");

        std::fs::write(
            &auth_path,
            r#"{"access_token":"access-a","refresh_token":"refresh-a"}"#,
        )
        .unwrap();
        let first = model_cache_scope_key(grok, None);

        std::fs::write(
            &auth_path,
            r#"{"access_token":"access-b","refresh_token":"refresh-b"}"#,
        )
        .unwrap();
        let second = model_cache_scope_key(grok, None);

        assert_ne!(first, second);
        assert!(first.starts_with("account:"));
        assert!(second.starts_with("account:"));
    }
}
