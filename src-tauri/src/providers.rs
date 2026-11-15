//! Credentialed Provider registry — single source of truth for supported provider identity,
//! auth kind, OAuth session hooks, and model-fetch strategy. User-defined Custom Providers
//! register here at runtime, so every consumer resolves providers through this module.

use crate::keychain::KeychainError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{LazyLock, RwLock};

const CUSTOM_PROVIDER_ID_PREFIX: &str = "custom_";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub models: Vec<String>,
}

static CUSTOM_PROVIDERS: LazyLock<RwLock<Vec<CustomProvider>>> =
    LazyLock::new(|| RwLock::new(Vec::new()));

pub fn custom_providers() -> Vec<CustomProvider> {
    CUSTOM_PROVIDERS
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub fn custom_provider(id: &str) -> Option<CustomProvider> {
    CUSTOM_PROVIDERS
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|provider| provider.id == id)
        .cloned()
}

pub fn is_custom_provider(id: &str) -> bool {
    CUSTOM_PROVIDERS
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|provider| provider.id == id)
}

pub fn replace_custom_providers(providers: Vec<CustomProvider>) {
    *CUSTOM_PROVIDERS.write().unwrap_or_else(|e| e.into_inner()) = providers;
}

/// A fresh Custom Provider id. The prefix keeps it from colliding with a built-in id.
pub fn new_custom_provider_id() -> String {
    format!("{CUSTOM_PROVIDER_ID_PREFIX}{}", uuid::Uuid::new_v4())
}

/// Whether `id` has the shape of a Custom Provider id. Registration is a separate question,
/// see [`is_custom_provider`].
pub fn is_custom_provider_id(id: &str) -> bool {
    id.strip_prefix(CUSTOM_PROVIDER_ID_PREFIX)
        .is_some_and(|rest| uuid::Uuid::parse_str(rest).is_ok())
}

/// Trims model IDs, drops blanks, and removes repeats, keeping first-seen order.
pub fn normalize_model_ids(ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty() && seen.insert(id.clone()))
        .collect()
}

/// Test-only handle that registers Custom Providers and clears them on drop. Hold
/// `keychain::lock_config_home()` first: the registry is process-global.
#[cfg(test)]
pub(crate) struct RegisteredCustomProviders;

#[cfg(test)]
impl RegisteredCustomProviders {
    pub(crate) fn new(providers: Vec<CustomProvider>) -> Self {
        replace_custom_providers(providers);
        Self
    }
}

#[cfg(test)]
impl Drop for RegisteredCustomProviders {
    fn drop(&mut self) {
        replace_custom_providers(Vec::new());
    }
}

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
    CustomXai,
    CustomOpenCodeGo,
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
        id: "xai",
        display_name: "xAI",
        auth_kind: ProviderAuthKind::ApiKey,
        model_fetch: ModelFetchKind::CustomXai,
        model_cache_scope: ModelCacheScope::ApiKey,
        oauth: None,
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
    CredentialedProviderRegistration {
        id: "opencode_go",
        display_name: "OpenCode Go",
        auth_kind: ProviderAuthKind::ApiKey,
        model_fetch: ModelFetchKind::CustomOpenCodeGo,
        model_cache_scope: ModelCacheScope::ApiKey,
        oauth: None,
    },
];

pub fn provider(id: &str) -> Option<&'static CredentialedProviderRegistration> {
    CREDENTIALED_PROVIDERS.iter().find(|entry| entry.id == id)
}

pub fn provider_ids() -> Vec<&'static str> {
    CREDENTIALED_PROVIDERS
        .iter()
        .map(|entry| entry.id)
        .collect()
}

/// Built-in registrations followed by Custom Providers: every provider the app can list.
pub fn all_provider_ids() -> Vec<String> {
    provider_ids()
        .into_iter()
        .map(String::from)
        .chain(custom_providers().into_iter().map(|custom| custom.id))
        .collect()
}

pub fn oauth_provider_ids() -> Vec<&'static str> {
    CREDENTIALED_PROVIDERS
        .iter()
        .filter(|entry| entry.auth_kind == ProviderAuthKind::Oauth)
        .map(|entry| entry.id)
        .collect()
}

pub fn is_known_provider(id: &str) -> bool {
    provider(id).is_some() || is_custom_provider(id)
}

pub fn is_oauth_provider(id: &str) -> bool {
    provider(id).is_some_and(|entry| entry.auth_kind == ProviderAuthKind::Oauth)
}

pub fn provider_display_name(id: &str) -> &'static str {
    provider(id)
        .map(|entry| entry.display_name)
        .unwrap_or("Unknown Provider")
}

/// Name to show for any provider id: a Custom Provider's own name, the registered display
/// name, or the id itself when nothing is registered under it.
pub fn display_name(id: &str) -> String {
    if let Some(custom) = custom_provider(id) {
        return custom.name;
    }
    match provider_display_name(id) {
        "Unknown Provider" => id.to_string(),
        name => name.to_string(),
    }
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
                "xai",
                "anthropic",
                "gemini",
                "groq",
                "mistral",
                "opencode_go"
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
        for id in ["openai", "xai", "anthropic", "gemini", "groq", "mistral", "opencode_go"] {
            assert!(!is_oauth_provider(id));
            assert_eq!(provider_auth_type(id), "api_key");
            assert!(provider(id).unwrap().oauth.is_none());
        }
    }

    #[test]
    fn xai_api_key_provider_is_distinct_from_grok_oauth() {
        let xai = provider("xai").unwrap();
        let grok = provider("grok").unwrap();
        assert_eq!(xai.display_name, "xAI");
        assert_eq!(xai.auth_kind, ProviderAuthKind::ApiKey);
        assert_eq!(xai.model_fetch, ModelFetchKind::CustomXai);
        assert_eq!(xai.model_cache_scope, ModelCacheScope::ApiKey);
        assert!(xai.oauth.is_none());
        assert_eq!(grok.auth_kind, ProviderAuthKind::Oauth);
        assert_ne!(xai.id, grok.id);
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

    #[test]
    fn custom_provider_ids_have_a_recognizable_shape() {
        for _ in 0..4 {
            assert!(is_custom_provider_id(&new_custom_provider_id()));
        }
        assert!(!is_custom_provider_id("openai"));
        assert!(!is_custom_provider_id("custom_"));
        assert!(!is_custom_provider_id("custom_not-a-uuid"));
        assert!(!is_custom_provider_id("custom_openai"));
    }

    #[test]
    fn normalize_model_ids_trims_drops_blanks_and_dedupes() {
        assert_eq!(
            normalize_model_ids([
                "  llama-3 ".to_string(),
                String::new(),
                "   ".to_string(),
                "other/model".to_string(),
                "llama-3".to_string(),
            ]),
            ["llama-3", "other/model"]
        );
        assert!(normalize_model_ids(Vec::new()).is_empty());
    }

    #[test]
    fn all_provider_ids_lists_builtins_then_custom_providers() {
        let _lock = crate::keychain::lock_config_home();
        let _registry = RegisteredCustomProviders::new(vec![CustomProvider {
            id: "custom_test".into(),
            name: "Local".into(),
            base_url: "https://example.test/v1".into(),
            models: Vec::new(),
        }]);

        let ids = all_provider_ids();
        assert_eq!(ids.len(), CREDENTIALED_PROVIDERS.len() + 1);
        assert!(ids.starts_with(&CREDENTIALED_PROVIDERS.iter().map(|entry| entry.id.to_string()).collect::<Vec<_>>()));
        assert_eq!(ids.last().map(String::as_str), Some("custom_test"));
        assert!(is_custom_provider("custom_test"));
        assert!(is_known_provider("custom_test"));
        assert!(!is_custom_provider("openai"));
    }

    #[test]
    fn display_name_prefers_custom_name_then_registration() {
        let _lock = crate::keychain::lock_config_home();
        let _registry = RegisteredCustomProviders::new(vec![CustomProvider {
            id: "custom_test".into(),
            name: "Local".into(),
            base_url: "https://example.test/v1".into(),
            models: Vec::new(),
        }]);

        assert_eq!(display_name("custom_test"), "Local");
        assert_eq!(display_name("openai"), "OpenAI");
        assert_eq!(display_name("unregistered"), "unregistered");
    }
}
