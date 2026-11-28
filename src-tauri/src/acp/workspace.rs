//! Workspace containment, filesystem policy, and approval classification for
//! ACP agent capabilities.
//!
//! The policy mirrors Gospel's own workspace tools: writes go through the same
//! blocked-path rules as `source_edit` (`.gospel/`, secret-like files, hidden
//! control directories, lockfiles, generated/noisy paths) plus symlink
//! containment, and shell commands are classified by the same `CommandPolicy`
//! that gates `run_shell_command` — including path-escape analysis that
//! decides whether a command is eligible for always-allow persistence.

use std::path::{Path, PathBuf};

use agent_client_protocol::v1::schema::{RequestPermissionRequest, ToolKind};
use serde_json::Value;

use crate::approval_broker::{split_compound_commands, ApprovalRequest};
use crate::shell_tools::{
    executable_name, has_shell_path_escape, CommandPolicy, CommandRisk, CommandSafety,
};
use crate::symlink_guard;
use crate::workspace_tools::source_edit_blocked_path_reason;

pub const MAX_FS_READ_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_FS_WRITE_BYTES: usize = 4 * 1024 * 1024;

/// Resolves `requested` against the workspace root with symlink containment.
/// Writes additionally apply the shared source-edit blocked-path policy.
pub fn resolve_workspace_path(
    workspace_root: &Path,
    requested: &Path,
    for_write: bool,
) -> Result<PathBuf, String> {
    let canonical_root = symlink_guard::canonical(workspace_root)
        .map_err(|e| format!("Invalid workspace root: {e}"))?;
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        canonical_root.join(requested)
    };

    if for_write {
        symlink_guard::validate_existing_ancestors(&canonical_root, &candidate)
            .map_err(|e| format!("Workspace containment rejected write: {e}"))?;
        if let Ok(relative) = candidate.strip_prefix(&canonical_root) {
            if let Some(reason) = source_edit_blocked_path_reason(relative) {
                return Err(format!("{} ({})", reason, requested.display()));
            }
        } else {
            return Err(format!(
                "Path {} escapes active workspace",
                requested.display()
            ));
        }
        Ok(candidate)
    } else {
        let resolved = symlink_guard::canonical(&candidate)
            .map_err(|e| format!("Failed to resolve {}: {e}", requested.display()))?;
        if !symlink_guard::is_within(&canonical_root, &resolved) {
            return Err(format!(
                "Path {} escapes active workspace",
                requested.display()
            ));
        }
        if is_secret_like_workspace_read(&canonical_root, &resolved) {
            return Err(format!(
                "Refusing to read secret-like file {}",
                requested.display()
            ));
        }
        Ok(resolved)
    }
}

fn is_secret_like_workspace_read(root: &Path, resolved: &Path) -> bool {
    resolved
        .strip_prefix(root)
        .map(crate::workspace_tools::is_secret_like)
        .unwrap_or(false)
}

/// How a shell command string should flow through approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpCommandVerdict {
    /// Every compound segment classifies read-only: auto-approve without a
    /// prompt, matching how `run_shell_command` executes read-only commands
    /// directly.
    ReadOnly,
    RequiresApproval {
        risk: CommandRisk,
        workspace_scoped: bool,
    },
}

/// Classifies a shell command string the way Gospel's own shell tool would:
/// compound segments are split and shell-parsed (quote-aware), each segment is
/// classified by `CommandPolicy`, and workspace scoping requires every segment
/// to stay inside the workspace. Anything unparseable is treated as mutating
/// and never workspace-scoped.
pub fn classify_acp_command(command: &str, workspace_root: Option<&Path>) -> AcpCommandVerdict {
    let root = workspace_root.unwrap_or_else(|| Path::new("."));
    let mut worst_safety: Option<CommandSafety> = None;
    let mut all_scoped = workspace_root.is_some();
    let mut any_segment = false;

    for segment in split_compound_commands(command) {
        let Ok(parts) = shell_words::split(segment) else {
            return AcpCommandVerdict::RequiresApproval {
                risk: CommandRisk::Mutating,
                workspace_scoped: false,
            };
        };
        let Some((program, args)) = parts.split_first() else {
            continue;
        };
        any_segment = true;
        if executable_name(program) != program.as_str()
            || has_shell_path_escape(program, args, root).is_some()
        {
            all_scoped = false;
        }
        let safety = CommandPolicy.classify_shell(program, args, root);
        worst_safety = Some(match (worst_safety, safety) {
            (None, s) => s,
            (Some(CommandSafety::Restricted(r)), CommandSafety::Restricted(_)) => {
                CommandSafety::Restricted(r)
            }
            (Some(CommandSafety::Restricted(r)), _) => CommandSafety::Restricted(r),
            (_, CommandSafety::Restricted(r)) => CommandSafety::Restricted(r),
            (Some(CommandSafety::Blocked(r)), CommandSafety::Blocked(_)) => {
                CommandSafety::Blocked(r)
            }
            (Some(CommandSafety::Blocked(r)), _) => CommandSafety::Blocked(r),
            (_, CommandSafety::Blocked(r)) => CommandSafety::Blocked(r),
            (Some(CommandSafety::Destructive), _) => CommandSafety::Destructive,
            (_, CommandSafety::Destructive) => CommandSafety::Destructive,
            (Some(CommandSafety::Mutating), _) => CommandSafety::Mutating,
            (_, CommandSafety::Mutating) => CommandSafety::Mutating,
            _ => CommandSafety::ReadOnly,
        });
    }

    if !any_segment {
        return AcpCommandVerdict::RequiresApproval {
            risk: CommandRisk::Restricted,
            workspace_scoped: false,
        };
    }

    match worst_safety.unwrap_or(CommandSafety::ReadOnly) {
        CommandSafety::ReadOnly => AcpCommandVerdict::ReadOnly,
        CommandSafety::Mutating => AcpCommandVerdict::RequiresApproval {
            risk: CommandRisk::Mutating,
            workspace_scoped: all_scoped,
        },
        CommandSafety::Destructive => AcpCommandVerdict::RequiresApproval {
            risk: CommandRisk::Destructive,
            workspace_scoped: false,
        },
        CommandSafety::Restricted(_) | CommandSafety::Blocked(_) => {
            AcpCommandVerdict::RequiresApproval {
                risk: CommandRisk::Restricted,
                workspace_scoped: false,
            }
        }
    }
}

/// Builds the Gospel approval request for one ACP permission request. Execute
/// tools are classified as shell commands; file-mutating tools map to
/// workspace or external-path requests; everything else is a mutating command
/// request keyed on the tool title.
pub fn build_approval_request(
    params: &RequestPermissionRequest,
    workspace_root: Option<&Path>,
    run_id: Option<&str>,
) -> ApprovalRequest {
    let kind = params.tool_call.fields.kind;
    let raw_input = params.tool_call.fields.raw_input.as_ref();
    let title = params.tool_call.fields.title.as_deref().unwrap_or("tool");

    if kind == Some(ToolKind::Execute) {
        let command = raw_input
            .and_then(|v| {
                v.get("command")
                    .or_else(|| v.get("cmd"))
                    .and_then(Value::as_str)
            })
            .unwrap_or(title);
        let (risk, workspace_scoped) = match classify_acp_command(command, workspace_root) {
            AcpCommandVerdict::ReadOnly => (CommandRisk::Mutating, false),
            AcpCommandVerdict::RequiresApproval {
                risk,
                workspace_scoped,
            } => (risk, workspace_scoped),
        };
        return ApprovalRequest::command(
            "factory_droid",
            command,
            format!("Factory Droid requested permission: {title}"),
            risk,
            run_id,
        )
        .with_workspace_scope(workspace_scoped);
    }

    let extracted_path = raw_input
        .and_then(|v| {
            v.get("path")
                .or_else(|| v.get("file_path"))
                .or_else(|| v.get("filePath"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            params
                .tool_call
                .fields
                .locations
                .as_ref()
                .and_then(|locs| locs.first())
                .and_then(|loc| loc.path.to_str())
        });

    if let Some(path_str) = extracted_path {
        let is_inside = workspace_root
            .and_then(|root| resolve_workspace_path(root, Path::new(path_str), true).ok())
            .is_some();
        if !is_inside {
            return ApprovalRequest::external_path(
                "factory_droid",
                path_str,
                format!("Factory Droid requested access outside workspace: {title}"),
                run_id,
            );
        }
    }

    let risk = if kind == Some(ToolKind::Delete) {
        CommandRisk::Destructive
    } else {
        CommandRisk::Mutating
    };
    ApprovalRequest::command(
        "factory_droid",
        title.to_string(),
        "Factory Droid requested permission to modify workspace files or state.",
        risk,
        run_id,
    )
    .with_workspace_scope(workspace_root.is_some())
}

/// True when an execute-kind permission request classifies read-only end to
/// end and can be auto-approved without involving the broker.
pub fn is_read_only_execute(params: &RequestPermissionRequest, workspace_root: Option<&Path>) -> bool {
    if params.tool_call.fields.kind != Some(ToolKind::Execute) {
        return false;
    }
    let command = params
        .tool_call
        .fields
        .raw_input
        .as_ref()
        .and_then(|v| v.get("command").or_else(|| v.get("cmd")).and_then(Value::as_str))
        .unwrap_or("");
    classify_acp_command(command, workspace_root) == AcpCommandVerdict::ReadOnly
}

pub fn slice_lines(text: &str, line: Option<u32>, limit: Option<u32>) -> String {
    if line.is_none() && limit.is_none() {
        return text.to_string();
    }
    let start = line.unwrap_or(1).saturating_sub(1) as usize;
    let max_lines = limit.map(|l| l as usize).unwrap_or(usize::MAX);
    text.lines()
        .skip(start)
        .take(max_lines)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_commands_auto_approve() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            classify_acp_command("ls -la", Some(dir.path())),
            AcpCommandVerdict::ReadOnly
        );
        assert_eq!(
            classify_acp_command("cat foo.txt && rg pattern src/", Some(dir.path())),
            AcpCommandVerdict::ReadOnly
        );
    }

    #[test]
    fn mutating_in_workspace_is_scoped() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            classify_acp_command("touch src/new_file.txt", Some(dir.path())),
            AcpCommandVerdict::RequiresApproval {
                risk: CommandRisk::Mutating,
                workspace_scoped: true,
            }
        );
    }

    #[test]
    fn workspace_escaping_mutation_is_not_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let verdict = classify_acp_command("cp x.txt /tmp/escape.txt", Some(dir.path()));
        assert_eq!(
            verdict,
            AcpCommandVerdict::RequiresApproval {
                risk: CommandRisk::Mutating,
                workspace_scoped: false,
            }
        );
    }

    #[test]
    fn compound_command_with_escape_is_not_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let verdict = classify_acp_command("touch a.txt && cp a.txt ../outside.txt", Some(dir.path()));
        match verdict {
            AcpCommandVerdict::RequiresApproval { workspace_scoped, .. } => {
                assert!(!workspace_scoped)
            }
            other => panic!("expected approval, got {other:?}"),
        }
    }

    #[test]
    fn quoted_paths_shell_parse_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let verdict = classify_acp_command("touch \"src/my file.txt\"", Some(dir.path()));
        match verdict {
            AcpCommandVerdict::RequiresApproval { workspace_scoped, .. } => assert!(workspace_scoped),
            other => panic!("expected scoped approval, got {other:?}"),
        }
    }

    #[test]
    fn destructive_and_restricted_never_scoped() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            classify_acp_command("rm -rf /", Some(dir.path())),
            AcpCommandVerdict::RequiresApproval {
                risk: CommandRisk::Restricted,
                workspace_scoped: false,
            }
        );
    }

}
