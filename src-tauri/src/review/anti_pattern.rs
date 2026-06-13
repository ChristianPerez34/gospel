use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static STORE_SAVE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RejectedFindingRecord {
    pub hash: String,
    pub rejected_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AntiPatternStore {
    #[serde(default)]
    pub rejected_hashes: HashSet<String>,
    #[serde(default)]
    pub rejected_findings: Vec<RejectedFindingRecord>,
}

impl AntiPatternStore {
    pub fn load(workspace_path: &Path) -> Result<Self, String> {
        let path = workspace_path
            .join(".gospel")
            .join("rejected_findings.json");
        match fs::read_to_string(&path) {
            Ok(content) => {
                let mut store: Self = serde_json::from_str(&content).map_err(|e| {
                    format!(
                        "Failed to parse rejected findings store {}: {}",
                        path.display(),
                        e
                    )
                })?;
                store.normalize_loaded();
                Ok(store)
            }
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
        write_store_atomically(&path, &content)
    }

    pub fn add_rejection(&mut self, file: &str, line_start: usize, line_end: usize, title: &str) {
        self.add_rejection_at(file, line_start, line_end, title, Utc::now());
    }

    pub fn add_rejection_at(
        &mut self,
        file: &str,
        line_start: usize,
        line_end: usize,
        title: &str,
        rejected_at: DateTime<Utc>,
    ) {
        let hash = rejection_hash(file, line_start, line_end, title);
        self.rejected_hashes.insert(hash.clone());
        if let Some(record) = self
            .rejected_findings
            .iter_mut()
            .find(|record| record.hash == hash)
        {
            record.rejected_at = rejected_at;
        } else {
            self.rejected_findings
                .push(RejectedFindingRecord { hash, rejected_at });
        }
    }

    pub fn is_rejected(&self, file: &str, line_start: usize, line_end: usize, title: &str) -> bool {
        let hash = rejection_hash(file, line_start, line_end, title);
        self.rejected_hashes.contains(&hash)
            || self
                .rejected_findings
                .iter()
                .any(|record| record.hash == hash)
    }

    fn normalize_loaded(&mut self) {
        for record in &self.rejected_findings {
            self.rejected_hashes.insert(record.hash.clone());
        }

        let now = Utc::now();
        let recorded_hashes = self
            .rejected_findings
            .iter()
            .map(|record| record.hash.clone())
            .collect::<HashSet<_>>();
        for hash in self.rejected_hashes.clone() {
            if !recorded_hashes.contains(&hash) {
                self.rejected_findings.push(RejectedFindingRecord {
                    hash,
                    rejected_at: now,
                });
            }
        }
    }
}

/// Computes a deterministic hash from `(file, line_start, line_end, title)`.
///
/// **Limitation:** Because the hash anchors on line numbers, any edit that adds or
/// removes lines above a previously-rejected finding will shift its line numbers,
/// causing the stored hash to no longer match. The finding will then resurface on
/// the next review. A future improvement could incorporate surrounding code context
/// into the hash to make rejections resilient to line shifts.
fn rejection_hash(file: &str, line_start: usize, line_end: usize, title: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"gospel-review-rejection-v1");
    update_length_prefixed(&mut hasher, &normalize_file_anchor(file));
    hasher.update((line_start as u64).to_le_bytes());
    hasher.update((line_end as u64).to_le_bytes());
    update_length_prefixed(&mut hasher, title);
    hex_digest(hasher.finalize().as_slice())
}

fn normalize_file_anchor(file: &str) -> String {
    file.trim_start_matches("./").replace('\\', "/")
}

fn update_length_prefixed(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

fn write_store_atomically(path: &Path, content: &str) -> Result<(), String> {
    use std::io::Write;

    let parent_dir = path
        .parent()
        .ok_or_else(|| "Rejected findings store path has no parent directory".to_string())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "Rejected findings store path has no file name".to_string())?;
    let count = STORE_SAVE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(file_name);
    temp_name.push(format!(".tmp.{}.{}", std::process::id(), count));
    let temp_path = parent_dir.join(temp_name);

    let mut file = fs::File::create(&temp_path)
        .map_err(|e| format!("Failed to create temp rejected findings store: {}", e))?;
    file.write_all(content.as_bytes()).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        format!("Failed to write temp rejected findings store: {}", e)
    })?;
    file.sync_all().map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        format!("Failed to sync temp rejected findings store: {}", e)
    })?;
    drop(file);

    fs::rename(&temp_path, path).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        format!("Failed to replace rejected findings store: {}", e)
    })?;

    if let Ok(dir_handle) = fs::File::open(parent_dir) {
        let _ = dir_handle.sync_all();
    }

    Ok(())
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
    fn rejection_hash_uses_fixed_width_numeric_fields() {
        assert_eq!(
            rejection_hash("src/main.rs", 10, 12, "Unsanitized command"),
            "06efb2e1bee5eabec79e19dfa5cf2e8fa6f6ff5fd4f06bb2e1485699a2477bbe"
        );
    }

    #[test]
    fn missing_rejected_findings_store_loads_empty() {
        let dir = tempdir().unwrap();

        let store = AntiPatternStore::load(dir.path()).unwrap();

        assert!(store.rejected_hashes.is_empty());
        assert!(store.rejected_findings.is_empty());
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

    #[test]
    fn rejection_records_include_timestamps() {
        let mut store = AntiPatternStore::default();

        store.add_rejection("src/main.rs", 10, 12, "Unsanitized command");

        assert_eq!(store.rejected_findings.len(), 1);
        assert_eq!(
            store.rejected_findings[0].hash,
            rejection_hash("src/main.rs", 10, 12, "Unsanitized command")
        );
    }
}
