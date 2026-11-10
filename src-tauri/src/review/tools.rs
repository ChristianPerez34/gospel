use super::outcome::{record_review_outcome, ReviewOutcome};
use super::{
    multi, ChunkStatus, MultiFocusStatus, PhaseStatus, ReviewComment, ReviewEngine, ReviewFocus,
    ReviewMode, ReviewPhase, ReviewProgressEmitter, ReviewProgressEvent, ReviewRequest,
    ReviewResult, ToolEventKind,
};
use crate::subagent::{SubagentEmitter, SubagentKind, SubagentRelay, SubagentStep};
use crate::workspace_tools::WorkspaceToolError;
use crate::REJECTION_STORE_LOCK;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

pub const REVIEW_TOOLS_SYSTEM_PROMPT: &str = r#"
## Review Tools

Use `run_review` for a single-focus review (Security, BugHunt, Architecture, Performance, or Style) over local changes, a pull request, or the full workspace. Use `run_multi_review` to run all focuses in parallel for a comprehensive code review.

`run_security_review` is a deprecated compatibility alias. Prefer `run_review` or `run_multi_review` for new review requests.

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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunMultiReviewArgs {
    mode: String,
    pr_number: Option<u64>,
    #[serde(default)]
    focuses: Option<Vec<ReviewFocus>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RunReviewTool {
    workspace_root: PathBuf,
    provider: String,
    model: String,
    #[serde(skip_serializing, skip_deserializing)]
    api_key: String,
    #[serde(skip)]
    subagents: Option<Arc<SubagentRelay>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RunSecurityReviewTool {
    workspace_root: PathBuf,
    provider: String,
    model: String,
    #[serde(skip_serializing, skip_deserializing)]
    api_key: String,
    #[serde(skip)]
    subagents: Option<Arc<SubagentRelay>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RunMultiReviewTool {
    workspace_root: PathBuf,
    provider: String,
    model: String,
    #[serde(skip_serializing, skip_deserializing)]
    api_key: String,
    #[serde(skip)]
    subagents: Option<Arc<SubagentRelay>>,
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

impl fmt::Debug for RunMultiReviewTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunMultiReviewTool")
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

    fn description(&self) -> String {
        "Run Gospel's code review pipeline for a single review focus over local changes, a pull request, or the full workspace and return structured findings.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        review_tool_parameters(true)
    }

    fn map_error(&self, error: Self::Error) -> ToolExecutionError {
        ToolExecutionError::other(error.to_string())
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let progress = review_subagent_progress(
            self.subagents.as_ref(),
            Self::NAME,
            &args.mode,
            args.pr_number,
        );
        run_review_tool(
            self.workspace_root.clone(),
            self.provider.clone(),
            self.model.clone(),
            self.api_key.clone(),
            args.mode,
            args.pr_number,
            args.focus,
            progress,
        )
        .await
    }
}

impl Tool for RunMultiReviewTool {
    const NAME: &'static str = "run_multi_review";

    type Error = WorkspaceToolError;
    type Args = RunMultiReviewArgs;
    type Output = multi::MultiReviewResult;

    fn description(&self) -> String {
        "Run Gospel's code review pipeline across multiple focuses in parallel and return unified findings.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["local", "pr", "scan"],
                    "description": "Review mode: local staged/unstaged changes, a GitHub pull request, or a full workspace scan."
                },
                "pr_number": {
                    "type": "integer",
                    "description": "Required when mode is pr."
                },
                "focuses": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["Security", "BugHunt", "Architecture", "Performance", "Style"]
                    },
                    "minItems": 1,
                    "description": "Optional subset of focuses. Defaults to all focuses."
                }
            },
            "required": ["mode"]
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
        let progress = review_subagent_progress(
            self.subagents.as_ref(),
            Self::NAME,
            &args.mode,
            args.pr_number,
        );
        let focuses = args.focuses.unwrap_or_else(|| multi::ALL_FOCUSES.to_vec());
        let mode_enum =
            ReviewMode::parse(&args.mode, args.pr_number).map_err(WorkspaceToolError::Internal)?;

        let request = ReviewRequest::new(
            self.workspace_root.clone(),
            mode_enum,
            focuses,
            self.provider.clone(),
            self.model.clone(),
            self.api_key.clone(),
        );

        let engine = ReviewEngine::new();
        engine
            .execute_multi(request, progress)
            .await
            .map_err(WorkspaceToolError::Internal)
    }
}

impl Tool for RunSecurityReviewTool {
    const NAME: &'static str = "run_security_review";

    type Error = WorkspaceToolError;
    type Args = RunSecurityReviewArgs;
    type Output = RunReviewOutput;

    fn description(&self) -> String {
        "Deprecated alias for run_review with focus fixed to Security. Run Gospel's security review pipeline for local changes, a pull request, or the full workspace and return structured findings.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        review_tool_parameters(false)
    }

    fn map_error(&self, error: Self::Error) -> ToolExecutionError {
        ToolExecutionError::other(error.to_string())
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let progress = review_subagent_progress(
            self.subagents.as_ref(),
            Self::NAME,
            &args.mode,
            args.pr_number,
        );
        run_review_tool(
            self.workspace_root.clone(),
            self.provider.clone(),
            self.model.clone(),
            self.api_key.clone(),
            args.mode,
            args.pr_number,
            ReviewFocus::Security,
            progress,
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
                "description": "Review focus for this single invocation."
            }),
        );
    }

    json!({
        "type": "object",
        "properties": properties,
        "required": ["mode"]
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_review_tool(
    workspace_root: PathBuf,
    provider: String,
    model: String,
    api_key: String,
    mode: String,
    pr_number: Option<u64>,
    focus: ReviewFocus,
    progress: Arc<dyn ReviewProgressEmitter>,
) -> Result<RunReviewOutput, WorkspaceToolError> {
    let mode_enum = ReviewMode::parse(&mode, pr_number).map_err(WorkspaceToolError::Internal)?;

    let request = ReviewRequest::new(
        workspace_root,
        mode_enum,
        vec![focus],
        provider,
        model,
        api_key,
    );

    let engine = ReviewEngine::new();
    match engine.execute(request, progress).await {
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

    fn description(&self) -> String {
        "Record whether a review finding was accepted or rejected. Validates the run_id and comment_id before writing outcome storage.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
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
    subagents: Option<Arc<SubagentRelay>>,
) -> RunSecurityReviewTool {
    RunSecurityReviewTool {
        workspace_root,
        provider,
        model,
        api_key,
        subagents,
    }
}

pub fn create_run_review_tool(
    workspace_root: PathBuf,
    provider: String,
    model: String,
    api_key: String,
    subagents: Option<Arc<SubagentRelay>>,
) -> RunReviewTool {
    RunReviewTool {
        workspace_root,
        provider,
        model,
        api_key,
        subagents,
    }
}

pub fn create_run_multi_review_tool(
    workspace_root: PathBuf,
    provider: String,
    model: String,
    api_key: String,
    subagents: Option<Arc<SubagentRelay>>,
) -> RunMultiReviewTool {
    RunMultiReviewTool {
        workspace_root,
        provider,
        model,
        api_key,
        subagents,
    }
}

pub fn create_record_review_outcome_tool(workspace_root: PathBuf) -> RecordReviewOutcomeTool {
    RecordReviewOutcomeTool { workspace_root }
}

fn review_subagent_progress(
    subagents: Option<&Arc<SubagentRelay>>,
    tool_name: &str,
    mode: &str,
    pr_number: Option<u64>,
) -> Arc<dyn ReviewProgressEmitter> {
    Arc::new(SubagentReviewProgress(SubagentEmitter::claim(
        subagents,
        tool_name,
        &json!({ "mode": mode, "pr_number": pr_number }),
        SubagentKind::Review,
    )))
}

/// Presents review pipeline progress as live sub-agent steps on the parent
/// review tool call.
struct SubagentReviewProgress(SubagentEmitter);

impl ReviewProgressEmitter for SubagentReviewProgress {
    fn emit_progress(&self, event: ReviewProgressEvent) {
        if let Some(step) = review_progress_step(&event) {
            self.0.emit(step);
        }
    }
}

fn focus_label(focus: ReviewFocus) -> &'static str {
    match focus {
        ReviewFocus::Security => "Security",
        ReviewFocus::BugHunt => "Bug hunt",
        ReviewFocus::Architecture => "Architecture",
        ReviewFocus::Performance => "Performance",
        ReviewFocus::Style => "Style",
    }
}

fn review_progress_step(event: &ReviewProgressEvent) -> Option<SubagentStep> {
    let focus = event.focus.map(focus_label);
    let scoped = |label: String| match focus {
        Some(focus) => format!("{focus}: {label}"),
        None => label,
    };
    let phase = |label: String| Some(SubagentStep::Phase { label });
    let focus_key = event
        .focus
        .map(|focus| focus.to_string())
        .unwrap_or_default();
    let tool_step = |scope: String, tool_name: &str, kind: &ToolEventKind| match kind {
        ToolEventKind::Call { id, arguments } => SubagentStep::ToolCall {
            id: format!("{focus_key}:{scope}:{id}"),
            name: tool_name.to_string(),
            arguments: arguments.clone(),
        },
        ToolEventKind::Result { id, summary } => SubagentStep::ToolResult {
            id: format!("{focus_key}:{scope}:{id}"),
            name: tool_name.to_string(),
            result: crate::subagent::capped_result(summary),
        },
    };
    match &event.phase {
        ReviewPhase::Detector {
            chunk: 0,
            status: ChunkStatus::Starting,
            ..
        } => phase(scoped("starting review".to_string())),
        ReviewPhase::Detector {
            chunk,
            total_chunks,
            status: ChunkStatus::Running,
            ..
        } => phase(scoped(format!("analyzing chunk {chunk} of {total_chunks}"))),
        ReviewPhase::Detector {
            chunk,
            status: ChunkStatus::Failed { .. },
            ..
        } => phase(scoped(format!("chunk {chunk} failed"))),
        ReviewPhase::Validator {
            candidate_count,
            status: PhaseStatus::Running,
        } => phase(scoped(format!(
            "validating {candidate_count} candidate{}",
            if *candidate_count == 1 { "" } else { "s" }
        ))),
        ReviewPhase::DetectorTool {
            chunk,
            tool_name,
            event,
        } => Some(tool_step(format!("d{chunk}"), tool_name, event)),
        ReviewPhase::ValidatorTool { tool_name, event } => {
            Some(tool_step("v".to_string(), tool_name, event))
        }
        ReviewPhase::Finalize {
            status: PhaseStatus::Running,
        } => phase(scoped("finalizing findings".to_string())),
        ReviewPhase::Done { findings, .. } => phase(scoped(format!(
            "{findings} finding{}",
            if *findings == 1 { "" } else { "s" }
        ))),
        ReviewPhase::MultiFocusStart { total } => phase(format!("Reviewing {total} focus areas")),
        ReviewPhase::MultiFocus {
            focus,
            status: MultiFocusStatus::Failed { .. },
            ..
        } => phase(format!("{}: review failed", focus_label(*focus))),
        ReviewPhase::Failed { .. } => phase(scoped("review failed".to_string())),
        _ => None,
    }
}

#[cfg(test)]
mod subagent_progress_tests {
    use super::*;

    #[test]
    fn detector_tool_events_become_scoped_tool_steps() {
        let call = ReviewProgressEvent::new_with_focus(
            "run",
            ReviewFocus::BugHunt,
            ReviewPhase::DetectorTool {
                chunk: 2,
                tool_name: "read_file".into(),
                event: ToolEventKind::Call {
                    id: "c1".into(),
                    arguments: json!({"path": "src/lib.rs"}),
                },
            },
        );
        assert_eq!(
            review_progress_step(&call),
            Some(SubagentStep::ToolCall {
                id: "BugHunt:d2:c1".into(),
                name: "read_file".into(),
                arguments: json!({"path": "src/lib.rs"}),
            })
        );
    }

    #[test]
    fn stage_events_become_readable_phases() {
        let running = ReviewProgressEvent::new_with_focus(
            "run",
            ReviewFocus::Security,
            ReviewPhase::Detector {
                chunk: 1,
                total_chunks: 3,
                files: vec![],
                candidate_count: 0,
                status: ChunkStatus::Running,
            },
        );
        assert_eq!(
            review_progress_step(&running),
            Some(SubagentStep::Phase {
                label: "Security: analyzing chunk 1 of 3".into()
            })
        );
        let done_chunk = ReviewProgressEvent::new(
            "run",
            ReviewPhase::Detector {
                chunk: 1,
                total_chunks: 3,
                files: vec![],
                candidate_count: 2,
                status: ChunkStatus::Done,
            },
        );
        assert_eq!(review_progress_step(&done_chunk), None);
    }
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
            None,
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
            None,
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
            None,
        );
        let alias = create_run_security_review_tool(
            PathBuf::from("/workspace"),
            "openai".to_string(),
            "gpt-test".to_string(),
            "sk-secret-value".to_string(),
            None,
        );

        let canonical_parameters = Tool::parameters(&canonical);
        let alias_parameters = Tool::parameters(&alias);

        assert!(canonical_parameters.to_string().contains("focus"));
        assert_eq!(
            canonical_parameters["properties"]["focus"]["enum"],
            json!([
                "Security",
                "BugHunt",
                "Architecture",
                "Performance",
                "Style"
            ])
        );
        assert!(!alias_parameters.to_string().contains("focus"));
    }

    #[tokio::test]
    async fn run_multi_review_tool_definition_exposes_all_focuses() {
        let tool = create_run_multi_review_tool(
            PathBuf::from("/workspace"),
            "openai".to_string(),
            "gpt-test".to_string(),
            "sk-secret-value".to_string(),
            None,
        );

        let parameters = Tool::parameters(&tool);

        assert_eq!(
            parameters["properties"]["focuses"]["items"]["enum"],
            json!([
                "Security",
                "BugHunt",
                "Architecture",
                "Performance",
                "Style"
            ])
        );
    }
}
