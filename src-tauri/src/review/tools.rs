use super::outcome::{record_review_outcome, ReviewOutcome};
use super::{run_review, ReviewComment, ReviewConfig, ReviewFocus, ReviewResult};
use crate::workspace_tools::WorkspaceToolError;
use crate::REJECTION_STORE_LOCK;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;
use std::path::PathBuf;

pub const REVIEW_TOOLS_SYSTEM_PROMPT: &str = r#"
## Review Tools

Use `run_review` when the user asks to review local changes, a pull request, or the full workspace. Set `focus` to `Security` for security findings.

`run_security_review` is a deprecated compatibility alias. It always runs the `Security` focus and must not be used for other focuses.

When narrating review results:
- Prefix each visible finding with its stable index, e.g. `[1]`, `[2]`.
- Use the returned `comment_id` when recording accept/reject outcomes.
- If the user says "accept finding 2" or "reject finding 2", map index 2 to that finding's `comment_id` and call `record_review_outcome`.
- Mention suppression only when `suppressed_count` is greater than 0.
"#;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunReviewArgs {
    mode: String,
    pr_number: Option<u64>,
    #[serde(default)]
    focus: ReviewFocus,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSecurityReviewArgs {
    mode: String,
    pr_number: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RunReviewTool {
    workspace_root: PathBuf,
    provider: String,
    model: String,
    #[serde(skip_serializing, skip_deserializing)]
    api_key: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RunSecurityReviewTool {
    workspace_root: PathBuf,
    provider: String,
    model: String,
    #[serde(skip_serializing, skip_deserializing)]
    api_key: String,
}

impl fmt::Debug for RunReviewTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunReviewTool")
            .field("workspace_root", &self.workspace_root)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("api_key", &"REDACTED")
            .finish()
    }
}

impl fmt::Debug for RunSecurityReviewTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunSecurityReviewTool")
            .field("workspace_root", &self.workspace_root)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("api_key", &"REDACTED")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexedReviewComment {
    pub index: usize,
    #[serde(flatten)]
    pub comment: ReviewComment,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunReviewOutput {
    pub success: bool,
    pub message: String,
    pub reason: Option<String>,
    pub review: Option<ReviewResult>,
    pub findings: Vec<IndexedReviewComment>,
}

impl Tool for RunReviewTool {
    const NAME: &'static str = "run_review";

    type Error = WorkspaceToolError;
    type Args = RunReviewArgs;
    type Output = RunReviewOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Run Gospel's code review pipeline for a single review focus over local changes, a pull request, or the full workspace and return structured findings.".to_string(),
            parameters: review_tool_parameters(true),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        run_review_tool(
            self.workspace_root.clone(),
            self.provider.clone(),
            self.model.clone(),
            self.api_key.clone(),
            args.mode,
            args.pr_number,
            args.focus,
        )
        .await
    }
}

impl Tool for RunSecurityReviewTool {
    const NAME: &'static str = "run_security_review";

    type Error = WorkspaceToolError;
    type Args = RunSecurityReviewArgs;
    type Output = RunReviewOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Deprecated alias for run_review with focus fixed to Security. Run Gospel's security review pipeline for local changes, a pull request, or the full workspace and return structured findings.".to_string(),
            parameters: review_tool_parameters(false),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        run_review_tool(
            self.workspace_root.clone(),
            self.provider.clone(),
            self.model.clone(),
            self.api_key.clone(),
            args.mode,
            args.pr_number,
            ReviewFocus::Security,
        )
        .await
    }
}

fn review_tool_parameters(include_focus: bool) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    properties.insert(
        "mode".to_string(),
        json!({
            "type": "string",
            "enum": ["local", "pr", "scan"],
            "description": "Review mode: local staged/unstaged changes, a GitHub pull request, or a full workspace scan."
        }),
    );
    properties.insert(
        "pr_number".to_string(),
        json!({
            "type": "integer",
            "description": "Required when mode is pr."
        }),
    );
    if include_focus {
        properties.insert(
            "focus".to_string(),
            json!({
                "type": "string",
                "enum": ["Security", "BugHunt", "Architecture", "Performance", "Style"],
                "description": "Review focus for this single invocation. Defaults to Security."
            }),
        );
    }

    json!({
        "type": "object",
        "properties": properties,
        "required": ["mode"]
    })
}

async fn run_review_tool(
    workspace_root: PathBuf,
    provider: String,
    model: String,
    api_key: String,
    mode: String,
    pr_number: Option<u64>,
    focus: ReviewFocus,
) -> Result<RunReviewOutput, WorkspaceToolError> {
    let config = ReviewConfig {
        provider,
        model,
        mode,
        focus,
        pr_number,
    };

    match run_review(config, workspace_root, api_key).await {
        Ok(review) => {
            let findings = review
                .comments
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, comment)| IndexedReviewComment {
                    index: index + 1,
                    comment,
                })
                .collect();
            Ok(RunReviewOutput {
                success: true,
                message: review.summary.clone(),
                reason: None,
                review: Some(review),
                findings,
            })
        }
        Err(error) => Ok(RunReviewOutput {
            success: false,
            message: format!("{} review failed: {}", focus, error),
            reason: Some("review_failed".to_string()),
            review: None,
            findings: Vec::new(),
        }),
    }
}

#[derive(Debug, Deserialize)]
pub struct RecordReviewOutcomeArgs {
    run_id: String,
    comment_id: String,
    outcome: ReviewOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordReviewOutcomeTool {
    workspace_root: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordReviewOutcomeToolOutput {
    pub success: bool,
    pub message: String,
    pub reason: Option<String>,
    pub run_id: String,
    pub comment_id: String,
    pub outcome: Option<ReviewOutcome>,
}

impl Tool for RecordReviewOutcomeTool {
    const NAME: &'static str = "record_review_outcome";

    type Error = WorkspaceToolError;
    type Args = RecordReviewOutcomeArgs;
    type Output = RecordReviewOutcomeToolOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Record whether a review finding was accepted or rejected. Validates the run_id and comment_id before writing outcome storage.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "run_id": {
                        "type": "string",
                        "description": "The run_id returned by run_review."
                    },
                    "comment_id": {
                        "type": "string",
                        "description": "The stable comment_id for the finding."
                    },
                    "outcome": {
                        "type": "string",
                        "enum": ["accepted", "rejected"]
                    }
                },
                "required": ["run_id", "comment_id", "outcome"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _guard = REJECTION_STORE_LOCK.lock().await;
        match record_review_outcome(
            &self.workspace_root,
            &args.run_id,
            &args.comment_id,
            args.outcome,
        ) {
            Ok(output) => Ok(RecordReviewOutcomeToolOutput {
                success: true,
                message: output.message,
                reason: None,
                run_id: output.run_id,
                comment_id: output.comment_id,
                outcome: Some(output.outcome),
            }),
            Err(error) => Ok(RecordReviewOutcomeToolOutput {
                success: false,
                message: error,
                reason: Some("invalid_review_outcome".to_string()),
                run_id: args.run_id,
                comment_id: args.comment_id,
                outcome: None,
            }),
        }
    }
}

pub fn create_run_security_review_tool(
    workspace_root: PathBuf,
    provider: String,
    model: String,
    api_key: String,
) -> RunSecurityReviewTool {
    RunSecurityReviewTool {
        workspace_root,
        provider,
        model,
        api_key,
    }
}

pub fn create_run_review_tool(
    workspace_root: PathBuf,
    provider: String,
    model: String,
    api_key: String,
) -> RunReviewTool {
    RunReviewTool {
        workspace_root,
        provider,
        model,
        api_key,
    }
}

pub fn create_record_review_outcome_tool(workspace_root: PathBuf) -> RecordReviewOutcomeTool {
    RecordReviewOutcomeTool { workspace_root }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_security_review_tool_redacts_api_key_from_debug_and_serialization() {
        let tool = create_run_security_review_tool(
            PathBuf::from("/workspace"),
            "openai".to_string(),
            "gpt-test".to_string(),
            "sk-secret-value".to_string(),
        );

        let debug = format!("{:?}", tool);
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("sk-secret-value"));

        let serialized = serde_json::to_string(&tool).unwrap();
        assert!(!serialized.contains("api_key"));
        assert!(!serialized.contains("sk-secret-value"));
    }

    #[test]
    fn run_review_tool_redacts_api_key_from_debug_and_serialization() {
        let tool = create_run_review_tool(
            PathBuf::from("/workspace"),
            "openai".to_string(),
            "gpt-test".to_string(),
            "sk-secret-value".to_string(),
        );

        let debug = format!("{:?}", tool);
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("sk-secret-value"));

        let serialized = serde_json::to_string(&tool).unwrap();
        assert!(!serialized.contains("api_key"));
        assert!(!serialized.contains("sk-secret-value"));
    }

    #[test]
    fn run_security_review_alias_rejects_focus_argument() {
        let parsed = serde_json::from_value::<RunSecurityReviewArgs>(json!({
            "mode": "local",
            "focus": "BugHunt"
        }));

        assert!(parsed.is_err());
    }

    #[tokio::test]
    async fn run_review_tool_definition_exposes_focus_but_alias_does_not() {
        let canonical = create_run_review_tool(
            PathBuf::from("/workspace"),
            "openai".to_string(),
            "gpt-test".to_string(),
            "sk-secret-value".to_string(),
        );
        let alias = create_run_security_review_tool(
            PathBuf::from("/workspace"),
            "openai".to_string(),
            "gpt-test".to_string(),
            "sk-secret-value".to_string(),
        );

        let canonical_parameters = canonical.definition(String::new()).await.parameters;
        let alias_parameters = alias.definition(String::new()).await.parameters;

        assert!(canonical_parameters.to_string().contains("focus"));
        assert!(!alias_parameters.to_string().contains("focus"));
    }
}
