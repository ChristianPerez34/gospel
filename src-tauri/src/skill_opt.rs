//! Offline skill optimization: bounded patches, harvest, Selection Gate, staging.

use crate::harness_profile::ActiveWorkspaceContext;
use crate::skills::SkillFrontmatter;
use crate::verification::VerificationStatus;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const DEFAULT_EDIT_BUDGET: usize = 4;
pub const SLOW_UPDATE_OPEN: &str = "<!-- gospel:slow-update -->";
pub const SLOW_UPDATE_CLOSE: &str = "<!-- /gospel:slow-update -->";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SkillEdit {
    Append { text: String },
    Insert { after: String, text: String },
    Replace { old: String, new: String },
    Delete { text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditSkipReason {
    Empty,
    NoMatch,
    AmbiguousMatch,
    NoOp,
    ProtectedRegion,
    OverBudget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedEdit {
    pub edit: SkillEdit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedEdit {
    pub edit: SkillEdit,
    pub reason: EditSkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyEditsResult {
    pub skill_md: String,
    pub applied: Vec<AppliedEdit>,
    pub skipped: Vec<SkippedEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyEditsError {
    MissingFrontmatter,
}

impl std::fmt::Display for ApplyEditsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyEditsError::MissingFrontmatter => {
                write!(f, "skill document is missing YAML frontmatter")
            }
        }
    }
}

impl std::error::Error for ApplyEditsError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarvestedTranscript {
    pub session_id: String,
    pub display_transcript: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillTask {
    pub session_id: String,
    pub prompt: String,
    pub tool_names: Vec<String>,
}

const SLASH_PATTERN: &str = r"^/([a-zA-Z0-9-]+)(?:[ \t]+([\s\S]*))?$";

pub fn invoked_preamble_from_skill_md(
    name: &str,
    skill_md: &str,
) -> Result<String, ApplyEditsError> {
    let (_, body) = split_skill_document(skill_md)?;
    Ok(format!("## Invoked Skill: {}\n\n{}", name, body.trim()))
}

pub fn score_verification(status: VerificationStatus) -> Option<f64> {
    match status {
        VerificationStatus::Pass => Some(1.0),
        VerificationStatus::Concerns => Some(0.5),
        VerificationStatus::Fail => Some(0.0),
        VerificationStatus::Unavailable => None,
    }
}

pub fn paired_scores(
    current: &[VerificationStatus],
    candidate: &[VerificationStatus],
) -> (Vec<f64>, Vec<f64>) {
    current
        .iter()
        .cloned()
        .zip(candidate.iter().cloned())
        .filter_map(|(current_status, candidate_status)| {
            Some((
                score_verification(current_status)?,
                score_verification(candidate_status)?,
            ))
        })
        .unzip()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudentTurn {
    pub effective_prompt: String,
    pub invoked_skill_section: String,
    pub matched_skills_section: Option<String>,
}

pub fn prepare_student_turn(
    name: &str,
    skill_md: &str,
    task_prompt: &str,
) -> Result<StudentTurn, ApplyEditsError> {
    Ok(StudentTurn {
        effective_prompt: task_prompt.to_string(),
        invoked_skill_section: invoked_preamble_from_skill_md(name, skill_md)?,
        matched_skills_section: None,
    })
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SkillReplayError {
    #[error("student turn is invalid")]
    InvalidStudentTurn,
    #[error("skill document is missing YAML frontmatter")]
    MissingFrontmatter,
    #[error("student replay failed: {0}")]
    Failed(String),
    #[error("selection split is empty")]
    EmptySelection,
    #[error("optimizer proposed no usable edits")]
    NoEditsProposed,
}

impl From<ApplyEditsError> for SkillReplayError {
    fn from(error: ApplyEditsError) -> Self {
        match error {
            ApplyEditsError::MissingFrontmatter => SkillReplayError::MissingFrontmatter,
        }
    }
}

pub trait SkillReplayExecutor {
    fn execute(
        &self,
        turn: &StudentTurn,
    ) -> impl std::future::Future<Output = Result<String, SkillReplayError>> + Send;
}

pub trait SkillReplayVerifier {
    fn verify(
        &self,
        prompt: &str,
        response: &str,
    ) -> impl std::future::Future<Output = VerificationStatus> + Send;
}

pub trait StudentLlm {
    fn complete(
        &self,
        prompt: &str,
        invoked_skill_section: &str,
    ) -> impl std::future::Future<Output = Result<String, SkillReplayError>> + Send;
}

impl<T: StudentLlm + Sync> StudentLlm for &T {
    fn complete(
        &self,
        prompt: &str,
        invoked_skill_section: &str,
    ) -> impl std::future::Future<Output = Result<String, SkillReplayError>> + Send {
        (*self).complete(prompt, invoked_skill_section)
    }
}

pub struct LlmStudentExecutor<L> {
    pub llm: L,
}

impl<L: StudentLlm + Sync> SkillReplayExecutor for LlmStudentExecutor<L> {
    async fn execute(&self, turn: &StudentTurn) -> Result<String, SkillReplayError> {
        if turn.matched_skills_section.is_some() {
            return Err(SkillReplayError::InvalidStudentTurn);
        }
        if turn.invoked_skill_section.trim().is_empty() {
            return Err(SkillReplayError::InvalidStudentTurn);
        }
        self.llm
            .complete(&turn.effective_prompt, &turn.invoked_skill_section)
            .await
    }
}

pub struct StreamCompletionStudentLlm {
    pub provider: String,
    pub model: String,
    pub variant: Option<String>,
    pub api_key: String,
    pub workspace: ActiveWorkspaceContext,
}

impl StudentLlm for StreamCompletionStudentLlm {
    async fn complete(
        &self,
        prompt: &str,
        invoked_skill_section: &str,
    ) -> Result<String, SkillReplayError> {
        let mut workspace = self.workspace.clone();
        workspace.session_mode = crate::session_mode::SessionMode::ReadOnly;
        let result = crate::llm::stream_completion(
            &self.provider,
            prompt,
            &self.model,
            self.variant.as_deref(),
            &self.api_key,
            &self.provider,
            &self.model,
            &self.api_key,
            Some(workspace),
            None,
            None,
            Vec::new(),
            None,
            Some(invoked_skill_section.to_string()),
            None,
            None,
            |_| {},
        )
        .await
        .map_err(|error| SkillReplayError::Failed(error.to_string()))?;
        Ok(result.full_response)
    }
}

pub async fn replay_skill_tasks<E, V>(
    name: &str,
    skill_md: &str,
    tasks: &[SkillTask],
    executor: &E,
    verifier: &V,
) -> Result<Vec<VerificationStatus>, SkillReplayError>
where
    E: SkillReplayExecutor + Sync,
    V: SkillReplayVerifier + Sync,
{
    let mut statuses = Vec::with_capacity(tasks.len());
    for task in tasks {
        let turn = prepare_student_turn(name, skill_md, &task.prompt)?;
        let response = executor.execute(&turn).await?;
        statuses.push(verifier.verify(&turn.effective_prompt, &response).await);
    }
    Ok(statuses)
}

#[allow(clippy::too_many_arguments)]
pub async fn replay_and_conclude<E, V>(
    workspace: &Path,
    name: &str,
    current_skill_md: &str,
    candidate_skill_md: &str,
    edits: &[SkillEdit],
    tasks: &[SkillTask],
    executor: &E,
    verifier: &V,
) -> Result<OptimizationConclusion, SkillReplayError>
where
    E: SkillReplayExecutor + Sync,
    V: SkillReplayVerifier + Sync,
{
    let current_statuses =
        replay_skill_tasks(name, current_skill_md, tasks, executor, verifier).await?;
    let candidate_statuses =
        replay_skill_tasks(name, candidate_skill_md, tasks, executor, verifier).await?;
    let (current_scores, candidate_scores) = paired_scores(&current_statuses, &candidate_statuses);
    conclude_optimization(
        workspace,
        name,
        candidate_skill_md,
        edits,
        &current_scores,
        &candidate_scores,
    )
    .map_err(|error| SkillReplayError::Failed(error.to_string()))
}

#[allow(clippy::too_many_arguments)]
pub async fn run_selection_replay<E, V>(
    workspace: &Path,
    name: &str,
    current_skill_md: &str,
    candidate_skill_md: &str,
    edits: &[SkillEdit],
    tasks: &[SkillTask],
    executor: &E,
    verifier: &V,
) -> Result<OptimizationConclusion, SkillReplayError>
where
    E: SkillReplayExecutor + Sync,
    V: SkillReplayVerifier + Sync,
{
    let (_train, selection) = split_skill_tasks(tasks);
    if selection.is_empty() {
        return Err(SkillReplayError::EmptySelection);
    }
    replay_and_conclude(
        workspace,
        name,
        current_skill_md,
        candidate_skill_md,
        edits,
        &selection,
        executor,
        verifier,
    )
    .await
}

pub trait SkillOptimizer {
    fn propose_edits(
        &self,
        prompt: &str,
    ) -> impl std::future::Future<Output = Result<String, SkillReplayError>> + Send;
}

pub fn parse_optimizer_edits(raw: &str) -> Result<Vec<SkillEdit>, SkillReplayError> {
    let json = extract_json_payload(raw);
    serde_json::from_str(json).map_err(|error| SkillReplayError::Failed(error.to_string()))
}

fn extract_json_payload(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            if end >= start {
                return &trimmed[start..=end];
            }
        }
    }
    trimmed
}

pub fn build_optimizer_prompt(
    skill_md: &str,
    train_tasks: &[SkillTask],
    rejected: &[RejectedEditRecord],
) -> String {
    let mut prompt = String::from(
        "Propose at most 4 bounded add/delete/replace/insert/append edits to this SKILL.md. Return a JSON array of edits only. Prefer reusable procedures over instance-specific examples.\n\n",
    );
    prompt.push_str("## Current skill\n\n");
    prompt.push_str(skill_md);
    prompt.push_str("\n\n## Train tasks\n");
    for task in train_tasks {
        prompt.push_str("\n- prompt: ");
        prompt.push_str(&task.prompt);
        if !task.tool_names.is_empty() {
            prompt.push_str("\n  tools: ");
            prompt.push_str(&task.tool_names.join(", "));
        }
    }
    if !rejected.is_empty() {
        prompt.push_str("\n\n## Rejected edits (do not repeat)\n");
        if let Ok(json) = serde_json::to_string_pretty(rejected) {
            prompt.push_str(&json);
        }
    }
    prompt
}

pub async fn optimize_and_replay<O, E, V>(
    workspace: &Path,
    name: &str,
    current_skill_md: &str,
    tasks: &[SkillTask],
    optimizer: &O,
    executor: &E,
    verifier: &V,
) -> Result<OptimizationConclusion, SkillReplayError>
where
    O: SkillOptimizer + Sync,
    E: SkillReplayExecutor + Sync,
    V: SkillReplayVerifier + Sync,
{
    let (train, selection) = split_skill_tasks(tasks);
    if selection.is_empty() {
        return Err(SkillReplayError::EmptySelection);
    }
    let rejected = load_rejected_edits(workspace, name).unwrap_or_default();
    let prompt = build_optimizer_prompt(current_skill_md, &train, &rejected);
    let raw = optimizer.propose_edits(&prompt).await?;
    let edits = parse_optimizer_edits(&raw)?;
    let applied = apply_edits(current_skill_md, &edits, DEFAULT_EDIT_BUDGET)
        .map_err(SkillReplayError::from)?;
    if applied.applied.is_empty() {
        return Err(SkillReplayError::NoEditsProposed);
    }
    run_selection_replay(
        workspace,
        name,
        current_skill_md,
        &applied.skill_md,
        &edits,
        tasks,
        executor,
        verifier,
    )
    .await
}

pub struct StreamCompletionSkillOptimizer {
    pub provider: String,
    pub model: String,
    pub variant: Option<String>,
    pub api_key: String,
    pub workspace: ActiveWorkspaceContext,
}

impl SkillOptimizer for StreamCompletionSkillOptimizer {
    async fn propose_edits(&self, prompt: &str) -> Result<String, SkillReplayError> {
        let mut workspace = self.workspace.clone();
        workspace.session_mode = crate::session_mode::SessionMode::ReadOnly;
        let result = crate::llm::stream_completion(
            &self.provider,
            prompt,
            &self.model,
            self.variant.as_deref(),
            &self.api_key,
            &self.provider,
            &self.model,
            &self.api_key,
            Some(workspace),
            None,
            None,
            Vec::new(),
            None,
            Some(
                "## Skill optimizer\n\nReturn only a JSON array of bounded skill edits."
                    .to_string(),
            ),
            None,
            None,
            |_| {},
        )
        .await
        .map_err(|error| SkillReplayError::Failed(error.to_string()))?;
        Ok(result.full_response)
    }
}

fn load_rejected_edits(
    workspace: &Path,
    name: &str,
) -> Result<Vec<RejectedEditRecord>, SkillOptIoError> {
    if validate_skill_name(name).is_err() {
        return Ok(Vec::new());
    }
    let path = workspace
        .join(".gospel")
        .join("skill-opt")
        .join(name)
        .join("rejected.json");
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(SkillOptIoError::Io(error.to_string())),
    };
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

pub struct VerificationAgentVerifier {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub workspace: ActiveWorkspaceContext,
}

impl SkillReplayVerifier for VerificationAgentVerifier {
    async fn verify(&self, prompt: &str, response: &str) -> VerificationStatus {
        crate::verification::run_verification(
            &self.provider,
            &self.model,
            &self.api_key,
            &self.workspace,
            response,
            prompt,
        )
        .await
        .status
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptimizationConclusion {
    pub verdict: GateVerdict,
    pub staged_path: Option<String>,
    pub rejected_path: Option<String>,
}

pub fn conclude_optimization(
    workspace: &Path,
    name: &str,
    candidate_skill_md: &str,
    edits: &[SkillEdit],
    current_scores: &[f64],
    candidate_scores: &[f64],
) -> Result<OptimizationConclusion, SkillOptIoError> {
    let verdict = evaluate_selection_gate(current_scores, candidate_scores);
    match verdict {
        GateVerdict::Accept {
            current_mean,
            candidate_mean,
        } => {
            let staged = stage_skill(workspace, name, candidate_skill_md)?;
            write_report(workspace, name, current_mean, candidate_mean)?;
            Ok(OptimizationConclusion {
                verdict,
                staged_path: Some(staged.display().to_string()),
                rejected_path: None,
            })
        }
        GateVerdict::Reject {
            current_mean,
            candidate_mean,
            ..
        } => {
            let rejected =
                record_rejected_edits(workspace, name, edits, current_mean, candidate_mean)?;
            write_report(workspace, name, current_mean, candidate_mean)?;
            Ok(OptimizationConclusion {
                verdict,
                staged_path: None,
                rejected_path: Some(rejected.display().to_string()),
            })
        }
    }
}

pub fn harvest_skill_tasks(
    skill_name: &str,
    transcripts: &[HarvestedTranscript],
) -> Vec<SkillTask> {
    let slash = Regex::new(SLASH_PATTERN).expect("slash pattern is static");
    let mut tasks = Vec::new();

    for transcript in transcripts {
        let entries =
            serde_json::from_str::<Vec<Value>>(&transcript.display_transcript).unwrap_or_default();
        let mut pending: Option<SkillTask> = None;

        for entry in entries {
            let role = entry.get("role").and_then(Value::as_str).unwrap_or("");
            if role == "user" {
                if let Some(task) = pending.take() {
                    tasks.push(task);
                }
                let content = entry.get("content").and_then(Value::as_str).unwrap_or("");
                let Some(captures) = slash.captures(content.trim()) else {
                    continue;
                };
                if captures.get(1).map(|m| m.as_str()) != Some(skill_name) {
                    continue;
                }
                let prompt = captures
                    .get(2)
                    .map(|m| m.as_str().trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| content.trim().to_string());
                pending = Some(SkillTask {
                    session_id: transcript.session_id.clone(),
                    prompt,
                    tool_names: Vec::new(),
                });
            } else if role == "assistant" {
                if let Some(task) = pending.as_mut() {
                    if let Some(blocks) = entry.get("blocks").and_then(Value::as_array) {
                        for block in blocks {
                            if block.get("kind").and_then(Value::as_str) != Some("tool") {
                                continue;
                            }
                            if let Some(name) = block.get("name").and_then(Value::as_str) {
                                if !name.is_empty() {
                                    task.tool_names.push(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Some(task) = pending {
            tasks.push(task);
        }
    }

    tasks
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateRejectReason {
    EmptySelection,
    NotStrictlyBetter,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum GateVerdict {
    Accept {
        current_mean: f64,
        candidate_mean: f64,
    },
    Reject {
        current_mean: f64,
        candidate_mean: f64,
        reason: GateRejectReason,
    },
}

pub fn evaluate_selection_gate(current: &[f64], candidate: &[f64]) -> GateVerdict {
    if current.is_empty() || candidate.is_empty() {
        return GateVerdict::Reject {
            current_mean: 0.0,
            candidate_mean: 0.0,
            reason: GateRejectReason::EmptySelection,
        };
    }

    let current_mean = mean(current);
    let candidate_mean = mean(candidate);
    if candidate_mean > current_mean {
        GateVerdict::Accept {
            current_mean,
            candidate_mean,
        }
    } else {
        GateVerdict::Reject {
            current_mean,
            candidate_mean,
            reason: GateRejectReason::NotStrictlyBetter,
        }
    }
}

fn mean(scores: &[f64]) -> f64 {
    scores.iter().sum::<f64>() / scores.len() as f64
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SkillOptIoError {
    #[error("skill name is invalid")]
    InvalidName,
    #[error("staged skill is missing")]
    MissingStage,
    #[error("discovered skill is missing")]
    MissingSkill,
    #[error("skill optimization IO failed: {0}")]
    Io(String),
}

const SKILL_NAME_PATTERN: &str = r"^[a-zA-Z0-9-]+$";

pub fn stage_skill(
    workspace: &Path,
    name: &str,
    skill_md: &str,
) -> Result<PathBuf, SkillOptIoError> {
    validate_skill_name(name)?;
    validate_skill_document(name, skill_md)?;
    let path = staged_skill_path(workspace, name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| SkillOptIoError::Io(e.to_string()))?;
    }
    atomic_write(&path, skill_md.as_bytes())?;
    Ok(path)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StagedSkillSummary {
    pub name: String,
    pub staged: bool,
    pub has_rejected: bool,
    pub current_mean: Option<f64>,
    pub candidate_mean: Option<f64>,
    pub preview: Option<String>,
    pub diff: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct OptimizationReport {
    current_mean: f64,
    candidate_mean: f64,
}

fn write_report(
    workspace: &Path,
    name: &str,
    current_mean: f64,
    candidate_mean: f64,
) -> Result<(), SkillOptIoError> {
    validate_skill_name(name)?;
    let path = workspace
        .join(".gospel")
        .join("skill-opt")
        .join(name)
        .join("report.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| SkillOptIoError::Io(e.to_string()))?;
    }
    let payload = serde_json::to_string_pretty(&OptimizationReport {
        current_mean,
        candidate_mean,
    })
    .map_err(|e| SkillOptIoError::Io(e.to_string()))?;
    atomic_write(&path, payload.as_bytes())
}

fn read_report(skill_dir: &Path) -> Option<OptimizationReport> {
    let raw = fs::read_to_string(skill_dir.join("report.json")).ok()?;
    serde_json::from_str(&raw).ok()
}

fn skill_body(skill_md: &str) -> String {
    split_skill_document(skill_md)
        .map(|(_, body)| body)
        .unwrap_or_else(|_| skill_md.to_string())
        .trim()
        .to_string()
}

fn skill_body_diff(current_md: &str, candidate_md: &str) -> Option<String> {
    let current = skill_body(current_md);
    let candidate = skill_body(candidate_md);
    if current == candidate {
        return None;
    }
    let old_lines: Vec<&str> = current.lines().collect();
    let new_lines: Vec<&str> = candidate.lines().collect();
    let mut diff = String::new();
    let mut i = 0;
    let mut j = 0;
    while i < old_lines.len() || j < new_lines.len() {
        if i < old_lines.len() && j < new_lines.len() && old_lines[i] == new_lines[j] {
            i += 1;
            j += 1;
            continue;
        }
        if j < new_lines.len() && old_lines[i..].iter().any(|line| *line == new_lines[j]) {
            diff.push_str(&format!("+ {}\n", new_lines[j]));
            j += 1;
            continue;
        }
        if i < old_lines.len() {
            diff.push_str(&format!("- {}\n", old_lines[i]));
            i += 1;
            continue;
        }
        if j < new_lines.len() {
            diff.push_str(&format!("+ {}\n", new_lines[j]));
            j += 1;
        }
    }
    let trimmed = diff.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn preview_skill_md(skill_md: &str) -> String {
    let body = split_skill_document(skill_md)
        .map(|(_, body)| body)
        .unwrap_or_else(|_| skill_md.to_string());
    let trimmed = body.trim();
    const MAX: usize = 400;
    if trimmed.chars().count() <= MAX {
        trimmed.to_string()
    } else {
        format!("{}…", trimmed.chars().take(MAX).collect::<String>())
    }
}

pub fn list_staged_skills(workspace: &Path) -> Result<Vec<StagedSkillSummary>, SkillOptIoError> {
    let root = workspace.join(".gospel").join("skill-opt");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(SkillOptIoError::Io(error.to_string())),
    };

    let mut summaries = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| SkillOptIoError::Io(error.to_string()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| SkillOptIoError::Io(error.to_string()))?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if validate_skill_name(&name).is_err() {
            continue;
        }
        let staged = entry.path().join("SKILL.md").is_file();
        let has_rejected = entry.path().join("rejected.json").is_file();
        if staged || has_rejected {
            let report = read_report(&entry.path());
            let staged_md = staged
                .then(|| fs::read_to_string(entry.path().join("SKILL.md")).ok())
                .flatten();
            let preview = staged_md.as_deref().map(preview_skill_md);
            let diff = staged_md.as_deref().and_then(|candidate| {
                current_skill_document(workspace, &name)
                    .ok()
                    .and_then(|current| skill_body_diff(&current, candidate))
            });
            summaries.push(StagedSkillSummary {
                name,
                staged,
                has_rejected,
                current_mean: report.as_ref().map(|r| r.current_mean),
                candidate_mean: report.as_ref().map(|r| r.candidate_mean),
                preview,
                diff,
            });
        }
    }
    summaries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(summaries)
}

pub fn current_skill_document(workspace: &Path, name: &str) -> Result<String, SkillOptIoError> {
    validate_skill_name(name)?;
    let path = discovered_skill_path(workspace, name);
    let skill_md = fs::read_to_string(&path).map_err(|_| SkillOptIoError::MissingSkill)?;
    validate_skill_document(name, &skill_md)?;
    Ok(skill_md)
}

pub fn adopt_skill(workspace: &Path, name: &str) -> Result<PathBuf, SkillOptIoError> {
    validate_skill_name(name)?;
    let staged = staged_skill_path(workspace, name);
    let skill_md = fs::read_to_string(&staged).map_err(|_| SkillOptIoError::MissingStage)?;
    validate_skill_document(name, &skill_md)?;
    let adopted = discovered_skill_path(workspace, name);
    if let Some(parent) = adopted.parent() {
        fs::create_dir_all(parent).map_err(|e| SkillOptIoError::Io(e.to_string()))?;
    }
    atomic_write(&adopted, skill_md.as_bytes())?;
    Ok(adopted)
}

fn validate_skill_document(name: &str, skill_md: &str) -> Result<(), SkillOptIoError> {
    let (frontmatter, _) =
        split_skill_document(skill_md).map_err(|_| SkillOptIoError::InvalidName)?;
    let parsed: SkillFrontmatter =
        serde_yaml::from_str(&frontmatter).map_err(|_| SkillOptIoError::InvalidName)?;
    if parsed.name != name {
        return Err(SkillOptIoError::InvalidName);
    }
    Ok(())
}

fn validate_skill_name(name: &str) -> Result<(), SkillOptIoError> {
    let valid = Regex::new(SKILL_NAME_PATTERN).expect("skill name pattern is static");
    if valid.is_match(name) {
        Ok(())
    } else {
        Err(SkillOptIoError::InvalidName)
    }
}

fn staged_skill_path(workspace: &Path, name: &str) -> PathBuf {
    workspace
        .join(".gospel")
        .join("skill-opt")
        .join(name)
        .join("SKILL.md")
}

fn discovered_skill_path(workspace: &Path, name: &str) -> PathBuf {
    workspace
        .join(".agents")
        .join("skills")
        .join(name)
        .join("SKILL.md")
}

pub fn split_skill_tasks(tasks: &[SkillTask]) -> (Vec<SkillTask>, Vec<SkillTask>) {
    if tasks.len() < 2 {
        return (tasks.to_vec(), Vec::new());
    }
    let selection_count = (tasks.len() / 5).max(1);
    let split_at = tasks.len() - selection_count;
    (tasks[..split_at].to_vec(), tasks[split_at..].to_vec())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RejectedEditRecord {
    pub edits: Vec<SkillEdit>,
    pub current_mean: f64,
    pub candidate_mean: f64,
}

pub fn record_rejected_edits(
    workspace: &Path,
    name: &str,
    edits: &[SkillEdit],
    current_mean: f64,
    candidate_mean: f64,
) -> Result<PathBuf, SkillOptIoError> {
    validate_skill_name(name)?;
    let path = workspace
        .join(".gospel")
        .join("skill-opt")
        .join(name)
        .join("rejected.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| SkillOptIoError::Io(e.to_string()))?;
    }
    let mut records: Vec<RejectedEditRecord> = fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    records.push(RejectedEditRecord {
        edits: edits.to_vec(),
        current_mean,
        candidate_mean,
    });
    let payload =
        serde_json::to_string_pretty(&records).map_err(|e| SkillOptIoError::Io(e.to_string()))?;
    atomic_write(&path, payload.as_bytes())?;
    Ok(path)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), SkillOptIoError> {
    let parent = path
        .parent()
        .ok_or_else(|| SkillOptIoError::Io("write target has no parent".to_string()))?;
    let temp_path = parent.join(format!(
        ".{}.tmp.{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("skill"),
        std::process::id()
    ));
    fs::write(&temp_path, bytes).map_err(|e| SkillOptIoError::Io(e.to_string()))?;
    fs::rename(&temp_path, path).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        SkillOptIoError::Io(e.to_string())
    })
}

pub fn apply_edits(
    skill_md: &str,
    edits: &[SkillEdit],
    budget: usize,
) -> Result<ApplyEditsResult, ApplyEditsError> {
    let (frontmatter, mut body) = split_skill_document(skill_md)?;
    let mut applied = Vec::new();
    let mut skipped = Vec::new();

    for (index, edit) in edits.iter().enumerate() {
        if index >= budget {
            skipped.push(SkippedEdit {
                edit: edit.clone(),
                reason: EditSkipReason::OverBudget,
            });
            continue;
        }
        match apply_one_edit(&body, edit) {
            Ok(next_body) => {
                body = next_body;
                applied.push(AppliedEdit { edit: edit.clone() });
            }
            Err(reason) => skipped.push(SkippedEdit {
                edit: edit.clone(),
                reason,
            }),
        }
    }

    Ok(ApplyEditsResult {
        skill_md: join_skill_document(&frontmatter, &body),
        applied,
        skipped,
    })
}

fn apply_one_edit(body: &str, edit: &SkillEdit) -> Result<String, EditSkipReason> {
    match edit {
        SkillEdit::Append { text } => {
            if text.is_empty() {
                return Err(EditSkipReason::Empty);
            }
            let mut next = body.to_string();
            if !next.ends_with('\n') {
                next.push('\n');
            }
            next.push_str(text);
            if !text.ends_with('\n') {
                next.push('\n');
            }
            Ok(next)
        }
        SkillEdit::Insert { after, text } => {
            if after.is_empty() || text.is_empty() {
                return Err(EditSkipReason::Empty);
            }
            locate_unique_unprotected(body, after)?;
            let at = body.find(after).expect("unique match already verified");
            let insert_at = at + after.len();
            let mut next = String::new();
            next.push_str(&body[..insert_at]);
            next.push_str(text);
            next.push_str(&body[insert_at..]);
            Ok(next)
        }
        SkillEdit::Replace { old, new } => {
            if old.is_empty() {
                return Err(EditSkipReason::Empty);
            }
            if old == new {
                return Err(EditSkipReason::NoOp);
            }
            locate_unique_unprotected(body, old)?;
            Ok(body.replacen(old, new, 1))
        }
        SkillEdit::Delete { text } => {
            if text.is_empty() {
                return Err(EditSkipReason::Empty);
            }
            locate_unique_unprotected(body, text)?;
            Ok(body.replacen(text, "", 1))
        }
    }
}

fn locate_unique_unprotected(body: &str, needle: &str) -> Result<(), EditSkipReason> {
    let matches = body.matches(needle).count();
    if matches == 0 {
        return Err(EditSkipReason::NoMatch);
    }
    if matches > 1 {
        return Err(EditSkipReason::AmbiguousMatch);
    }
    if overlaps_protected_region(body, needle) {
        return Err(EditSkipReason::ProtectedRegion);
    }
    Ok(())
}

fn split_skill_document(skill_md: &str) -> Result<(String, String), ApplyEditsError> {
    let content = skill_md.replace("\r\n", "\n").replace('\r', "\n");
    let stripped = content
        .strip_prefix("---\n")
        .ok_or(ApplyEditsError::MissingFrontmatter)?;
    let (frontmatter, body) = stripped
        .split_once("\n---\n")
        .ok_or(ApplyEditsError::MissingFrontmatter)?;
    Ok((frontmatter.to_string(), body.to_string()))
}

fn join_skill_document(frontmatter: &str, body: &str) -> String {
    format!("---\n{frontmatter}\n---\n{body}")
}

fn protected_region_range(body: &str) -> Option<(usize, usize)> {
    let start = body.find(SLOW_UPDATE_OPEN)?;
    let close_at = body[start..].find(SLOW_UPDATE_CLOSE)?;
    let end = start + close_at + SLOW_UPDATE_CLOSE.len();
    Some((start, end))
}

fn overlaps_protected_region(body: &str, needle: &str) -> bool {
    let Some((start, end)) = protected_region_range(body) else {
        return false;
    };
    let Some(found) = body.find(needle) else {
        return false;
    };
    found < end && found + needle.len() > start
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_skill() -> String {
        "---\nname: tdd\ndescription: Test-driven development\n---\n\nWrite a failing test first.\nThen make it pass.\n"
            .to_string()
    }

    #[test]
    fn unique_replace_updates_body_and_preserves_frontmatter() {
        let result = apply_edits(
            &sample_skill(),
            &[SkillEdit::Replace {
                old: "Write a failing test first.".to_string(),
                new: "Write one failing test first.".to_string(),
            }],
            DEFAULT_EDIT_BUDGET,
        )
        .unwrap();

        assert!(result.skill_md.starts_with("---\nname: tdd\n"));
        assert!(result.skill_md.contains("Write one failing test first."));
        assert!(!result.skill_md.contains("Write a failing test first."));
        assert_eq!(result.applied.len(), 1);
        assert!(result.skipped.is_empty());
    }

    #[test]
    fn missing_replace_target_is_skipped_without_changing_the_skill() {
        let original = sample_skill();
        let result = apply_edits(
            &original,
            &[SkillEdit::Replace {
                old: "This sentence is not in the skill.".to_string(),
                new: "Replacement.".to_string(),
            }],
            DEFAULT_EDIT_BUDGET,
        )
        .unwrap();

        assert_eq!(result.skill_md, original);
        assert!(result.applied.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.skipped[0].reason, EditSkipReason::NoMatch);
    }

    #[test]
    fn ambiguous_replace_target_is_skipped_without_changing_the_skill() {
        let skill =
            "---\nname: tdd\ndescription: Test-driven development\n---\n\nRepeat me.\nRepeat me.\n"
                .to_string();
        let result = apply_edits(
            &skill,
            &[SkillEdit::Replace {
                old: "Repeat me.".to_string(),
                new: "Once.".to_string(),
            }],
            DEFAULT_EDIT_BUDGET,
        )
        .unwrap();

        assert_eq!(result.skill_md, skill);
        assert!(result.applied.is_empty());
        assert_eq!(result.skipped[0].reason, EditSkipReason::AmbiguousMatch);
    }

    #[test]
    fn empty_or_no_op_replace_is_skipped() {
        let original = sample_skill();
        let result = apply_edits(
            &original,
            &[
                SkillEdit::Replace {
                    old: String::new(),
                    new: "x".to_string(),
                },
                SkillEdit::Replace {
                    old: "Write a failing test first.".to_string(),
                    new: "Write a failing test first.".to_string(),
                },
            ],
            DEFAULT_EDIT_BUDGET,
        )
        .unwrap();

        assert_eq!(result.skill_md, original);
        assert!(result.applied.is_empty());
        assert_eq!(result.skipped[0].reason, EditSkipReason::Empty);
        assert_eq!(result.skipped[1].reason, EditSkipReason::NoOp);
    }

    #[test]
    fn replace_inside_slow_update_region_is_skipped() {
        let skill = format!(
            "---\nname: tdd\ndescription: Test-driven development\n---\n\nKeep this.\n{SLOW_UPDATE_OPEN}\nDurable lesson.\n{SLOW_UPDATE_CLOSE}\n"
        );
        let result = apply_edits(
            &skill,
            &[SkillEdit::Replace {
                old: "Durable lesson.".to_string(),
                new: "Clobber.".to_string(),
            }],
            DEFAULT_EDIT_BUDGET,
        )
        .unwrap();

        assert_eq!(result.skill_md, skill);
        assert_eq!(result.skipped[0].reason, EditSkipReason::ProtectedRegion);
    }

    #[test]
    fn append_insert_and_delete_mutate_the_body() {
        let result = apply_edits(
            &sample_skill(),
            &[
                SkillEdit::Append {
                    text: "Never skip the red step.".to_string(),
                },
                SkillEdit::Insert {
                    after: "Write a failing test first.".to_string(),
                    text: "\nWatch it fail.".to_string(),
                },
                SkillEdit::Delete {
                    text: "Then make it pass.\n".to_string(),
                },
            ],
            DEFAULT_EDIT_BUDGET,
        )
        .unwrap();

        assert!(result.skill_md.contains("Never skip the red step."));
        assert!(result.skill_md.contains("Watch it fail."));
        assert!(!result.skill_md.contains("Then make it pass."));
        assert_eq!(result.applied.len(), 3);
    }

    #[test]
    fn extra_edits_beyond_budget_are_skipped() {
        let result = apply_edits(
            &sample_skill(),
            &[
                SkillEdit::Append {
                    text: "one".to_string(),
                },
                SkillEdit::Append {
                    text: "two".to_string(),
                },
                SkillEdit::Append {
                    text: "three".to_string(),
                },
            ],
            2,
        )
        .unwrap();

        assert_eq!(result.applied.len(), 2);
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.skipped[0].reason, EditSkipReason::OverBudget);
        assert!(result.skill_md.contains("one"));
        assert!(result.skill_md.contains("two"));
        assert!(!result.skill_md.contains("three"));
    }

    #[test]
    fn replace_does_not_edit_frontmatter() {
        let result = apply_edits(
            &sample_skill(),
            &[SkillEdit::Replace {
                old: "tdd".to_string(),
                new: "other".to_string(),
            }],
            DEFAULT_EDIT_BUDGET,
        )
        .unwrap();

        assert!(result.skill_md.starts_with("---\nname: tdd\n"));
        assert_eq!(result.skipped[0].reason, EditSkipReason::NoMatch);
    }

    #[test]
    fn harvest_keeps_invoked_prompts_and_tool_names_without_arguments() {
        let transcript = serde_json::json!([
            {"role": "user", "content": "/tdd add a failing test"},
            {
                "role": "assistant",
                "content": "writing it",
                "blocks": [
                    {
                        "kind": "tool",
                        "name": "source_edit",
                        "arguments": {"old_text": "secret old", "new_text": "secret new"},
                        "result": "secret result"
                    }
                ]
            },
            {"role": "user", "content": "/diagnose flaky stream"},
            {"role": "user", "content": "please use tdd without a slash"}
        ])
        .to_string();

        let tasks = harvest_skill_tasks(
            "tdd",
            &[HarvestedTranscript {
                session_id: "session-1".to_string(),
                display_transcript: transcript,
            }],
        );

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].session_id, "session-1");
        assert_eq!(tasks[0].prompt, "add a failing test");
        assert_eq!(tasks[0].tool_names, vec!["source_edit"]);
        let serialized = serde_json::to_string(&tasks).unwrap();
        assert!(!serialized.contains("secret"));
    }

    #[test]
    fn selection_gate_accepts_only_a_strictly_higher_mean() {
        let accepted = evaluate_selection_gate(&[0.0, 1.0], &[1.0, 1.0]);
        assert_eq!(
            accepted,
            GateVerdict::Accept {
                current_mean: 0.5,
                candidate_mean: 1.0,
            }
        );

        let tied = evaluate_selection_gate(&[1.0, 0.0], &[0.5, 0.5]);
        assert_eq!(
            tied,
            GateVerdict::Reject {
                current_mean: 0.5,
                candidate_mean: 0.5,
                reason: GateRejectReason::NotStrictlyBetter,
            }
        );

        let empty = evaluate_selection_gate(&[], &[]);
        assert_eq!(
            empty,
            GateVerdict::Reject {
                current_mean: 0.0,
                candidate_mean: 0.0,
                reason: GateRejectReason::EmptySelection,
            }
        );
    }

    #[test]
    fn staging_does_not_write_discovered_skills_until_adopt() {
        let workspace = tempfile::tempdir().unwrap();
        let skill_md = sample_skill();

        let staged = stage_skill(workspace.path(), "tdd", &skill_md).unwrap();
        assert_eq!(
            staged,
            workspace.path().join(".gospel/skill-opt/tdd/SKILL.md")
        );
        assert_eq!(std::fs::read_to_string(&staged).unwrap(), skill_md);
        assert!(!workspace
            .path()
            .join(".agents/skills/tdd/SKILL.md")
            .exists());

        let adopted = adopt_skill(workspace.path(), "tdd").unwrap();
        assert_eq!(
            adopted,
            workspace.path().join(".agents/skills/tdd/SKILL.md")
        );
        assert_eq!(std::fs::read_to_string(&adopted).unwrap(), skill_md);
    }

    #[test]
    fn adopt_rejects_missing_stage_and_unsafe_names() {
        let workspace = tempfile::tempdir().unwrap();
        assert!(adopt_skill(workspace.path(), "tdd").is_err());
        assert!(stage_skill(workspace.path(), "../escape", &sample_skill()).is_err());
        assert!(stage_skill(workspace.path(), "tdd/nested", &sample_skill()).is_err());
        assert!(stage_skill(workspace.path(), "other", &sample_skill()).is_err());
    }

    #[test]
    fn split_holds_out_the_tail_and_refuses_a_single_task() {
        let tasks: Vec<SkillTask> = (0..5)
            .map(|i| SkillTask {
                session_id: format!("s{i}"),
                prompt: format!("task {i}"),
                tool_names: vec![],
            })
            .collect();

        let (train, selection) = split_skill_tasks(&tasks);
        assert_eq!(train.len(), 4);
        assert_eq!(selection.len(), 1);
        assert_eq!(selection[0].prompt, "task 4");

        let one = split_skill_tasks(&tasks[..1]);
        assert_eq!(one.0.len(), 1);
        assert!(one.1.is_empty());
    }

    #[test]
    fn rejected_edits_are_recorded_without_adopting_a_skill() {
        let workspace = tempfile::tempdir().unwrap();
        let edits = vec![SkillEdit::Append {
            text: "harmful rule".to_string(),
        }];
        let path = record_rejected_edits(workspace.path(), "tdd", &edits, 0.8, 0.4).unwrap();

        assert_eq!(
            path,
            workspace.path().join(".gospel/skill-opt/tdd/rejected.json")
        );
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("harmful rule"));
        assert!(!workspace
            .path()
            .join(".agents/skills/tdd/SKILL.md")
            .exists());
    }

    #[test]
    fn candidate_preamble_injects_the_full_body() {
        let preamble = invoked_preamble_from_skill_md("tdd", &sample_skill()).unwrap();
        assert!(preamble.starts_with("## Invoked Skill: tdd\n\n"));
        assert!(preamble.contains("Write a failing test first."));
        assert!(!preamble.contains("## Active Skills"));
    }

    #[test]
    fn verification_outcomes_map_to_gate_scores() {
        use crate::verification::VerificationStatus;

        assert_eq!(score_verification(VerificationStatus::Pass), Some(1.0));
        assert_eq!(score_verification(VerificationStatus::Concerns), Some(0.5));
        assert_eq!(score_verification(VerificationStatus::Fail), Some(0.0));
        assert_eq!(score_verification(VerificationStatus::Unavailable), None);
    }

    #[test]
    fn student_turn_injects_full_body_and_skips_auto_match() {
        let turn = prepare_student_turn("tdd", &sample_skill(), "add a failing test").unwrap();

        assert_eq!(turn.effective_prompt, "add a failing test");
        assert!(turn
            .invoked_skill_section
            .contains("Write a failing test first."));
        assert!(turn.matched_skills_section.is_none());
    }

    #[test]
    fn conclude_stages_on_accept_and_records_rejects_without_adopting() {
        let workspace = tempfile::tempdir().unwrap();
        let edits = vec![SkillEdit::Append {
            text: "Always watch it fail.".to_string(),
        }];
        let candidate = apply_edits(&sample_skill(), &edits, DEFAULT_EDIT_BUDGET)
            .unwrap()
            .skill_md;

        let accepted = conclude_optimization(
            workspace.path(),
            "tdd",
            &candidate,
            &edits,
            &[0.0, 1.0],
            &[1.0, 1.0],
        )
        .unwrap();
        assert!(matches!(accepted.verdict, GateVerdict::Accept { .. }));
        assert!(workspace
            .path()
            .join(".gospel/skill-opt/tdd/SKILL.md")
            .exists());
        assert!(!workspace
            .path()
            .join(".agents/skills/tdd/SKILL.md")
            .exists());

        let rejected = conclude_optimization(
            workspace.path(),
            "tdd",
            &candidate,
            &edits,
            &[1.0, 1.0],
            &[0.0, 1.0],
        )
        .unwrap();
        assert!(matches!(rejected.verdict, GateVerdict::Reject { .. }));
        assert!(workspace
            .path()
            .join(".gospel/skill-opt/tdd/rejected.json")
            .exists());
        assert!(!workspace
            .path()
            .join(".agents/skills/tdd/SKILL.md")
            .exists());
    }

    #[test]
    fn paired_scores_drop_unavailable_on_either_side() {
        use crate::verification::VerificationStatus::*;

        let (current, candidate) = paired_scores(
            &[Pass, Concerns, Unavailable, Fail],
            &[Fail, Pass, Pass, Concerns],
        );

        assert_eq!(current, vec![1.0, 0.5, 0.0]);
        assert_eq!(candidate, vec![0.0, 1.0, 0.5]);
    }

    #[test]
    fn list_staged_skills_reports_accepted_and_rejected_artifacts() {
        let workspace = tempfile::tempdir().unwrap();
        stage_skill(workspace.path(), "tdd", &sample_skill()).unwrap();
        record_rejected_edits(
            workspace.path(),
            "diagnose",
            &[SkillEdit::Append {
                text: "nope".to_string(),
            }],
            1.0,
            0.0,
        )
        .unwrap();

        let mut listed = list_staged_skills(workspace.path()).unwrap();
        listed.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "diagnose");
        assert!(!listed[0].staged);
        assert!(listed[0].has_rejected);
        assert_eq!(listed[1].name, "tdd");
        assert!(listed[1].staged);
        assert!(!listed[1].has_rejected);
    }

    #[test]
    fn list_staged_skills_includes_gate_scores_and_preview_after_accept() {
        let workspace = tempfile::tempdir().unwrap();
        let candidate = apply_edits(
            &sample_skill(),
            &[SkillEdit::Append {
                text: "Always watch it fail.".to_string(),
            }],
            DEFAULT_EDIT_BUDGET,
        )
        .unwrap()
        .skill_md;

        conclude_optimization(
            workspace.path(),
            "tdd",
            &candidate,
            &[],
            &[0.0, 1.0],
            &[1.0, 1.0],
        )
        .unwrap();

        let listed = list_staged_skills(workspace.path()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].current_mean, Some(0.5));
        assert_eq!(listed[0].candidate_mean, Some(1.0));
        assert!(listed[0]
            .preview
            .as_deref()
            .unwrap()
            .contains("Always watch it fail."));
        assert!(listed[0].diff.is_none());
    }

    #[test]
    fn list_staged_skills_includes_a_body_diff_against_the_discovered_skill() {
        let workspace = tempfile::tempdir().unwrap();
        let current_path = workspace.path().join(".agents/skills/tdd/SKILL.md");
        std::fs::create_dir_all(current_path.parent().unwrap()).unwrap();
        std::fs::write(&current_path, sample_skill()).unwrap();
        let candidate = apply_edits(
            &sample_skill(),
            &[SkillEdit::Append {
                text: "Always watch it fail.".to_string(),
            }],
            DEFAULT_EDIT_BUDGET,
        )
        .unwrap()
        .skill_md;
        stage_skill(workspace.path(), "tdd", &candidate).unwrap();

        let listed = list_staged_skills(workspace.path()).unwrap();
        let diff = listed[0].diff.as_deref().unwrap();
        assert!(diff.contains("+ Always watch it fail."));
        assert!(!diff.contains("- name: tdd"));
    }

    struct ScriptedExecutor;

    impl SkillReplayExecutor for ScriptedExecutor {
        async fn execute(&self, turn: &StudentTurn) -> Result<String, SkillReplayError> {
            if turn.matched_skills_section.is_some() {
                return Err(SkillReplayError::InvalidStudentTurn);
            }
            if turn.invoked_skill_section.contains("Always watch it fail.") {
                Ok("candidate-ok".to_string())
            } else {
                Ok("current-weak".to_string())
            }
        }
    }

    struct ScriptedVerifier;

    impl SkillReplayVerifier for ScriptedVerifier {
        async fn verify(&self, _prompt: &str, response: &str) -> VerificationStatus {
            if response == "candidate-ok" {
                VerificationStatus::Pass
            } else {
                VerificationStatus::Fail
            }
        }
    }

    #[tokio::test]
    async fn replay_runner_stages_when_candidate_beats_current() {
        let workspace = tempfile::tempdir().unwrap();
        let edits = vec![SkillEdit::Append {
            text: "Always watch it fail.".to_string(),
        }];
        let candidate = apply_edits(&sample_skill(), &edits, DEFAULT_EDIT_BUDGET)
            .unwrap()
            .skill_md;
        let tasks = vec![
            SkillTask {
                session_id: "s1".to_string(),
                prompt: "add a failing test".to_string(),
                tool_names: vec![],
            },
            SkillTask {
                session_id: "s2".to_string(),
                prompt: "watch the red".to_string(),
                tool_names: vec![],
            },
        ];

        let conclusion = replay_and_conclude(
            workspace.path(),
            "tdd",
            &sample_skill(),
            &candidate,
            &edits,
            &tasks,
            &ScriptedExecutor,
            &ScriptedVerifier,
        )
        .await
        .unwrap();

        assert!(matches!(conclusion.verdict, GateVerdict::Accept { .. }));
        assert!(workspace
            .path()
            .join(".gospel/skill-opt/tdd/SKILL.md")
            .exists());
        assert!(!workspace
            .path()
            .join(".agents/skills/tdd/SKILL.md")
            .exists());
    }

    struct WeakerCandidateExecutor;

    impl SkillReplayExecutor for WeakerCandidateExecutor {
        async fn execute(&self, turn: &StudentTurn) -> Result<String, SkillReplayError> {
            if turn.invoked_skill_section.contains("Always watch it fail.") {
                Ok("candidate-weak".to_string())
            } else {
                Ok("current-ok".to_string())
            }
        }
    }

    struct WeakerCandidateVerifier;

    impl SkillReplayVerifier for WeakerCandidateVerifier {
        async fn verify(&self, _prompt: &str, response: &str) -> VerificationStatus {
            if response == "current-ok" {
                VerificationStatus::Pass
            } else {
                VerificationStatus::Fail
            }
        }
    }

    #[tokio::test]
    async fn replay_runner_records_rejects_without_adopting() {
        let workspace = tempfile::tempdir().unwrap();
        let edits = vec![SkillEdit::Append {
            text: "Always watch it fail.".to_string(),
        }];
        let candidate = apply_edits(&sample_skill(), &edits, DEFAULT_EDIT_BUDGET)
            .unwrap()
            .skill_md;
        let tasks = vec![SkillTask {
            session_id: "s1".to_string(),
            prompt: "add a failing test".to_string(),
            tool_names: vec![],
        }];

        let conclusion = replay_and_conclude(
            workspace.path(),
            "tdd",
            &sample_skill(),
            &candidate,
            &edits,
            &tasks,
            &WeakerCandidateExecutor,
            &WeakerCandidateVerifier,
        )
        .await
        .unwrap();

        assert!(matches!(conclusion.verdict, GateVerdict::Reject { .. }));
        assert!(workspace
            .path()
            .join(".gospel/skill-opt/tdd/rejected.json")
            .exists());
        assert!(!workspace
            .path()
            .join(".agents/skills/tdd/SKILL.md")
            .exists());
    }

    #[derive(Default)]
    struct FakeStudentLlm {
        last_prompt: std::sync::Mutex<Option<String>>,
        last_invoked: std::sync::Mutex<Option<String>>,
    }

    impl StudentLlm for FakeStudentLlm {
        async fn complete(
            &self,
            prompt: &str,
            invoked_skill_section: &str,
        ) -> Result<String, SkillReplayError> {
            *self.last_prompt.lock().unwrap() = Some(prompt.to_string());
            *self.last_invoked.lock().unwrap() = Some(invoked_skill_section.to_string());
            Ok(format!("replayed:{prompt}"))
        }
    }

    #[tokio::test]
    async fn llm_student_executor_sends_full_body_without_auto_match() {
        let llm = FakeStudentLlm::default();
        let executor = LlmStudentExecutor { llm: &llm };
        let turn = prepare_student_turn("tdd", &sample_skill(), "add a failing test").unwrap();

        let response = executor.execute(&turn).await.unwrap();

        assert_eq!(response, "replayed:add a failing test");
        assert!(llm
            .last_invoked
            .lock()
            .unwrap()
            .as_deref()
            .unwrap()
            .contains("Write a failing test first."));
        assert_eq!(
            llm.last_prompt.lock().unwrap().as_deref(),
            Some("add a failing test")
        );
    }

    #[tokio::test]
    async fn llm_student_executor_rejects_auto_match_turns() {
        let llm = FakeStudentLlm::default();
        let executor = LlmStudentExecutor { llm: &llm };
        let turn = StudentTurn {
            effective_prompt: "hi".to_string(),
            invoked_skill_section: "## Invoked Skill: tdd\n\nbody".to_string(),
            matched_skills_section: Some("## Active Skills\n".to_string()),
        };

        let error = executor.execute(&turn).await.unwrap_err();
        assert_eq!(error, SkillReplayError::InvalidStudentTurn);
    }

    struct CountingExecutor {
        calls: std::sync::atomic::AtomicUsize,
    }

    impl SkillReplayExecutor for CountingExecutor {
        async fn execute(&self, _turn: &StudentTurn) -> Result<String, SkillReplayError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok("ok".to_string())
        }
    }

    struct PassVerifier;

    impl SkillReplayVerifier for PassVerifier {
        async fn verify(&self, _prompt: &str, _response: &str) -> VerificationStatus {
            VerificationStatus::Pass
        }
    }

    fn numbered_tasks(count: usize) -> Vec<SkillTask> {
        (0..count)
            .map(|i| SkillTask {
                session_id: format!("s{i}"),
                prompt: format!("task {i}"),
                tool_names: vec![],
            })
            .collect()
    }

    #[tokio::test]
    async fn selection_replay_does_not_call_the_llm_without_a_hold_out() {
        let workspace = tempfile::tempdir().unwrap();
        let executor = CountingExecutor {
            calls: std::sync::atomic::AtomicUsize::new(0),
        };

        let error = run_selection_replay(
            workspace.path(),
            "tdd",
            &sample_skill(),
            &sample_skill(),
            &[],
            &numbered_tasks(1),
            &executor,
            &PassVerifier,
        )
        .await
        .unwrap_err();

        assert_eq!(error, SkillReplayError::EmptySelection);
        assert_eq!(executor.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn selection_replay_runs_only_the_hold_out_tasks() {
        let workspace = tempfile::tempdir().unwrap();
        let executor = CountingExecutor {
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let edits = vec![SkillEdit::Append {
            text: "Always watch it fail.".to_string(),
        }];
        let candidate = apply_edits(&sample_skill(), &edits, DEFAULT_EDIT_BUDGET)
            .unwrap()
            .skill_md;

        run_selection_replay(
            workspace.path(),
            "tdd",
            &sample_skill(),
            &candidate,
            &edits,
            &numbered_tasks(5),
            &executor,
            &PassVerifier,
        )
        .await
        .unwrap();

        // 5 tasks → 1 selection item × current and candidate.
        assert_eq!(executor.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn current_skill_document_reads_the_discovered_workspace_skill() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join(".agents/skills/tdd/SKILL.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, sample_skill()).unwrap();

        assert_eq!(
            current_skill_document(workspace.path(), "tdd").unwrap(),
            sample_skill()
        );
        assert!(current_skill_document(workspace.path(), "missing").is_err());
    }

    #[test]
    fn parse_optimizer_edits_reads_json_array_and_keeps_budget() {
        let raw = "```json\n[{\"op\":\"append\",\"text\":\"Always watch it fail.\"},{\"op\":\"append\",\"text\":\"two\"},{\"op\":\"append\",\"text\":\"three\"},{\"op\":\"append\",\"text\":\"four\"},{\"op\":\"append\",\"text\":\"five\"}]\n```";
        let edits = parse_optimizer_edits(raw).unwrap();
        assert_eq!(edits.len(), 5);
        assert_eq!(
            edits[0],
            SkillEdit::Append {
                text: "Always watch it fail.".to_string(),
            }
        );
        assert!(parse_optimizer_edits("not json").is_err());
    }

    #[derive(Default)]
    struct CountingOptimizer {
        calls: std::sync::atomic::AtomicUsize,
        response: std::sync::Mutex<String>,
    }

    impl SkillOptimizer for CountingOptimizer {
        async fn propose_edits(&self, _prompt: &str) -> Result<String, SkillReplayError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(self.response.lock().unwrap().clone())
        }
    }

    #[tokio::test]
    async fn optimize_and_replay_does_not_call_optimizer_without_a_hold_out() {
        let workspace = tempfile::tempdir().unwrap();
        let optimizer = CountingOptimizer::default();
        let executor = CountingExecutor {
            calls: std::sync::atomic::AtomicUsize::new(0),
        };

        let error = optimize_and_replay(
            workspace.path(),
            "tdd",
            &sample_skill(),
            &numbered_tasks(1),
            &optimizer,
            &executor,
            &PassVerifier,
        )
        .await
        .unwrap_err();

        assert_eq!(error, SkillReplayError::EmptySelection);
        assert_eq!(optimizer.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(executor.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn optimize_and_replay_skips_student_when_no_edits_apply() {
        let workspace = tempfile::tempdir().unwrap();
        let optimizer = CountingOptimizer {
            calls: std::sync::atomic::AtomicUsize::new(0),
            response: std::sync::Mutex::new("[]".to_string()),
        };
        let executor = CountingExecutor {
            calls: std::sync::atomic::AtomicUsize::new(0),
        };

        let error = optimize_and_replay(
            workspace.path(),
            "tdd",
            &sample_skill(),
            &numbered_tasks(5),
            &optimizer,
            &executor,
            &PassVerifier,
        )
        .await
        .unwrap_err();

        assert_eq!(error, SkillReplayError::NoEditsProposed);
        assert_eq!(optimizer.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(executor.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn optimize_and_replay_stages_when_proposed_edit_helps() {
        let workspace = tempfile::tempdir().unwrap();
        let optimizer = CountingOptimizer {
            calls: std::sync::atomic::AtomicUsize::new(0),
            response: std::sync::Mutex::new(
                r#"[{"op":"append","text":"Always watch it fail."}]"#.to_string(),
            ),
        };

        let conclusion = optimize_and_replay(
            workspace.path(),
            "tdd",
            &sample_skill(),
            &numbered_tasks(5),
            &optimizer,
            &ScriptedExecutor,
            &ScriptedVerifier,
        )
        .await
        .unwrap();

        assert!(matches!(conclusion.verdict, GateVerdict::Accept { .. }));
        assert!(workspace
            .path()
            .join(".gospel/skill-opt/tdd/SKILL.md")
            .exists());
        assert!(!workspace
            .path()
            .join(".agents/skills/tdd/SKILL.md")
            .exists());
    }
}
