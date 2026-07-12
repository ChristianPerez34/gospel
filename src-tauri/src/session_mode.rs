use serde::{Deserialize, Serialize};

pub const SESSION_MODE_BUILD: &str = "Build";
pub const SESSION_MODE_READ_ONLY: &str = "ReadOnly";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionMode {
    Build,
    ReadOnly,
}

impl SessionMode {
    pub fn from_stored(value: &str) -> Self {
        match value {
            SESSION_MODE_BUILD => Self::Build,
            SESSION_MODE_READ_ONLY => Self::ReadOnly,
            _ => Self::ReadOnly,
        }
    }

    pub fn allows_source_edit(self) -> bool {
        self == Self::Build
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Build => SESSION_MODE_BUILD,
            Self::ReadOnly => SESSION_MODE_READ_ONLY,
        }
    }
}

pub fn normalize_session_mode(value: Option<&str>) -> &'static str {
    value
        .map(SessionMode::from_stored)
        .unwrap_or(SessionMode::Build)
        .as_str()
}

pub fn is_valid_session_mode(value: &str) -> bool {
    matches!(value, SESSION_MODE_BUILD | SESSION_MODE_READ_ONLY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_modes_fail_closed() {
        assert_eq!(
            normalize_session_mode(Some("Buidl")),
            SESSION_MODE_READ_ONLY
        );
        assert_eq!(SessionMode::from_stored("Buidl"), SessionMode::ReadOnly);
        assert!(!SessionMode::from_stored("Buidl").allows_source_edit());
    }
}
