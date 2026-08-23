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
        model_cache_scope: ModelCacheScope::ApiKey,
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
    }
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
}
