use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use sha2::{Sha256, Digest};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AntiPatternStore {
    pub rejected_hashes: HashSet<String>,
}

impl AntiPatternStore {
    pub fn load(workspace_path: &Path) -> Self {
        let path = workspace_path.join(".gospel").join("rejected_findings.json");
        if let Ok(content) = fs::read_to_string(path) {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
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

    pub fn add_rejection(&mut self, file: &str, title: &str, evidence: &str) {
        let mut hasher = Sha256::new();
        hasher.update(file);
        hasher.update(title);
        hasher.update(evidence);
        let hash = hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>();
        self.rejected_hashes.insert(hash);
    }

    pub fn is_rejected(&self, file: &str, title: &str, evidence: &str) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(file);
        hasher.update(title);
        hasher.update(evidence);
        let hash = hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>();
        self.rejected_hashes.contains(&hash)
    }
}
