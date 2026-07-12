use crate::corpus::tools::{
    create_corpus_neighbors_tool, create_corpus_query_tool, create_corpus_summary_tool,
    CORPUS_SYSTEM_PROMPT,
};
use crate::review::tools::{
    create_record_review_outcome_tool, create_run_multi_review_tool, create_run_review_tool,
    create_run_security_review_tool, REVIEW_TOOLS_SYSTEM_PROMPT,
};
use crate::session_mode::session_mode_allows_source_edit;
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
pub struct WorkspaceToolContext {
    pub workspace_path: std::path::PathBuf,
    pub corpus_available: bool,
    pub session_mode: String,
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

pub struct MainHarnessMechanisms {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub external_path_approval: Option<Arc<dyn ExternalPathApproval>>,
    pub command_approval: Option<Arc<dyn CommandApproval>>,
    pub additional_tools: Vec<Box<dyn ToolDyn>>,
}

pub struct HarnessProfileRequest {
    pub role: AgentRole,
    pub workspace: Option<WorkspaceToolContext>,
    pub role_guidance: Option<String>,
    pub matched_skills_section: Option<String>,
    pub invoked_skill_section: Option<String>,
    pub main_mechanisms: Option<MainHarnessMechanisms>,
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
    pub fn mechanism_names(&self) -> Vec<String> {
        self.tools.iter().map(|tool| tool.name()).collect()
    }

    pub fn summary(&self) -> HarnessProfileSummary {
        let mechanism_names = self.mechanism_names();
        HarnessProfileSummary {
            role: self.role,
            source_edit_enabled: mechanism_names.iter().any(|name| name == "source_edit"),
            mechanism_names,
            workspace_available: self.workspace_available,
            corpus_enabled: self.corpus_enabled,
            guards: self.guards,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HarnessProfileSummary {
    pub role: AgentRole,
    pub mechanism_names: Vec<String>,
    pub workspace_available: bool,
    pub source_edit_enabled: bool,
    pub corpus_enabled: bool,
    pub guards: HarnessRunGuards,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HarnessProfileError {
    #[error("{role:?} requires an active workspace")]
    WorkspaceRequired { role: AgentRole },
    #[error("main workspace Harness Profile requires Main Harness Mechanisms")]
    MainMechanismsRequired,
    #[error("unexpected additional Main Harness Mechanism: {0}")]
    UnexpectedAdditionalMechanism(String),
}

pub fn resolve_harness_profile(
    request: HarnessProfileRequest,
) -> Result<HarnessProfile, HarnessProfileError> {
    if let Some(mechanisms) = request.main_mechanisms.as_ref() {
        for tool in &mechanisms.additional_tools {
            let name = tool.name();
            if !matches!(name.as_str(), "delegate_exploration" | "run_skill_script") {
                return Err(HarnessProfileError::UnexpectedAdditionalMechanism(name));
            }
        }
    }
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
                .main_mechanisms
                .map(|mechanisms| mechanisms.additional_tools)
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
            tools.push(Box::new(create_corpus_summary_tool(workspace_path.clone())));
            tools.push(Box::new(create_corpus_query_tool(workspace_path.clone())));
            tools.push(Box::new(create_corpus_neighbors_tool(
                workspace_path.clone(),
            )));
            tools.push(Box::new(create_context_search_tool(workspace_path)));
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

    let MainHarnessMechanisms {
        provider,
        model,
        api_key,
        external_path_approval,
        command_approval,
        additional_tools,
    } = request
        .main_mechanisms
        .ok_or(HarnessProfileError::MainMechanismsRequired)?;

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
    if session_mode_allows_source_edit(&workspace.session_mode) {
        tools.push(Box::new(create_source_edit_tool(workspace_path.clone())));
    }
    if workspace.corpus_available {
        tools.push(Box::new(create_corpus_summary_tool(workspace_path.clone())));
        tools.push(Box::new(create_corpus_query_tool(workspace_path.clone())));
        tools.push(Box::new(create_corpus_neighbors_tool(
            workspace_path.clone(),
        )));
        tools.push(Box::new(create_context_search_tool(workspace_path.clone())));
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
    tools.extend(additional_tools);

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

fn compose_preamble(
    role: AgentRole,
    workspace: Option<&WorkspaceToolContext>,
    role_guidance: Option<String>,
    matched_skills_section: Option<String>,
    invoked_skill_section: Option<String>,
    mechanism_names: &[String],
) -> Option<String> {
    let mut sections = Vec::new();
    if role == AgentRole::Main {
        push_section(&mut sections, invoked_skill_section);
        push_section(&mut sections, matched_skills_section);
    }

    if let Some(workspace) = workspace {
        let workspace_guidance = if role == AgentRole::Main
            && session_mode_allows_source_edit(&workspace.session_mode)
        {
            WORKSPACE_TOOLS_SYSTEM_PROMPT
        } else {
            READ_ONLY_WORKSPACE_TOOLS_SYSTEM_PROMPT
        };
        push_section(&mut sections, Some(workspace_guidance.to_string()));

        if role == AgentRole::Main {
            if !session_mode_allows_source_edit(&workspace.session_mode) {
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
        if mechanism_names
            .iter()
            .any(|name| name == "delegate_exploration")
        {
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
    use crate::session_mode::{SESSION_MODE_BUILD, SESSION_MODE_READ_ONLY};
    use rig::agent::AgentBuilder;
    use rig::test_utils::MockCompletionModel;
    use std::path::PathBuf;

    #[test]
    fn main_build_profile_contains_real_workspace_and_mutation_mechanisms() {
        let profile = resolve_harness_profile(HarnessProfileRequest {
            role: AgentRole::Main,
            workspace: Some(WorkspaceToolContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: false,
                session_mode: SESSION_MODE_BUILD.to_string(),
            }),
            role_guidance: None,
            matched_skills_section: None,
            invoked_skill_section: None,
            main_mechanisms: Some(MainHarnessMechanisms {
                provider: "openai".to_string(),
                model: "gpt-test".to_string(),
                api_key: "secret".to_string(),
                external_path_approval: None,
                command_approval: None,
                additional_tools: vec![],
            }),
        })
        .expect("profile");

        assert_eq!(
            profile.mechanism_names(),
            vec![
                "read_file",
                "search_code",
                "find_files",
                "list_directory",
                "write_harness_file",
                "run_review",
                "run_multi_review",
                "run_security_review",
                "record_review_outcome",
                "source_edit",
                "run_shell_command",
                "run_git_command",
                "run_github_cli_command",
            ]
        );
    }

    #[test]
    fn main_read_only_profile_keeps_harness_control_but_removes_source_edit() {
        let profile = resolve_harness_profile(HarnessProfileRequest {
            role: AgentRole::Main,
            workspace: Some(WorkspaceToolContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: false,
                session_mode: SESSION_MODE_READ_ONLY.to_string(),
            }),
            role_guidance: None,
            matched_skills_section: None,
            invoked_skill_section: None,
            main_mechanisms: Some(MainHarnessMechanisms {
                provider: "openai".to_string(),
                model: "gpt-test".to_string(),
                api_key: "secret".to_string(),
                external_path_approval: None,
                command_approval: None,
                additional_tools: vec![],
            }),
        })
        .expect("profile");

        let names = profile.mechanism_names();
        assert!(names.contains(&"write_harness_file".to_string()));
        assert!(!names.contains(&"source_edit".to_string()));
        let preamble = profile.preamble.expect("read-only preamble");
        assert!(preamble.contains("Read-Only Session"));
        assert!(preamble.contains("Harness Control Area"));
        assert!(!preamble.contains("Use `source_edit`"));
    }

    #[test]
    fn subordinate_profiles_are_read_only_and_add_corpus_mechanisms_coherently() {
        for role in [
            AgentRole::Exploration,
            AgentRole::Verification,
            AgentRole::ReviewDetector,
            AgentRole::ReviewValidator,
        ] {
            let profile = resolve_harness_profile(HarnessProfileRequest {
                role,
                workspace: Some(WorkspaceToolContext {
                    workspace_path: PathBuf::from("/tmp/workspace"),
                    corpus_available: true,
                    session_mode: SESSION_MODE_BUILD.to_string(),
                }),
                role_guidance: Some(format!("Guidance for {role:?}")),
                matched_skills_section: None,
                invoked_skill_section: None,
                main_mechanisms: None,
            })
            .expect("subordinate profile");

            assert_eq!(
                profile.mechanism_names(),
                vec![
                    "read_file",
                    "search_code",
                    "find_files",
                    "list_directory",
                    "corpus_summary",
                    "corpus_query",
                    "corpus_neighbors",
                    "context_search",
                ]
            );
            let preamble = profile.preamble.as_deref().expect("subordinate preamble");
            assert!(preamble.contains(&format!("Guidance for {role:?}")));
            assert!(preamble.contains("Codebase Knowledge"));
            assert!(!preamble.contains("Use `source_edit`"));
            assert!(!preamble.contains("Harness control artifacts remain available"));
            assert!(!profile
                .mechanism_names()
                .contains(&"write_harness_file".to_string()));
        }
    }

    #[test]
    fn safe_summary_is_derived_from_real_mechanisms_without_sensitive_content() {
        let profile = resolve_harness_profile(HarnessProfileRequest {
            role: AgentRole::Main,
            workspace: Some(WorkspaceToolContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: false,
                session_mode: SESSION_MODE_BUILD.to_string(),
            }),
            role_guidance: None,
            matched_skills_section: Some("private matched Skill text".to_string()),
            invoked_skill_section: None,
            main_mechanisms: Some(MainHarnessMechanisms {
                provider: "openai".to_string(),
                model: "gpt-test".to_string(),
                api_key: "sk-never-serialize".to_string(),
                external_path_approval: None,
                command_approval: None,
                additional_tools: vec![],
            }),
        })
        .expect("profile");

        let summary = profile.summary();
        assert_eq!(summary.role, AgentRole::Main);
        assert!(summary.workspace_available);
        assert!(summary.source_edit_enabled);
        assert!(!summary.corpus_enabled);
        assert_eq!(summary.mechanism_names, profile.mechanism_names());
        let serialized = serde_json::to_string(&summary).expect("safe summary JSON");
        assert!(!serialized.contains("sk-never-serialize"));
        assert!(!serialized.contains("private matched Skill text"));
    }

    #[test]
    fn steering_text_never_advertises_context_search_without_the_mechanism() {
        let profile = resolve_harness_profile(HarnessProfileRequest {
            role: AgentRole::Verification,
            workspace: Some(WorkspaceToolContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: false,
                session_mode: SESSION_MODE_BUILD.to_string(),
            }),
            role_guidance: Some("Verify the response".to_string()),
            matched_skills_section: None,
            invoked_skill_section: None,
            main_mechanisms: None,
        })
        .expect("profile");

        assert!(!profile
            .mechanism_names()
            .contains(&"context_search".to_string()));
        assert!(!profile
            .preamble
            .as_deref()
            .expect("preamble")
            .contains("`context_search`"));
    }

    #[test]
    fn only_main_can_resolve_without_an_active_workspace() {
        let main = resolve_harness_profile(HarnessProfileRequest {
            role: AgentRole::Main,
            workspace: None,
            role_guidance: None,
            matched_skills_section: Some("matched Skill guidance".to_string()),
            invoked_skill_section: None,
            main_mechanisms: None,
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
                main_mechanisms: None,
            })
            .err()
            .expect("missing workspace must fail");
            assert_eq!(error, HarnessProfileError::WorkspaceRequired { role });
        }
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
            let guards = guards_for_role(role);
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
            workspace: Some(WorkspaceToolContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: false,
                session_mode: SESSION_MODE_BUILD.to_string(),
            }),
            role_guidance: Some("Verify".to_string()),
            matched_skills_section: None,
            invoked_skill_section: None,
            main_mechanisms: None,
        })
        .expect("profile");

        let preamble = profile.preamble.unwrap_or_default();
        let _agent = AgentBuilder::new(MockCompletionModel::text("ok"))
            .preamble(&preamble)
            .default_max_turns(profile.guards.max_turns)
            .tools(profile.tools)
            .build();
    }

    #[test]
    fn additional_main_mechanisms_cannot_bypass_the_capability_ceiling() {
        let error = resolve_harness_profile(HarnessProfileRequest {
            role: AgentRole::Main,
            workspace: Some(WorkspaceToolContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                corpus_available: false,
                session_mode: SESSION_MODE_READ_ONLY.to_string(),
            }),
            role_guidance: None,
            matched_skills_section: None,
            invoked_skill_section: None,
            main_mechanisms: Some(MainHarnessMechanisms {
                provider: "openai".to_string(),
                model: "gpt-test".to_string(),
                api_key: "secret".to_string(),
                external_path_approval: None,
                command_approval: None,
                additional_tools: vec![Box::new(create_source_edit_tool(PathBuf::from(
                    "/tmp/workspace",
                )))],
            }),
        })
        .err()
        .expect("unexpected additional mechanism must fail");

        assert_eq!(
            error,
            HarnessProfileError::UnexpectedAdditionalMechanism("source_edit".to_string())
        );
    }
}
