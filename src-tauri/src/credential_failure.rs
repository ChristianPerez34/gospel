//! Classify provider credential failures into re-auth vs entitlement/tier-gate.

/// How Gospel should guide the user after a credential-related provider failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialFailureKind {
    /// Session/refresh credentials are invalid or expired — signing in again may fix it.
    ReauthRequired,
    /// OAuth session is valid but the account is not entitled — re-login will not help.
    EntitlementBlocked,
}

/// Map an uncontrolled provider/OAuth error string to a credential-failure kind.
///
/// Returns `None` when the error is not clearly credential-related.
pub fn classify_credential_failure(error: &str) -> Option<CredentialFailureKind> {
    let lower = error.to_lowercase();

    if is_reauth_failure(&lower) {
        return Some(CredentialFailureKind::ReauthRequired);
    }
    if is_entitlement_failure(&lower) {
        return Some(CredentialFailureKind::EntitlementBlocked);
    }
    None
}

fn is_reauth_failure(lower: &str) -> bool {
    lower.contains("invalid_grant")
        || lower.contains("sign in again")
        || lower.contains("oauth session expired")
        || lower.contains("oauth session not found")
        || lower.contains("oauth provider credential not found")
        || lower.contains("refresh token")
            && (lower.contains("invalid")
                || lower.contains("expired")
                || lower.contains("revoked")
                || lower.contains("missing"))
        || (lower.contains("401") && !is_entitlement_failure(lower))
        || (lower.contains("unauthorized") && !is_entitlement_failure(lower))
}

fn is_entitlement_failure(lower: &str) -> bool {
    lower.contains("entitlement")
        || lower.contains("tier-gate")
        || lower.contains("tier gate")
        || lower.contains("spending")
        || lower.contains("not entitled")
        || lower.contains("subscription required")
        || lower.contains("upgrade required")
        || lower.contains("supergrok")
        || lower.contains("premium+")
        || lower.contains("insufficient_quota")
        || lower.contains("insufficient quota")
        || (lower.contains("403")
            && (lower.contains("forbidden")
                || lower.contains("tier")
                || lower.contains("subscription")
                || lower.contains("plan")))
}

pub fn reauth_user_message() -> &'static str {
    "Provider session expired or is invalid. Sign in again."
}

pub fn entitlement_user_message() -> &'static str {
    "Your account is signed in but not entitled for this request. Signing in again will not fix this. Upgrade SuperGrok / X Premium+ or configure an xAI API key in Settings."
}

pub fn entitlement_model_fetch_detail() -> &'static str {
    "Account is signed in but not entitled for model access. Signing in again will not fix this — upgrade SuperGrok / X Premium+ or use an xAI API key."
}

/// Whether an OAuth refresh error means the stored refresh credential is unusable.
pub fn refresh_error_requires_reauth(error: &str) -> bool {
    matches!(
        classify_credential_failure(error),
        Some(CredentialFailureKind::ReauthRequired)
    ) || {
        let lower = error.to_lowercase();
        lower.contains("not found")
            || lower.contains("http 400")
            || lower.contains("http 401")
            || lower.contains("invalid_grant")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_refresh_maps_to_reauth() {
        assert_eq!(
            classify_credential_failure("Grok token refresh failed (HTTP 400): invalid_grant"),
            Some(CredentialFailureKind::ReauthRequired)
        );
        assert_eq!(
            classify_credential_failure("Grok OAuth session expired or invalid; sign in again"),
            Some(CredentialFailureKind::ReauthRequired)
        );
        assert_eq!(
            classify_credential_failure("HTTP 401 Unauthorized: access token rejected"),
            Some(CredentialFailureKind::ReauthRequired)
        );
        assert_eq!(
            classify_credential_failure("refresh token expired for this account"),
            Some(CredentialFailureKind::ReauthRequired)
        );
    }

    #[test]
    fn entitlement_and_tier_gate_map_to_entitlement() {
        assert_eq!(
            classify_credential_failure(
                "HTTP 403 Forbidden: entitlement check failed for subscription tier"
            ),
            Some(CredentialFailureKind::EntitlementBlocked)
        );
        assert_eq!(
            classify_credential_failure("spending limit reached for this SuperGrok account"),
            Some(CredentialFailureKind::EntitlementBlocked)
        );
        assert_eq!(
            classify_credential_failure("account not entitled for cli chat proxy"),
            Some(CredentialFailureKind::EntitlementBlocked)
        );
    }

    #[test]
    fn ordinary_provider_errors_are_unclassified() {
        assert_eq!(
            classify_credential_failure("connection refused"),
            None
        );
        assert_eq!(
            classify_credential_failure("HTTP 500 internal server error"),
            None
        );
    }
}
