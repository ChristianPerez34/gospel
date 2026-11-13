use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use thiserror::Error;

const PENDING_FILE: &str = "checkpoint.pending";
const APPROVED_FILE: &str = "checkpoint.approved";
const THREAT_MODEL_FILE: &str = "threat-model.md";

#[derive(Error, Debug)]
pub enum BountyCheckpointError {
    #[error("failed to write checkpoint artifacts: {0}")]
    Io(String),
}

/// Human gate between recon and broad testing. Once the agent calls this, the
/// pending marker makes `http_request` refuse until `approve_bounty_checkpoint`
/// writes the approved marker — so recon-freeze is capability-enforced, not
/// prompt-enforced.
#[derive(Debug, Clone)]
pub struct BountyCheckpointTool {
    run_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct BountyCheckpointArgs {
    /// Full threat-model markdown, written to `threat-model.md` in the run dir.
    pub threat_model: String,
    /// One-line summary shown in the pending marker and tool output.
    pub summary: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BountyCheckpointOutput {
    pub pending: bool,
    pub approved: bool,
    pub threat_model_path: String,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointState {
    None,
    Pending,
    Approved,
}

pub fn checkpoint_state(run_dir: &Path) -> CheckpointState {
    if run_dir.join(APPROVED_FILE).exists() {
        CheckpointState::Approved
    } else if run_dir.join(PENDING_FILE).exists() {
        CheckpointState::Pending
    } else {
        CheckpointState::None
    }
}

/// Called by the `approve_bounty_checkpoint` command.
pub fn approve_checkpoint(run_dir: &Path) -> Result<(), String> {
    match checkpoint_state(run_dir) {
        CheckpointState::Approved => Ok(()),
        CheckpointState::Pending => {
            std::fs::write(run_dir.join(APPROVED_FILE), chrono::Utc::now().to_rfc3339())
                .map_err(|error| error.to_string())
        }
        CheckpointState::None => {
            Err("no pending checkpoint for this run".to_string())
        }
    }
}

impl BountyCheckpointTool {
    pub fn new(run_dir: PathBuf) -> Self {
        Self { run_dir }
    }
}

impl Tool for BountyCheckpointTool {
    const NAME: &'static str = "bounty_checkpoint";

    type Error = BountyCheckpointError;
    type Args = BountyCheckpointArgs;
    type Output = BountyCheckpointOutput;

    fn description(&self) -> String {
        "Declare recon complete and request human review of the threat model. Writes threat-model.md and a pending marker; http_request refuses until a human approves the checkpoint. Call exactly once, after recon, before broad testing.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "threat_model": {
                    "type": "string",
                    "description": "Full threat-model markdown: assets, trust boundaries, ranked hypotheses."
                },
                "summary": {
                    "type": "string",
                    "description": "One-line summary of what broad testing will cover."
                }
            },
            "required": ["threat_model"]
        })
    }

    fn map_error(&self, error: Self::Error) -> ToolExecutionError {
        ToolExecutionError::other(error.to_string())
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        if args.threat_model.trim().is_empty() {
            return Err(BountyCheckpointError::Io(
                "threat_model must not be empty".to_string(),
            ));
        }
        if checkpoint_state(&self.run_dir) != CheckpointState::None {
            return Err(BountyCheckpointError::Io(
                "a checkpoint already exists for this run".to_string(),
            ));
        }
        let threat_model_path = self.run_dir.join(THREAT_MODEL_FILE);
        std::fs::create_dir_all(&self.run_dir)
            .and_then(|_| std::fs::write(&threat_model_path, &args.threat_model))
            .map_err(|error| BountyCheckpointError::Io(error.to_string()))?;
        let pending = serde_json::json!({
            "summary": args.summary,
            "ts": chrono::Utc::now().to_rfc3339(),
        });
        std::fs::write(self.run_dir.join(PENDING_FILE), pending.to_string())
            .map_err(|error| BountyCheckpointError::Io(error.to_string()))?;
        Ok(BountyCheckpointOutput {
            pending: true,
            approved: false,
            threat_model_path: threat_model_path.to_string_lossy().into_owned(),
            summary: "Checkpoint pending human approval. http_request is refused until the user approves it (Bounty findings panel → Approve checkpoint). Stop here and wait.".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn checkpoint_pending_then_approved() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BountyCheckpointTool::new(dir.path().to_path_buf());
        let output = tool
            .call(
                &mut ToolContext::default(),
                BountyCheckpointArgs {
                    threat_model: "# threat model".to_string(),
                    summary: Some("test admin paths".to_string()),
                },
            )
            .await
            .unwrap();
        assert!(output.pending && !output.approved);
        assert_eq!(checkpoint_state(dir.path()), CheckpointState::Pending);
        assert!(dir.path().join(THREAT_MODEL_FILE).exists());

        approve_checkpoint(dir.path()).unwrap();
        assert_eq!(checkpoint_state(dir.path()), CheckpointState::Approved);
        // Second call is refused once a checkpoint exists.
        assert!(tool
            .call(
                &mut ToolContext::default(),
                BountyCheckpointArgs {
                    threat_model: "# again".to_string(),
                    summary: None,
                },
            )
            .await
            .is_err());
    }

    #[test]
    fn approve_requires_pending() {
        let dir = tempfile::tempdir().unwrap();
        assert!(approve_checkpoint(dir.path()).is_err());
    }
}
