use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AntiPatternStore {
    pub rejected_hashes: HashSet<String>,
}

impl AntiPatternStore {
    pub fn load(workspace_path: &Path) -> Result<Self, String> {
        let path = workspace_path
            .join(".gospel")
            .join("rejected_findings.json");
        match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).map_err(|e| {
                format!(
                    "Failed to parse rejected findings store {}: {}",
                    path.display(),
                    e
                )
            }),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!(
                "Failed to read rejected findings store {}: {}",
                path.display(),
                error
            )),
        }
    }

    pub fn save(&self, workspace_path: &Path) -> Result<(), String> {
        let gospel_dir = workspace_path.join(".gospel");
        if !gospel_dir.exists() {
            fs::create_dir_all(&gospel_dir).map_err(|e| e.to_string())?;
        }
        let path = gospel_dir.join("rejected_findings.json");
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, content).map_err(|e| e.to_string())
    }

    pub fn add_rejection(&mut self, file: &str, line_start: usize, line_end: usize, title: &str) {
        let hash = rejection_hash(file, line_start, line_end, title);
        self.rejected_hashes.insert(hash);
    }

    pub fn is_rejected(&self, file: &str, line_start: usize, line_end: usize, title: &str) -> bool {
        let hash = rejection_hash(file, line_start, line_end, title);
        self.rejected_hashes.contains(&hash)
    }
}

fn rejection_hash(file: &str, line_start: usize, line_end: usize, title: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"gospel-review-rejection-v1");
    update_length_prefixed(&mut hasher, &normalize_file_anchor(file));
    hasher.update(line_start.to_le_bytes());
    hasher.update(line_end.to_le_bytes());
    update_length_prefixed(&mut hasher, title);
    hex_digest(hasher.finalize().as_slice())
}

fn normalize_file_anchor(file: &str) -> String {
    file.trim_start_matches("./").replace('\\', "/")
}

fn update_length_prefixed(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_le_bytes());
    hasher.update(value.as_bytes());
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rejection_hash_disambiguates_anchor_fields() {
        let mut store = AntiPatternStore::default();

        store.add_rejection("ab", 1, 2, "c");

        assert!(store.is_rejected("ab", 1, 2, "c"));
        assert!(!store.is_rejected("a", 1, 2, "bc"));
        assert!(!store.is_rejected("ab", 12, 0, "c"));
    }

    #[test]
    fn missing_rejected_findings_store_loads_empty() {
        let dir = tempdir().unwrap();

        let store = AntiPatternStore::load(dir.path()).unwrap();

        assert!(store.rejected_hashes.is_empty());
    }

    #[test]
    fn corrupt_rejected_findings_store_returns_error() {
        let dir = tempdir().unwrap();
        let gospel_dir = dir.path().join(".gospel");
        fs::create_dir_all(&gospel_dir).unwrap();
        fs::write(gospel_dir.join("rejected_findings.json"), "{not json").unwrap();

        let error = AntiPatternStore::load(dir.path()).unwrap_err();

        assert!(error.contains("Failed to parse rejected findings store"));
    }
}
