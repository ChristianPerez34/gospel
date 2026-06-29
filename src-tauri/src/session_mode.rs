pub const SESSION_MODE_BUILD: &str = "Build";
pub const SESSION_MODE_READ_ONLY: &str = "ReadOnly";

pub fn normalize_session_mode(value: Option<&str>) -> &'static str {
    match value {
        Some(SESSION_MODE_READ_ONLY) => SESSION_MODE_READ_ONLY,
        _ => SESSION_MODE_BUILD,
    }
}

pub fn is_valid_session_mode(value: &str) -> bool {
    matches!(value, SESSION_MODE_BUILD | SESSION_MODE_READ_ONLY)
}

pub fn session_mode_allows_source_edit(value: &str) -> bool {
    normalize_session_mode(Some(value)) == SESSION_MODE_BUILD
}
