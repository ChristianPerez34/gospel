//! Helpers that ensure corpus operations stay within the active workspace,
//! even when the workspace contains attacker-planted symlinks.

use std::io;
use std::path::{Path, PathBuf};

/// Resolve `path` to its canonical form while preserving resolution errors.
pub fn canonical(path: &Path) -> io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

/// Return `true` when `target` (already canonical) lives under `root`
/// (already canonical). Empty root is rejected.
pub fn is_within(root: &Path, target: &Path) -> bool {
    if root.as_os_str().is_empty() {
        return false;
    }
    target.starts_with(root)
}

/// Reject paths whose existing ancestors resolve outside `root`.
///
/// Missing suffixes are allowed so callers can create new workspace-local
/// storage, but every existing path component must resolve successfully.
pub fn validate_existing_ancestors(root: &Path, target: &Path) -> io::Result<()> {
    let relative = target.strip_prefix(root).map_err(|_| {
        io::Error::other(format!(
            "path {} is not under workspace root {}",
            target.display(),
            root.display()
        ))
    })?;
    let mut current = root.to_path_buf();

    for component in relative.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(_) => {
                let resolved = canonical(&current)?;
                if !is_within(root, &resolved) {
                    return Err(io::Error::other(format!(
                        "path {} escapes workspace root {}",
                        resolved.display(),
                        root.display()
                    )));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }

    Ok(())
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
        assert!(canonical(&dir.path().join("missing")).is_err());
        assert!(
            validate_existing_ancestors(&canonical_dir, &canonical_dir.join("missing")).is_ok()
        );
        assert!(!is_within(Path::new(""), &canonical_dir));
    }
}
