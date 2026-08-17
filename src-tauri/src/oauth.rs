#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OauthStartAdapter {
    Chatgpt,
    GithubCopilot,
}

pub struct OauthProviderRegistration {
    pub id: &'static str,
    pub auth_complete_event: &'static str,
    pub start_adapter: OauthStartAdapter,
    pub has_session: fn() -> bool,
    pub delete_session: fn() -> Result<(), crate::keychain::KeychainError>,
}

pub const OAUTH_PROVIDERS: &[OauthProviderRegistration] = &[
    OauthProviderRegistration {
        id: "chatgpt",
        auth_complete_event: "chatgpt-auth-complete",
        start_adapter: OauthStartAdapter::Chatgpt,
        has_session: crate::keychain::has_chatgpt_oauth_session,
        delete_session: crate::keychain::delete_chatgpt_auth_file,
    },
    OauthProviderRegistration {
        id: "github_copilot",
        auth_complete_event: "github-copilot-auth-complete",
        start_adapter: OauthStartAdapter::GithubCopilot,
        has_session: crate::keychain::has_github_copilot_oauth_session,
        delete_session: crate::keychain::delete_github_copilot_auth_files,
    },
];

pub fn oauth_provider(id: &str) -> Option<&'static OauthProviderRegistration> {
    OAUTH_PROVIDERS.iter().find(|provider| provider.id == id)
}

pub fn oauth_provider_ids() -> Vec<&'static str> {
    OAUTH_PROVIDERS.iter().map(|provider| provider.id).collect()
}
