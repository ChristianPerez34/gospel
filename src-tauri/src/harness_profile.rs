use crate::agents::{AgentDefinition, DEFAULT_MAX_TURNS};
use crate::bounty::{BountyHttpTool, BountySignalTool};
use crate::review::tools::{
    create_record_review_outcome_tool, create_run_multi_review_tool, create_run_review_tool,
    create_run_security_review_tool, REVIEW_TOOLS_SYSTEM_PROMPT,
};
use crate::shell_tools::CommandApproval;
use crate::shell_tools::{
    create_run_git_command_tool, create_run_github_cli_command_tool, create_run_shell_command_tool,
    SHELL_TOOLS_SYSTEM_PROMPT,
};
use crate::subagent::SubagentRelay;
use crate::workspace_tools::{
    build_base_workspace_tools, build_base_workspace_tools_with_external_approval,
    create_find_files_tool, create_list_directory_tool, create_read_file_tool,
    create_search_code_tool, create_source_edit_tool, create_write_harness_file_tool,
    ExternalPathApproval, HARNESS_CONTROL_AREA_SYSTEM_PROMPT,
    READ_ONLY_WORKSPACE_TOOLS_SYSTEM_PROMPT, WORKSPACE_TOOLS_SYSTEM_PROMPT,
};
use rig::tool::RegisteredTool;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

const DELEGATION_SYSTEM_PROMPT: &str = r#"
Use `delegate_exploration` only for broad multi-file, architectural, or investigative tasks that would benefit from a focused report before you answer the user.
Prefer direct file reads and targeted search for small or obvious tasks.
"#;

pub const MAIN_AGENT_AUTONOMY_PROMPT: &str = r#"
## Autonomous Operating Mandate

You are Gospel, an autonomous software engineering assistant working directly in the user's active codebase.

### Proactive Execution
- Take full ownership of driving tasks to completion. When the user asks what is next, how to proceed, or whether you can perform a task, do not reply with passive checklists or conversational deferrals. Inspect the workspace, decide the immediate engineering step, and execute it using your tools.
- Never ask the user to perform tasks that your available tools can execute (e.g. reading files, editing source code, compiling, running test suites, or checking git status).

### Desktop, GUI, and Manual Verification Discipline
- When working on desktop, web, or mobile applications, you may lack tools to directly see or click inside a graphical window.
- **Never allow a human-only visual or interactive verification step to stall you while actionable engineering work remains.**
- When direct visual testing is not supported by your tools:
  1. Write and run automated or headless tests (e.g. unit tests, integration tests, or framework test harnesses like `gpui::TestAppContext` for GPUI desktop apps or headless runners).
  2. Verify process execution, build artifacts, and CLI flags via shell tools where appropriate.
  3. Address outstanding compiler warnings, lint issues, edge cases, and code quality debts.
  4. If only manual user verification remains, ensure all code compiles, automated tests pass, and changes are cleanly organized before providing the user with a focused checklist to verify in the running app.

### Working Tree and Commit Hygiene
- Maintain clean git working tree hygiene. Avoid leaving large swathes of modified files uncommitted across multiple turns.
- Once a logical unit of work compiles, passes tests, and satisfies verification, inspect `git diff` and offer or make clean, well-described commits.
"#;

const BOUNTY_NETWORK_SYSTEM_PROMPT: &str = r#"
You have a scoped `http_request` tool. Every request is checked against `.gospel/bounty/scope.yaml` before it is sent. Out-of-scope requests are refused; do not attempt to work around the scope, and do not run shell commands to reach the network.

Use named `auth_profile` entries instead of handling raw credentials. Evidence and logs redact secrets. Prefer read-only methods; mutating requests require explicit scope opt-in. Record hypotheses and signals under `.gospel/bounty/` so a later run can resume.
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Main,
    Exploration,
    Verification,
    ReviewDetector,
    ReviewValidator,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveWorkspaceContext {
    pub workspace_path: std::path::PathBuf,
    /// Whether the Main agent's Harness Profile may register workspace source
    /// mutation tools (`source_edit`). Always `true` for Session turns;
    /// internal consumers such as skill optimization set `false` to replay a
    /// turn without write access.
    pub source_edit_allowed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LoopGuardPolicy {
    pub warning_threshold: usize,
    pub stop_threshold: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HarnessRunGuards {
    pub max_turns: Option<usize>,
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
    DelegateExploration(RegisteredTool),
    RunSkillScript(RegisteredTool),
}

impl MainToolContribution {
    fn into_tool(self) -> Result<RegisteredTool, HarnessProfileError> {
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
    /// Dynamic tools from connected MCP servers, already namespaced and
    /// approval-wired for this run.
    pub mcp_tools: Vec<RegisteredTool>,
    pub subagents: Option<Arc<SubagentRelay>>,
}

pub struct HarnessProfileRequest {
    pub role: AgentRole,
    pub workspace: Option<ActiveWorkspaceContext>,
    pub role_guidance: Option<String>,
    pub matched_skills_section: Option<String>,
    pub invoked_skill_section: Option<String>,
    pub memory_section: Option<String>,
    pub main_tool_inputs: Option<MainToolInputs>,
}

pub struct HarnessProfile {
    pub role: AgentRole,
    pub preamble: Option<String>,
    pub tools: Vec<RegisteredTool>,
    pub guards: HarnessRunGuards,
    workspace_available: bool,
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
                request.memory_section,
                &[],
            ),
            tools: request
                .main_tool_inputs
                .map(|inputs| {
                    let mut tools = inputs
                        .contributions
                        .into_iter()
                        .map(MainToolContribution::into_tool)
                        .collect::<Result<Vec<_>, _>>()?;
                    tools.extend(inputs.mcp_tools);
                    Ok::<_, HarnessProfileError>(tools)
                })
                .transpose()?
                .unwrap_or_default(),
            guards,
            workspace_available: false,
        });
    };

    if request.role != AgentRole::Main {
        let workspace_path = workspace.workspace_path.clone();
        let tools = build_base_workspace_tools(workspace_path.clone());
        let preamble = compose_preamble(
            request.role,
            Some(&workspace),
            request.role_guidance,
            None,
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
        });
    }

    let MainToolInputs {
        provider,
        model,
        api_key,
        external_path_approval,
        command_approval,
        contributions,
        mcp_tools,
        subagents,
    } = request
        .main_tool_inputs
        .ok_or(HarnessProfileError::MainToolInputsRequired)?;

    let workspace_path = workspace.workspace_path.clone();
    let mut tools = build_base_workspace_tools_with_external_approval(
        workspace_path.clone(),
        external_path_approval,
    );
    tools.push(RegisteredTool::from_tool(create_write_harness_file_tool(
        workspace_path.clone(),
    )));
    tools.push(RegisteredTool::from_tool(create_run_review_tool(
        workspace_path.clone(),
        provider.clone(),
        model.clone(),
        api_key.clone(),
        subagents.clone(),
    )));
    tools.push(RegisteredTool::from_tool(create_run_multi_review_tool(
        workspace_path.clone(),
        provider.clone(),
        model.clone(),
        api_key.clone(),
        subagents.clone(),
    )));
    tools.push(RegisteredTool::from_tool(create_run_security_review_tool(
        workspace_path.clone(),
        provider,
        model,
        api_key,
        subagents,
    )));
    tools.push(RegisteredTool::from_tool(
        create_record_review_outcome_tool(workspace_path.clone()),
    ));
    if workspace.source_edit_allowed {
        tools.push(RegisteredTool::from_tool(create_source_edit_tool(
            workspace_path.clone(),
        )));
    }
    // Shell tools stay registered on read-only turns: with no command
    // approval configured (e.g. skill-opt replays) mutating commands are denied.
    tools.push(RegisteredTool::from_tool(create_run_shell_command_tool(
        workspace_path.clone(),
        command_approval.clone(),
    )));
    tools.push(RegisteredTool::from_tool(create_run_git_command_tool(
        workspace_path.clone(),
        command_approval.clone(),
    )));
    tools.push(RegisteredTool::from_tool(
        create_run_github_cli_command_tool(workspace_path, command_approval),
    ));
    tools.extend(
        contributions
            .into_iter()
            .map(MainToolContribution::into_tool)
            .collect::<Result<Vec<_>, _>>()?,
    );
    tools.extend(mcp_tools);

    let preamble = compose_preamble(
        request.role,
        Some(&workspace),
        request.role_guidance,
        request.matched_skills_section,
        request.invoked_skill_section,
        request.memory_section,
        &tools.iter().map(|tool| tool.name()).collect::<Vec<_>>(),
    );

    Ok(HarnessProfile {
        role: request.role,
        preamble,
        tools,
        guards,
        workspace_available: true,
    })
}

pub struct CustomAgentProfileRequest {
    pub agent: AgentDefinition,
    pub workspace_path: PathBuf,
    pub run_id: String,
    pub attached_skills_section: Option<String>,
}

pub fn resolve_custom_agent_profile(request: CustomAgentProfileRequest) -> HarnessProfile {
    let tools = build_custom_agent_tools(&request.agent, &request.workspace_path, &request.run_id);
    let guards = HarnessRunGuards {
        max_turns: Some(request.agent.max_turns),
        deadline: request.agent.deadline,
        loop_guard: LoopGuardPolicy {
            warning_threshold: 3,
            stop_threshold: 5,
        },
    };

    let mut sections = Vec::new();
    push_section(
        &mut sections,
        Some(READ_ONLY_WORKSPACE_TOOLS_SYSTEM_PROMPT.to_string()),
    );
    push_section(
        &mut sections,
        Some(HARNESS_CONTROL_AREA_SYSTEM_PROMPT.to_string()),
    );
    if request.agent.network {
        push_section(
            &mut sections,
            Some(BOUNTY_NETWORK_SYSTEM_PROMPT.to_string()),
        );
    }
    push_section(&mut sections, Some(request.agent.body));
    push_section(&mut sections, request.attached_skills_section);

    HarnessProfile {
        role: AgentRole::Custom,
        preamble: (!sections.is_empty()).then(|| sections.join("\n\n")),
        tools,
        guards,
        workspace_available: true,
    }
}

fn build_custom_agent_tools(
    agent: &AgentDefinition,
    workspace_path: &Path,
    run_id: &str,
) -> Vec<RegisteredTool> {
    let mut tools = Vec::new();
    for name in &agent.tools {
        match name.as_str() {
            "read_file" => tools.push(RegisteredTool::from_tool(create_read_file_tool(
                workspace_path.to_path_buf(),
            ))),
            "search_code" => tools.push(RegisteredTool::from_tool(create_search_code_tool(
                workspace_path.to_path_buf(),
            ))),
            "find_files" => tools.push(RegisteredTool::from_tool(create_find_files_tool(
                workspace_path.to_path_buf(),
            ))),
            "list_directory" => tools.push(RegisteredTool::from_tool(
                create_list_directory_tool(workspace_path.to_path_buf()),
            )),
            "write_harness_file" => tools.push(RegisteredTool::from_tool(
                create_write_harness_file_tool(workspace_path.to_path_buf()),
            )),
            "http_request" if agent.network => {
                match BountyHttpTool::new(workspace_path, run_id) {
                    Ok(tool) => tools.push(RegisteredTool::from_tool(tool)),
                    Err(error) => tracing::warn!(
                        "Failed to register http_request for agent '{}': {}",
                        agent.name,
                        error
                    ),
                }
            }
            "record_bounty_signal" if agent.network => {
                match BountySignalTool::new(workspace_path, run_id) {
                    Ok(tool) => tools.push(RegisteredTool::from_tool(tool)),
                    Err(error) => tracing::warn!(
                        "Failed to register record_bounty_signal for agent '{}': {}",
                        agent.name,
                        error
                    ),
                }
            }
            "bounty_checkpoint" if agent.network => tools.push(
                RegisteredTool::from_tool(crate::bounty::BountyCheckpointTool::new(
                    crate::bounty::http_tool::run_dir(workspace_path, run_id),
                )),
            ),
            "bounty_surface" if agent.network => {
                match crate::bounty::surface::BountySurfaceTool::new(workspace_path, run_id) {
                    Ok(tool) => tools.push(RegisteredTool::from_tool(tool)),
                    Err(error) => tracing::warn!(
                        "Failed to register bounty_surface for agent '{}': {}",
                        agent.name,
                        error
                    ),
                }
            }
            "http_request" => tracing::warn!(
                "Agent '{}' lists http_request but network is false; skipping",
                agent.name
            ),
            "record_bounty_signal" => tracing::warn!(
                "Agent '{}' lists record_bounty_signal but network is false; skipping",
                agent.name
            ),
            "bounty_checkpoint" => tracing::warn!(
                "Agent '{}' lists bounty_checkpoint but network is false; skipping",
                agent.name
            ),
            "bounty_surface" => tracing::warn!(
                "Agent '{}' lists bounty_surface but network is false; skipping",
                agent.name
            ),
            other => tracing::warn!(
                "Agent '{}' lists unknown tool '{}'; skipping",
                agent.name,
                other
            ),
        }
    }
    tools
}

fn compose_preamble(
    role: AgentRole,
    workspace: Option<&ActiveWorkspaceContext>,
    role_guidance: Option<String>,
    matched_skills_section: Option<String>,
    invoked_skill_section: Option<String>,
    memory_section: Option<String>,
    tool_names: &[String],
) -> Option<String> {
    let mut sections = Vec::new();
    if role == AgentRole::Main {
        push_section(&mut sections, invoked_skill_section);
        push_section(&mut sections, matched_skills_section);
        push_section(&mut sections, memory_section);
    }

    if let Some(workspace) = workspace {
        if role == AgentRole::Main {
            push_section(&mut sections, Some(MAIN_AGENT_AUTONOMY_PROMPT.to_string()));
        }

        let workspace_guidance = if role == AgentRole::Main && workspace.source_edit_allowed {
            WORKSPACE_TOOLS_SYSTEM_PROMPT
        } else {
            READ_ONLY_WORKSPACE_TOOLS_SYSTEM_PROMPT
        };
        push_section(&mut sections, Some(workspace_guidance.to_string()));

        if role == AgentRole::Main {
            push_section(&mut sections, Some(SHELL_TOOLS_SYSTEM_PROMPT.to_string()));
            push_section(&mut sections, Some(REVIEW_TOOLS_SYSTEM_PROMPT.to_string()));
            push_section(
                &mut sections,
                Some(HARNESS_CONTROL_AREA_SYSTEM_PROMPT.to_string()),
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
            max_turns: None,
            deadline: None,
            loop_guard: LoopGuardPolicy {
                warning_threshold: 3,
                stop_threshold: 5,
            },
        },
        AgentRole::Exploration => HarnessRunGuards {
            max_turns: None,
            deadline: Some(Duration::from_secs(90)),
            loop_guard: LoopGuardPolicy {
                warning_threshold: 3,
                stop_threshold: 5,
            },
        },
        AgentRole::Verification => HarnessRunGuards {
            max_turns: None,
            deadline: Some(Duration::from_secs(90)),
            loop_guard: LoopGuardPolicy {
                warning_threshold: 2,
                stop_threshold: 3,
            },
        },
        AgentRole::ReviewDetector => HarnessRunGuards {
            max_turns: None,
            deadline: Some(Duration::from_secs(60)),
            loop_guard: LoopGuardPolicy {
                warning_threshold: 2,
                stop_threshold: 3,
            },
        },
        AgentRole::ReviewValidator => HarnessRunGuards {
            max_turns: None,
            deadline: Some(Duration::from_secs(30)),
            loop_guard: LoopGuardPolicy {
                warning_threshold: 2,
                stop_threshold: 3,
            },
        },
        AgentRole::Custom => HarnessRunGuards {
            max_turns: Some(DEFAULT_MAX_TURNS),
            deadline: None,
            loop_guard: LoopGuardPolicy {
                warning_threshold: 3,
                stop_threshold: 5,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            mcp_tools: vec![],
            subagents: None,
        }
    }

    #[test]
    fn main_receives_mcp_tools_with_and_without_a_workspace() {
        for workspace_available in [false, true] {
            let mut request = request(AgentRole::Main, workspace_available, true);
            let tool = rig::tool::DynamicTool::new(
                rig::message::ToolName::new("mcp_test__search").unwrap(),
                "Search the remote server",
                serde_json::json!({ "type": "object", "properties": {} }),
                |_| Box::pin(async { Ok(serde_json::json!({"found": true}).into()) }),
            );
            request.main_tool_inputs.as_mut().unwrap().mcp_tools =
                vec![RegisteredTool::from_dynamic(tool)];
            let profile = resolve_harness_profile(request).unwrap();
            assert!(profile
                .tool_names()
                .contains(&"mcp_test__search".to_string()));
            assert_eq!(profile.workspace_available, workspace_available);
        }
    }

    fn request(
        role: AgentRole,
        workspace_available: bool,
        source_edit_allowed: bool,
    ) -> HarnessProfileRequest {
        HarnessProfileRequest {
            role,
            workspace: workspace_available.then(|| ActiveWorkspaceContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                source_edit_allowed,
            }),
            role_guidance: Some(format!("Guidance for {role:?}")),
            matched_skills_section: (role == AgentRole::Main)
                .then(|| "private matched Skill text".to_string()),
            invoked_skill_section: None,
            memory_section: None,
            main_tool_inputs: (role == AgentRole::Main).then(main_tool_inputs),
        }
    }

    fn expected_tool_names(
        role: AgentRole,
        workspace_available: bool,
        source_edit_allowed: bool,
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
            if source_edit_allowed {
                names.push("source_edit");
            }
            names.extend([
                "run_shell_command",
                "run_git_command",
                "run_github_cli_command",
            ]);
        }
        names.into_iter().map(str::to_string).collect()
    }

    #[test]
    fn contract_matrix_covers_roles_and_source_edit() {
        let roles = [
            AgentRole::Main,
            AgentRole::Exploration,
            AgentRole::Verification,
            AgentRole::ReviewDetector,
            AgentRole::ReviewValidator,
        ];
        for role in roles {
            for workspace_available in [false, true] {
                for source_edit_allowed in [true, false] {
                    let result = resolve_harness_profile(request(
                        role,
                        workspace_available,
                        source_edit_allowed,
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
                        expected_tool_names(role, workspace_available, source_edit_allowed,)
                    );
                    assert_eq!(summary.role, role);
                    assert_eq!(summary.workspace_available, workspace_available);
                    assert_eq!(summary.tool_names, names);
                    assert_eq!(summary.guards, profile.guards);

                    let source_edit_expected =
                        workspace_available && role == AgentRole::Main && source_edit_allowed;
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

                    let preamble = profile.preamble.unwrap_or_default();
                    assert!(preamble.contains(&format!("Guidance for {role:?}")));
                    assert_eq!(
                        preamble.contains("private matched Skill text"),
                        role == AgentRole::Main
                    );
                    assert_eq!(preamble.contains("Use `source_edit`"), source_edit_expected);
                    if workspace_available && role == AgentRole::Main {
                        assert!(preamble.contains("Autonomous Operating Mandate"));
                        assert!(preamble.contains("Harness Control Area"));
                        assert!(preamble.contains("Shell, Git, and GitHub CLI Tools"));
                        assert!(preamble.contains("Review Tools"));
                    } else {
                        assert!(!preamble.contains("Autonomous Operating Mandate"));
                        assert!(!preamble.contains("Harness Control Area"));
                        assert!(!preamble.contains("Shell, Git, and GitHub CLI Tools"));
                        assert!(!preamble.contains("Review Tools"));
                    }

                    let serialized = serde_json::to_string(&summary).expect("summary JSON");
                    assert!(!serialized.contains("sk-never-serialize"));
                    assert!(!serialized.contains("private matched Skill text"));
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
            memory_section: None,
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
                memory_section: None,
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
                source_edit_allowed: true,
            }),
            role_guidance: None,
            matched_skills_section: None,
            invoked_skill_section: None,
            memory_section: None,
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
            .push(MainToolContribution::DelegateExploration(
                RegisteredTool::from_tool(create_source_edit_tool(PathBuf::from("/tmp/workspace"))),
            ));
        let error = resolve_harness_profile(HarnessProfileRequest {
            role: AgentRole::Main,
            workspace: Some(ActiveWorkspaceContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                source_edit_allowed: true,
            }),
            role_guidance: None,
            matched_skills_section: None,
            invoked_skill_section: None,
            memory_section: None,
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
    fn custom_agent_profile_uses_definition_tools_and_guards() {
        let workspace = tempfile::tempdir().unwrap();
        let agent = AgentDefinition {
            name: "bug-bounty".to_string(),
            description: "test".to_string(),
            source: crate::agents::AgentSource::Workspace,
            body: "You are the bounty agent.".to_string(),
            tools: vec![
                "read_file".to_string(),
                "http_request".to_string(),
                "unknown".to_string(),
            ],
            skills: Vec::new(),
            model: None,
            worker_model: None,
            orchestration: None,
            max_turns: 12,
            deadline: Some(Duration::from_secs(90)),
            network: true,
        };
        let profile = resolve_custom_agent_profile(CustomAgentProfileRequest {
            agent,
            workspace_path: workspace.path().to_path_buf(),
            run_id: "test".to_string(),
            attached_skills_section: Some("## Attached Skills\n\n- recon".to_string()),
        });

        assert_eq!(profile.role, AgentRole::Custom);
        assert_eq!(profile.guards.max_turns, Some(12));
        assert_eq!(profile.guards.deadline, Some(Duration::from_secs(90)));
        let names = profile.tool_names();
        assert!(names.contains(&"read_file".to_string()));
        assert!(names.contains(&"http_request".to_string()));
        assert!(!names.contains(&"unknown".to_string()));
        assert!(!names.contains(&"source_edit".to_string()));
        assert!(!names.contains(&"run_shell_command".to_string()));
        let preamble = profile.preamble.unwrap_or_default();
        assert!(preamble.contains("You are the bounty agent."));
        assert!(preamble.contains("Attached Skills"));
        assert!(preamble.contains("http_request"));
    }

    #[test]
    fn role_guards_are_resolved_with_the_profile() {
        let expected = [
            (AgentRole::Main, None, None, 3, 5),
            (AgentRole::Exploration, None, Some(90), 3, 5),
            (AgentRole::Verification, None, Some(90), 2, 3),
            (AgentRole::ReviewDetector, None, Some(60), 2, 3),
            (AgentRole::ReviewValidator, None, Some(30), 2, 3),
            (AgentRole::Custom, Some(DEFAULT_MAX_TURNS), None, 3, 5),
        ];

        for (role, max_turns, deadline_seconds, warning, stop) in expected {
            let profile = resolve_harness_profile(request(role, true, true)).expect("profile");
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

    #[tokio::test]
    async fn resolved_profile_can_be_consumed_by_the_ecs_runtime() {
        let profile = resolve_harness_profile(HarnessProfileRequest {
            role: AgentRole::Verification,
            workspace: Some(ActiveWorkspaceContext {
                workspace_path: PathBuf::from("/tmp/workspace"),
                source_edit_allowed: true,
            }),
            role_guidance: Some("Verify".to_string()),
            matched_skills_section: None,
            invoked_skill_section: None,
            memory_section: None,
            main_tool_inputs: None,
        })
        .expect("profile");

        let preamble = profile.preamble.unwrap_or_default();
        let result = crate::ecs_runtime::run(
            crate::ecs_runtime::RunRequest {
                session_id: None,
                model: MockCompletionModel::from_stream_turns([vec![
                    rig::test_utils::MockStreamEvent::text("ok"),
                    rig::test_utils::MockStreamEvent::final_response_with_default_usage(),
                ]])
                .into(),
                prompt: "Verify".into(),
                preamble: Some(preamble),
                tools: profile.tools,
                history: Vec::new(),
                guards: profile.guards,
                additional_params: None,
                reasoning: None,
                subagents: None,
            },
            |_| {},
        )
        .await
        .expect("ECS executes the profile");
        assert_eq!(result.full_response, "ok");
    }

    #[test]
    fn memory_section_is_a_main_preamble_sibling_and_ignored_for_other_roles() {
        let mut main_request = request(AgentRole::Main, false, true);
        main_request.memory_section = Some("## Memory\nprefer bun".to_string());
        let main = resolve_harness_profile(main_request).expect("unscoped Main profile");
        let main_preamble = main.preamble.unwrap_or_default();
        assert!(main_preamble.contains("private matched Skill text"));
        assert!(main_preamble.contains("## Memory\nprefer bun"));
        assert!(
            main_preamble.find("private matched Skill text").unwrap()
                < main_preamble.find("## Memory\nprefer bun").unwrap()
        );

        for role in [
            AgentRole::Exploration,
            AgentRole::Verification,
            AgentRole::ReviewDetector,
            AgentRole::ReviewValidator,
        ] {
            let mut other = request(role, true, true);
            other.memory_section = Some("## Memory\nprefer bun".to_string());
            let profile = resolve_harness_profile(other).expect("workspace profile");
            let preamble = profile.preamble.unwrap_or_default();
            assert!(
                !preamble.contains("## Memory"),
                "{role:?} preamble must not include recalled Memory"
            );
        }
    }
}
