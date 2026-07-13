use crate::corpus::tools::{
    create_corpus_neighbors_tool, create_corpus_query_tool, create_corpus_summary_tool,
    CORPUS_SYSTEM_PROMPT,
};
use crate::review::tools::{
    create_record_review_outcome_tool, create_run_multi_review_tool, create_run_review_tool,
    create_run_security_review_tool, REVIEW_TOOLS_SYSTEM_PROMPT,
};
use crate::session_mode::SessionMode;
use crate::shell_tools::CommandApproval;
use crate::shell_tools::{
    create_run_git_command_tool, create_run_github_cli_command_tool, create_run_shell_command_tool,
    SHELL_TOOLS_SYSTEM_PROMPT,
};
use crate::workspace_tools::{
    build_base_workspace_tools, build_base_workspace_tools_with_external_approval,
    create_context_search_tool, create_source_edit_tool, create_write_harness_file_tool,
    ExternalPathApproval, CONTEXT_SEARCH_SYSTEM_PROMPT, HARNESS_CONTROL_AREA_SYSTEM_PROMPT,
    READ_ONLY_SESSION_SYSTEM_PROMPT, READ_ONLY_WORKSPACE_TOOLS_SYSTEM_PROMPT,
    WORKSPACE_TOOLS_SYSTEM_PROMPT,
};
use rig::tool::ToolDyn;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

const DELEGATION_SYSTEM_PROMPT: &str = r#"
Use `delegate_exploration` only for broad multi-file, architectural, or investigative tasks that would benefit from a focused report before you answer the user.
Prefer direct file reads and targeted search for small or obvious tasks.
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Main,
    Exploration,
    Verification,
    ReviewDetector,
    ReviewValidator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveWorkspaceContext {
    pub workspace_path: std::path::PathBuf,
    pub corpus_available: bool,
    pub session_mode: SessionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LoopGuardPolicy {
    pub warning_threshold: usize,
    pub stop_threshold: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HarnessRunGuards {
    pub max_turns: usize,
    #[serde(serialize_with = "serialize_optional_duration_seconds")]
    pub deadline: Option<Duration>,
    pub loop_guard: LoopGuardPolicy,
}

const DETERMINISTIC_FAILURE_REASONS: &[&str] = &[
    "blocked",
    "path_escape",
    "secret",
    "not_found",
    "invalid_query",
    "binary",
    "too_large",
    "oversized",
    "invalid_utf8",
    "invalid_range",
    "invalid_replacement",
    "no_match",
    "ambiguous_match",
    "no_op",
];

#[derive(Debug)]
pub(crate) struct LoopDetector {
    last_call_hash: u64,
    consecutive_count: usize,
    last_failure_reason: String,
    failure_streak: usize,
    policy: LoopGuardPolicy,
}

impl LoopDetector {
    pub(crate) fn new(policy: LoopGuardPolicy) -> Self {
        Self {
            last_call_hash: 0,
            consecutive_count: 0,
            last_failure_reason: String::new(),
            failure_streak: 0,
            policy,
        }
    }

    pub(crate) fn record_call(&mut self, tool_name: &str, args: &serde_json::Value) -> LoopStatus {
        let canonical = format!(
            "{}:{}",
            tool_name,
            crate::json_utils::canonical_json_string(args)
        );
        let mut hasher = DefaultHasher::new();
        canonical.hash(&mut hasher);
        let hash = hasher.finish();
        self.consecutive_count = if hash == self.last_call_hash {
            self.consecutive_count + 1
        } else {
            self.last_call_hash = hash;
            1
        };

        self.status(self.consecutive_count)
    }

    pub(crate) fn record_failure(&mut self, reason: &str) -> Option<LoopStatus> {
        if !DETERMINISTIC_FAILURE_REASONS.contains(&reason) {
            self.reset_failure_streak();
            return None;
        }
        self.failure_streak = if reason == self.last_failure_reason {
            self.failure_streak + 1
        } else {
            self.last_failure_reason = reason.to_string();
            1
        };
        Some(self.status(self.failure_streak))
    }

    pub(crate) fn consecutive_count(&self) -> usize {
        self.consecutive_count
    }

    pub(crate) fn failure_streak(&self) -> usize {
        self.failure_streak
    }

    fn status(&self, count: usize) -> LoopStatus {
        if count >= self.policy.stop_threshold {
            LoopStatus::Stop
        } else if count >= self.policy.warning_threshold {
            LoopStatus::Warning(count)
        } else {
            LoopStatus::Ok
        }
    }

    pub(crate) fn reset_failure_streak(&mut self) {
        self.last_failure_reason.clear();
        self.failure_streak = 0;
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LoopStatus {
    Ok,
    Warning(usize),
    Stop,
}

fn serialize_optional_duration_seconds<S>(
    value: &Option<Duration>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    value
        .map(|duration| duration.as_secs())
        .serialize(serializer)
}

pub enum MainToolContribution {
    DelegateExploration(Box<dyn ToolDyn>),
    RunSkillScript(Box<dyn ToolDyn>),
}

impl MainToolContribution {
    fn into_tool(self) -> Result<Box<dyn ToolDyn>, HarnessProfileError> {
        let (expected_name, tool) = match self {
            Self::DelegateExploration(tool) => ("delegate_exploration", tool),
            Self::RunSkillScript(tool) => ("run_skill_script", tool),
        };
        let actual_name = tool.name();
        if actual_name != expected_name {
            return Err(HarnessProfileError::MismatchedToolContribution {
                expected: expected_name,
                actual: actual_name,
            });
        }
        Ok(tool)
    }
}

pub struct MainToolInputs {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub external_path_approval: Option<Arc<dyn ExternalPathApproval>>,
    pub command_approval: Option<Arc<dyn CommandApproval>>,
    pub contributions: Vec<MainToolContribution>,
}

pub struct HarnessProfileRequest {
    pub role: AgentRole,
    pub workspace: Option<ActiveWorkspaceContext>,
    pub role_guidance: Option<String>,
    pub matched_skills_section: Option<String>,
    pub invoked_skill_section: Option<String>,
    pub main_tool_inputs: Option<MainToolInputs>,
}

pub struct HarnessProfile {
    pub role: AgentRole,
    pub preamble: Option<String>,
    pub tools: Vec<Box<dyn ToolDyn>>,
    pub guards: HarnessRunGuards,
    workspace_available: bool,
    corpus_enabled: bool,
}

impl HarnessProfile {
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.iter().map(|tool| tool.name()).collect()
    }

    pub fn summary(&self) -> HarnessProfileSummary {
        let tool_names = self.tool_names();
        HarnessProfileSummary {
            role: self.role,
            source_edit_enabled: tool_names.iter().any(|name| name == "source_edit"),
            tool_names,
            workspace_available: self.workspace_available,
            corpus_enabled: self.corpus_enabled,
            guards: self.guards,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HarnessProfileSummary {
    pub role: AgentRole,
    pub tool_names: Vec<String>,
    pub workspace_available: bool,
    pub source_edit_enabled: bool,
    pub corpus_enabled: bool,
    pub guards: HarnessRunGuards,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HarnessProfileError {
    #[error("{role:?} requires an active workspace")]
    WorkspaceRequired { role: AgentRole },
    #[error("main workspace Harness Profile requires Main Tool Inputs")]
    MainToolInputsRequired,
    #[error("Main Tool Contribution expected {expected}, received {actual}")]
    MismatchedToolContribution {
        expected: &'static str,
        actual: String,
    },
}

pub fn resolve_harness_profile(
    request: HarnessProfileRequest,
) -> Result<HarnessProfile, HarnessProfileError> {
    let guards = guards_for_role(request.role);
    let Some(workspace) = request.workspace else {
        if request.role != AgentRole::Main {
            return Err(HarnessProfileError::WorkspaceRequired { role: request.role });
        }
        return Ok(HarnessProfile {
            role: request.role,
            preamble: compose_preamble(
                request.role,
                None,
                request.role_guidance,
                request.matched_skills_section,
                request.invoked_skill_section,
                &[],
            ),
            tools: request
                .main_tool_inputs
                .map(|inputs| {
                    inputs
                        .contributions
                        .into_iter()
                        .map(MainToolContribution::into_tool)
                        .collect()
                })
                .transpose()?
                .unwrap_or_default(),
            guards,
            workspace_available: false,
            corpus_enabled: false,
        });
    };

    if request.role != AgentRole::Main {
        let workspace_path = workspace.workspace_path.clone();
        let mut tools = build_base_workspace_tools(workspace_path.clone());
        if workspace.corpus_available {
            append_corpus_tools(&mut tools, workspace_path);
        }
        let preamble = compose_preamble(
            request.role,
            Some(&workspace),
            request.role_guidance,
            None,
            None,
            &tools.iter().map(|tool| tool.name()).collect::<Vec<_>>(),
        );
        return Ok(HarnessProfile {
            role: request.role,
            preamble,
            tools,
            guards,
            workspace_available: true,
            corpus_enabled: workspace.corpus_available,
        });
    }

    let MainToolInputs {
        provider,
        model,
        api_key,
        external_path_approval,
        command_approval,
        contributions,
    } = request
        .main_tool_inputs
        .ok_or(HarnessProfileError::MainToolInputsRequired)?;

    let workspace_path = workspace.workspace_path.clone();
    let mut tools = build_base_workspace_tools_with_external_approval(
        workspace_path.clone(),
        external_path_approval,
    );
    tools.push(Box::new(create_write_harness_file_tool(
        workspace_path.clone(),
    )));
    tools.push(Box::new(create_run_review_tool(
        workspace_path.clone(),
        provider.clone(),
        model.clone(),
        api_key.clone(),
    )));
    tools.push(Box::new(create_run_multi_review_tool(
        workspace_path.clone(),
        provider.clone(),
        model.clone(),
        api_key.clone(),
    )));
    tools.push(Box::new(create_run_security_review_tool(
        workspace_path.clone(),
        provider,
        model,
        api_key,
    )));
    tools.push(Box::new(create_record_review_outcome_tool(
        workspace_path.clone(),
    )));
    if workspace.session_mode.allows_source_edit() {
        tools.push(Box::new(create_source_edit_tool(workspace_path.clone())));
    }
    if workspace.corpus_available {
        append_corpus_tools(&mut tools, workspace_path.clone());
    }
    tools.push(Box::new(create_run_shell_command_tool(
        workspace_path.clone(),
        command_approval.clone(),
    )));
    tools.push(Box::new(create_run_git_command_tool(
        workspace_path.clone(),
        command_approval.clone(),
    )));
    tools.push(Box::new(create_run_github_cli_command_tool(
        workspace_path,
        command_approval,
    )));
    tools.extend(
        contributions
            .into_iter()
            .map(MainToolContribution::into_tool)
            .collect::<Result<Vec<_>, _>>()?,
    );

    let preamble = compose_preamble(
        request.role,
        Some(&workspace),
        request.role_guidance,
        request.matched_skills_section,
        request.invoked_skill_section,
        &tools.iter().map(|tool| tool.name()).collect::<Vec<_>>(),
    );

    Ok(HarnessProfile {
        role: request.role,
        preamble,
        tools,
        guards,
        workspace_available: true,
        corpus_enabled: workspace.corpus_available,
    })
}

fn append_corpus_tools(tools: &mut Vec<Box<dyn ToolDyn>>, workspace_path: std::path::PathBuf) {
    tools.push(Box::new(create_corpus_summary_tool(workspace_path.clone())));
    tools.push(Box::new(create_corpus_query_tool(workspace_path.clone())));
    tools.push(Box::new(create_corpus_neighbors_tool(
        workspace_path.clone(),
    )));
    tools.push(Box::new(create_context_search_tool(workspace_path)));
}

fn compose_preamble(
    role: AgentRole,
    workspace: Option<&ActiveWorkspaceContext>,
    role_guidance: Option<String>,
    matched_skills_section: Option<String>,
    invoked_skill_section: Option<String>,
    tool_names: &[String],
) -> Option<String> {
    let mut sections = Vec::new();
    if role == AgentRole::Main {
        push_section(&mut sections, invoked_skill_section);
        push_section(&mut sections, matched_skills_section);
    }

    if let Some(workspace) = workspace {
        let workspace_guidance =
            if role == AgentRole::Main && workspace.session_mode.allows_source_edit() {
                WORKSPACE_TOOLS_SYSTEM_PROMPT
            } else {
                READ_ONLY_WORKSPACE_TOOLS_SYSTEM_PROMPT
            };
        push_section(&mut sections, Some(workspace_guidance.to_string()));

        if role == AgentRole::Main {
            if !workspace.session_mode.allows_source_edit() {
                push_section(
                    &mut sections,
                    Some(READ_ONLY_SESSION_SYSTEM_PROMPT.to_string()),
                );
            }
            push_section(&mut sections, Some(SHELL_TOOLS_SYSTEM_PROMPT.to_string()));
            push_section(&mut sections, Some(REVIEW_TOOLS_SYSTEM_PROMPT.to_string()));
            push_section(
                &mut sections,
                Some(HARNESS_CONTROL_AREA_SYSTEM_PROMPT.to_string()),
            );
        }
        if workspace.corpus_available {
            push_section(&mut sections, Some(CORPUS_SYSTEM_PROMPT.to_string()));
            push_section(
                &mut sections,
                Some(CONTEXT_SEARCH_SYSTEM_PROMPT.to_string()),
            );
        }
        if tool_names.iter().any(|name| name == "delegate_exploration") {
            push_section(&mut sections, Some(DELEGATION_SYSTEM_PROMPT.to_string()));
        }
    }

    push_section(&mut sections, role_guidance);
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

fn push_section(sections: &mut Vec<String>, section: Option<String>) {
    if let Some(section) = section.map(|value| value.trim().to_string()) {
        if !section.is_empty() {
            sections.push(section);
        }
    }
}

pub(crate) fn guards_for_role(role: AgentRole) -> HarnessRunGuards {
    match role {
        AgentRole::Main => HarnessRunGuards {
            max_turns: 50,
            deadline: None,
            loop_guard: LoopGuardPolicy {
                warning_threshold: 3,
                stop_threshold: 5,
            },
        },
        AgentRole::Exploration => HarnessRunGuards {
            max_turns: 50,
            deadline: Some(Duration::from_secs(90)),
            loop_guard: LoopGuardPolicy {
                warning_threshold: 3,
                stop_threshold: 5,
            },
        },
        AgentRole::Verification => HarnessRunGuards {
            max_turns: 6,
            deadline: Some(Duration::from_secs(90)),
            loop_guard: LoopGuardPolicy {
                warning_threshold: 2,
                stop_threshold: 3,
            },
        },
        AgentRole::ReviewDetector => HarnessRunGuards {
            max_turns: 50,
            deadline: Some(Duration::from_secs(60)),
            loop_guard: LoopGuardPolicy {
                warning_threshold: 2,
                stop_threshold: 3,
            },
        },
        AgentRole::ReviewValidator => HarnessRunGuards {
            max_turns: 50,
            deadline: Some(Duration::from_secs(30)),
            loop_guard: LoopGuardPolicy {
                warning_threshold: 2,
                stop_threshold: 3,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_mode::SessionMode;
    use rig::agent::AgentBuilder;
    use rig::test_utils::MockCompletionModel;
    use std::path::PathBuf;

    fn main_tool_inputs() -> MainToolInputs {
        MainToolInputs {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
            api_key: "sk-never-serialize".to_string(),
            external_path_approval: None,
            command_approval: None,
            contributions: vec![],
        }
    }

    fn request(
        role: AgentRole,
        workspace_available: bool,
        mode: SessionMode,
        corpus_available: bool,
    ) -> HarnessProfileRequest {
        HarnessProfileRequest {
            role,
            workspace: workspace_available.then(|| ActiveWorkspaceContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available,
                session_mode: mode,
            }),
            role_guidance: Some(format!("Guidance for {role:?}")),
            matched_skills_section: (role == AgentRole::Main)
                .then(|| "private matched Skill text".to_string()),
            invoked_skill_section: None,
            main_tool_inputs: (role == AgentRole::Main).then(main_tool_inputs),
        }
    }

    fn expected_tool_names(
        role: AgentRole,
        workspace_available: bool,
        mode: SessionMode,
        corpus_available: bool,
    ) -> Vec<String> {
        if !workspace_available {
            return vec![];
        }
        let mut names = vec!["read_file", "search_code", "find_files", "list_directory"];
        if role == AgentRole::Main {
            names.extend([
                "write_harness_file",
                "run_review",
                "run_multi_review",
                "run_security_review",
                "record_review_outcome",
            ]);
            if mode == SessionMode::Build {
                names.push("source_edit");
            }
        }
        if corpus_available {
            names.extend([
                "corpus_summary",
                "corpus_query",
                "corpus_neighbors",
                "context_search",
            ]);
        }
        if role == AgentRole::Main {
            names.extend([
                "run_shell_command",
                "run_git_command",
                "run_github_cli_command",
            ]);
        }
        names.into_iter().map(str::to_string).collect()
    }

    #[test]
    fn contract_matrix_covers_roles_workspace_modes_and_corpus() {
        let roles = [
            AgentRole::Main,
            AgentRole::Exploration,
            AgentRole::Verification,
            AgentRole::ReviewDetector,
            AgentRole::ReviewValidator,
        ];
        for role in roles {
            for workspace_available in [false, true] {
                for mode in [SessionMode::Build, SessionMode::ReadOnly] {
                    for corpus_available in [false, true] {
                        let result = resolve_harness_profile(request(
                            role,
                            workspace_available,
                            mode,
                            corpus_available,
                        ));
                        if !workspace_available && role != AgentRole::Main {
                            assert_eq!(
                                result.err(),
                                Some(HarnessProfileError::WorkspaceRequired { role })
                            );
                            continue;
                        }

                        let profile = result.expect("valid contract-matrix profile");
                        let names = profile.tool_names();
                        let summary = profile.summary();
                        assert_eq!(
                            names,
                            expected_tool_names(role, workspace_available, mode, corpus_available,)
                        );
                        assert_eq!(summary.role, role);
                        assert_eq!(summary.workspace_available, workspace_available);
                        assert_eq!(summary.tool_names, names);
                        assert_eq!(summary.guards, profile.guards);
                        assert_eq!(
                            summary.corpus_enabled,
                            workspace_available && corpus_available
                        );

                        let source_edit_expected = workspace_available
                            && role == AgentRole::Main
                            && mode == SessionMode::Build;
                        assert_eq!(
                            names.contains(&"source_edit".to_string()),
                            source_edit_expected
                        );
                        assert_eq!(summary.source_edit_enabled, source_edit_expected);
                        assert_eq!(
                            names.contains(&"write_harness_file".to_string()),
                            workspace_available && role == AgentRole::Main
                        );
                        for mutating_name in ["source_edit", "write_harness_file"] {
                            if role != AgentRole::Main {
                                assert!(!names.contains(&mutating_name.to_string()));
                            }
                        }

                        let corpus_expected = workspace_available && corpus_available;
                        for corpus_name in [
                            "corpus_summary",
                            "corpus_query",
                            "corpus_neighbors",
                            "context_search",
                        ] {
                            assert_eq!(names.contains(&corpus_name.to_string()), corpus_expected);
                        }
                        let preamble = profile.preamble.unwrap_or_default();
                        assert!(preamble.contains(&format!("Guidance for {role:?}")));
                        assert_eq!(
                            preamble.contains("private matched Skill text"),
                            role == AgentRole::Main
                        );
                        assert_eq!(preamble.contains("Codebase Knowledge"), corpus_expected);
                        assert_eq!(preamble.contains("`context_search`"), corpus_expected);
                        assert_eq!(preamble.contains("Use `source_edit`"), source_edit_expected);
                        if workspace_available && role == AgentRole::Main {
                            assert!(preamble.contains("Harness Control Area"));
                            assert!(preamble.contains("Shell, Git, and GitHub CLI Tools"));
                            assert!(preamble.contains("Review Tools"));
                            assert_eq!(
                                preamble.contains("Read-Only Session"),
                                mode == SessionMode::ReadOnly
                            );
                        } else {
                            assert!(!preamble.contains("Harness Control Area"));
                            assert!(!preamble.contains("Shell, Git, and GitHub CLI Tools"));
                            assert!(!preamble.contains("Review Tools"));
                            assert!(!preamble.contains("Read-Only Session"));
                        }

                        let serialized = serde_json::to_string(&summary).expect("summary JSON");
                        assert!(!serialized.contains("sk-never-serialize"));
                        assert!(!serialized.contains("private matched Skill text"));
                    }
                }
            }
        }
    }

    #[test]
    fn only_main_can_resolve_without_an_active_workspace() {
        let main = resolve_harness_profile(HarnessProfileRequest {
            role: AgentRole::Main,
            workspace: None,
            role_guidance: None,
            matched_skills_section: Some("matched Skill guidance".to_string()),
            invoked_skill_section: None,
            main_tool_inputs: None,
        })
        .expect("unscoped Main profile");
        assert!(main.tools.is_empty());
        assert_eq!(main.preamble.as_deref(), Some("matched Skill guidance"));

        for role in [
            AgentRole::Exploration,
            AgentRole::Verification,
            AgentRole::ReviewDetector,
            AgentRole::ReviewValidator,
        ] {
            let error = resolve_harness_profile(HarnessProfileRequest {
                role,
                workspace: None,
                role_guidance: None,
                matched_skills_section: None,
                invoked_skill_section: None,
                main_tool_inputs: None,
            })
            .err()
            .expect("missing workspace must fail");
            assert_eq!(error, HarnessProfileError::WorkspaceRequired { role });
        }
    }

    #[test]
    fn scoped_main_requires_tool_inputs() {
        let error = resolve_harness_profile(HarnessProfileRequest {
            role: AgentRole::Main,
            workspace: Some(ActiveWorkspaceContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: false,
                session_mode: SessionMode::ReadOnly,
            }),
            role_guidance: None,
            matched_skills_section: None,
            invoked_skill_section: None,
            main_tool_inputs: None,
        })
        .err()
        .expect("missing Main Tool Inputs must fail");

        assert_eq!(error, HarnessProfileError::MainToolInputsRequired);
    }

    #[test]
    fn closed_main_contributions_reject_a_mismatched_tool() {
        let mut inputs = main_tool_inputs();
        inputs
            .contributions
            .push(MainToolContribution::DelegateExploration(Box::new(
                create_source_edit_tool(PathBuf::from("/tmp/workspace")),
            )));
        let error = resolve_harness_profile(HarnessProfileRequest {
            role: AgentRole::Main,
            workspace: Some(ActiveWorkspaceContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: false,
                session_mode: SessionMode::ReadOnly,
            }),
            role_guidance: None,
            matched_skills_section: None,
            invoked_skill_section: None,
            main_tool_inputs: Some(inputs),
        })
        .err()
        .expect("mismatched contribution must fail");

        assert_eq!(
            error,
            HarnessProfileError::MismatchedToolContribution {
                expected: "delegate_exploration",
                actual: "source_edit".to_string(),
            }
        );
    }

    #[test]
    fn role_guards_are_resolved_with_the_profile() {
        let expected = [
            (AgentRole::Main, 50, None, 3, 5),
            (AgentRole::Exploration, 50, Some(90), 3, 5),
            (AgentRole::Verification, 6, Some(90), 2, 3),
            (AgentRole::ReviewDetector, 50, Some(60), 2, 3),
            (AgentRole::ReviewValidator, 50, Some(30), 2, 3),
        ];

        for (role, max_turns, deadline_seconds, warning, stop) in expected {
            let profile = resolve_harness_profile(request(role, true, SessionMode::Build, false))
                .expect("profile");
            let guards = profile.guards;
            assert_eq!(guards.max_turns, max_turns);
            assert_eq!(
                guards.deadline.map(|value| value.as_secs()),
                deadline_seconds
            );
            assert_eq!(guards.loop_guard.warning_threshold, warning);
            assert_eq!(guards.loop_guard.stop_threshold, stop);
        }
    }

    #[test]
    fn resolved_profile_can_be_consumed_by_the_real_rig_builder() {
        let profile = resolve_harness_profile(HarnessProfileRequest {
            role: AgentRole::Verification,
            workspace: Some(ActiveWorkspaceContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: false,
                session_mode: SessionMode::Build,
            }),
            role_guidance: Some("Verify".to_string()),
            matched_skills_section: None,
            invoked_skill_section: None,
            main_tool_inputs: None,
        })
        .expect("profile");

        let preamble = profile.preamble.unwrap_or_default();
        let _agent = AgentBuilder::new(MockCompletionModel::text("ok"))
            .preamble(&preamble)
            .default_max_turns(profile.guards.max_turns)
            .tools(profile.tools)
            .build();
    }
}
