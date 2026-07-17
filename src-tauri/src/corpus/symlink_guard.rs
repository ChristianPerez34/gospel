//! Helpers that ensure corpus operations stay within the active workspace,
//! even when the workspace contains attacker-planted symlinks.

use std::path::{Path, PathBuf};

/// Resolve `path` to its canonical form. Returns `None` if the path does
/// not exist or cannot be canonicalized.
pub fn canonical(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

/// Return `true` when `target` (already canonical) lives under `root`
/// (already canonical). Empty root is rejected.
pub fn is_within(root: &Path, target: &Path) -> bool {
    if root.as_os_str().is_empty() {
        return false;
    }
    target.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn canonical_resolves_existing_paths_and_rejects_missing_paths() {
        let dir = TempDir::new().unwrap();
        let canonical_dir = canonical(dir.path()).expect("canonical tempdir");

        assert!(is_within(&canonical_dir, &canonical_dir));
        assert!(canonical(&dir.path().join("missing")).is_none());
        assert!(!is_within(Path::new(""), &canonical_dir));
    }
}
