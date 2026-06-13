use super::signal::{PartialSignalRules, SignalRules};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

pub const DEFAULT_NOISE_THRESHOLD_PERCENT: f64 = 60.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceReviewConfig {
    pub noise_threshold_percent: f64,
    pub signal_rules: SignalRules,
}

impl Default for WorkspaceReviewConfig {
    fn default() -> Self {
        Self {
            noise_threshold_percent: DEFAULT_NOISE_THRESHOLD_PERCENT,
            signal_rules: SignalRules::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PartialWorkspaceReviewConfig {
    noise_threshold_percent: Option<f64>,
    signal_rules: Option<PartialSignalRules>,
}

impl WorkspaceReviewConfig {
    fn merge(&mut self, partial: PartialWorkspaceReviewConfig) {
        if let Some(threshold) = partial.noise_threshold_percent {
            self.noise_threshold_percent = threshold.clamp(0.0, 100.0);
        }

        if let Some(signal_rules) = partial.signal_rules {
            self.signal_rules.merge(signal_rules);
        }
    }
}

pub fn load_workspace_review_config(
    workspace_path: &Path,
) -> Result<WorkspaceReviewConfig, String> {
    let path = workspace_path.join(".gospel").join("review_config.json");
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(WorkspaceReviewConfig::default());
        }
        Err(error) => {
            return Err(format!(
                "Failed to read review config {}: {}",
                path.display(),
                error
            ));
        }
    };

    let partial: PartialWorkspaceReviewConfig =
        serde_json::from_str(&content).map_err(|error| {
            format!(
                "Failed to parse review config {}: {}",
                path.display(),
                error
            )
        })?;
    let mut config = WorkspaceReviewConfig::default();
    config.merge(partial);
    Ok(config)
}

pub fn load_workspace_review_config_with_warnings(
    workspace_path: &Path,
    warnings: &mut Vec<String>,
) -> WorkspaceReviewConfig {
    match load_workspace_review_config(workspace_path) {
        Ok(config) => config,
        Err(error) => {
            let warning = format!(
                "Ignored review config because it could not be loaded: {}",
                error
            );
            tracing::warn!("{}", warning);
            warnings.push(warning);
            WorkspaceReviewConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_review_config_loads_defaults() {
        let dir = tempdir().unwrap();

        let config = load_workspace_review_config(dir.path()).unwrap();

        assert_eq!(
            config.noise_threshold_percent,
            DEFAULT_NOISE_THRESHOLD_PERCENT
        );
        assert!(config
            .signal_rules
            .tier1_cwes
            .contains(&"CWE-78".to_string()));
    }

    #[test]
    fn review_config_merges_workspace_overrides_with_defaults() {
        let dir = tempdir().unwrap();
        let gospel_dir = dir.path().join(".gospel");
        fs::create_dir_all(&gospel_dir).unwrap();
        fs::write(
            gospel_dir.join("review_config.json"),
            r#"{
              "noise_threshold_percent": 42,
              "signal_rules": {
                "tier1_cwes": ["CWE-999"],
                "noise_categories": ["cosmetic"]
              }
            }"#,
        )
        .unwrap();

        let config = load_workspace_review_config(dir.path()).unwrap();

        assert_eq!(config.noise_threshold_percent, 42.0);
        assert!(config
            .signal_rules
            .tier1_cwes
            .contains(&"CWE-78".to_string()));
        assert!(config
            .signal_rules
            .tier1_cwes
            .contains(&"CWE-999".to_string()));
        assert!(config
            .signal_rules
            .noise_categories
            .contains(&"cosmetic".to_string()));
    }
}
