use crate::text_utils::truncate_text_bytes;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::ffi::OsStr;
use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

const COMMAND_OUTPUT_CAP: usize = 32 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

pub const SHELL_TOOLS_SYSTEM_PROMPT: &str = r#"
## Shell, Git, and GitHub CLI Tools

You can run shell, git, and GitHub CLI commands in the active workspace.

### Available Tools

- `run_shell_command`: Run a non-shell program with arguments in the active workspace. Shell interpreters (`sh`, `bash`, `zsh`, etc.) and shell metacharacters are blocked.
- `run_git_command`: Run a git command in the active workspace.
- `run_github_cli_command`: Run a `gh` command in the active workspace.

### Safety Rules

- Read-only commands run directly.
- Mutating or destructive commands require one-time user approval before execution.
- The following are always blocked:
  - `rm -rf /` or `rm -rf /*`
  - Any command containing shell metacharacters (`;`, `|`, `&`, `$`, `` ` ``, `<`, `>`, newline, carriage return, NUL)
  - `git push --force`, `git push -f`, `git reset --hard`, `git clean`
  - `gh repo delete`
- Do not attempt to bypass these rules with encoded characters or indirect invocation.

### Guidance

- Prefer `run_git_command` for git operations instead of `run_shell_command`.
- Prefer `run_github_cli_command` for GitHub operations instead of `run_shell_command`.
- Always verify important claims with live workspace tools when possible.
- When a command is blocked or denied, report the reason to the user and ask how to proceed.
"#;

#[derive(Debug, Error)]
pub enum ShellToolError {
    #[error("workspace unavailable: {0}")]
    WorkspaceUnavailable(String),
    #[error("command execution failed: {0}")]
    Execution(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSafety {
    ReadOnly,
    Mutating,
    Destructive,
    Blocked(String),
}

/// Per-workspace policy used to classify commands before execution.
///
/// Phase 1 ships with conservative hard-coded rules. Later phases can extend
/// this struct to load allowlists/denylists from `.gospel/shell-policy.json`.
#[derive(Debug, Clone, Default)]
pub struct CommandPolicy;

impl CommandPolicy {
    /// Shared safety checks for both top-level and `env`-wrapped shell commands.
    fn common_shell_safety(
        &self,
        executable: &str,
        program: &str,
        args: &[String],
        workspace_root: &Path,
    ) -> Option<CommandSafety> {
        let executable_lower = executable.to_ascii_lowercase();

        // Block shell interpreters to prevent shell-injection attacks.
        if is_shell_interpreter(&executable_lower) {
            return Some(CommandSafety::Blocked("shell_interpreter".to_string()));
        }

        // Block shell metacharacters and secret-like paths anywhere in the invocation.
        if contains_shell_metacharacter(program) {
            return Some(CommandSafety::Blocked("shell_metacharacter".to_string()));
        }
        if let Some(safety) = blocked_argument_safety(args) {
            return Some(safety);
        }

        if executable_lower == "env" {
            return Some(self.classify_env_shell_wrapper(args, workspace_root));
        }

        // Hard-block known destructive patterns.
        if is_blocked_shell_pattern(&executable_lower, args) {
            return Some(CommandSafety::Blocked("dangerous_command".to_string()));
        }

        None
    }

    pub fn classify_shell(
        &self,
        program: &str,
        args: &[String],
        workspace_root: &Path,
    ) -> CommandSafety {
        let program = program.trim();
        if program.is_empty() {
            return CommandSafety::Blocked("empty_program".to_string());
        }

        let executable = executable_name(program);
        let executable_lower = executable.to_ascii_lowercase();
        let program_is_bare_name = executable == program;

        if let Some(safety) = self.common_shell_safety(executable, program, args, workspace_root) {
            return safety;
        }

        let promote_git_gh_safety = |safety: CommandSafety| {
            if program_is_bare_name || matches!(safety, CommandSafety::Blocked(_)) {
                safety
            } else {
                CommandSafety::Mutating
            }
        };

        if executable_lower == "git" {
            return promote_git_gh_safety(self.classify_git(args, workspace_root));
        }

        if executable_lower == "gh" {
            return promote_git_gh_safety(self.classify_gh(args, workspace_root));
        }

        if executable_lower == "find" {
            if let Some(safety) = classify_find_action(args) {
                return safety;
            }
        }

        // Workspace-escaping paths require approval.
        if has_shell_path_escape(&executable_lower, args, workspace_root).is_some() {
            return CommandSafety::Mutating;
        }

        // Small read-only allowlist runs directly.
        if program_is_bare_name && is_read_only_shell_program(&executable_lower, args) {
            return CommandSafety::ReadOnly;
        }

        // Everything else is treated as mutating and requires approval.
        CommandSafety::Mutating
    }

    fn classify_env_shell_wrapper(&self, args: &[String], workspace_root: &Path) -> CommandSafety {
        if let Some(split_args) = parse_env_split_args(args) {
            let Some((program, wrapped_args)) = split_args.split_first() else {
                return CommandSafety::Mutating;
            };
            return self.classify_wrapped_shell_command(program, wrapped_args, workspace_root);
        }

        let Some((program, wrapped_args)) = parse_env_wrapped_command(args) else {
            return CommandSafety::Mutating;
        };
        self.classify_wrapped_shell_command(program, wrapped_args, workspace_root)
    }

    fn classify_wrapped_shell_command(
        &self,
        program: &str,
        args: &[String],
        workspace_root: &Path,
    ) -> CommandSafety {
        let executable = executable_name(program);
        let executable_lower = executable.to_ascii_lowercase();

        if let Some(safety) = self.common_shell_safety(executable, program, args, workspace_root) {
            return safety;
        }

        let wrapped_safety = match executable_lower.as_str() {
            "git" => self.classify_git(args, workspace_root),
            "gh" => self.classify_gh(args, workspace_root),
            _ => CommandSafety::Mutating,
        };
        if matches!(wrapped_safety, CommandSafety::Blocked(_)) {
            wrapped_safety
        } else {
            CommandSafety::Mutating
        }
    }

    pub fn classify_git(&self, args: &[String], workspace_root: &Path) -> CommandSafety {
        if args.is_empty() {
            return CommandSafety::Blocked("empty_git_command".to_string());
        }

        if let Some(safety) = blocked_argument_safety(args) {
            return safety;
        }

        let subcommand = args[0].to_ascii_lowercase();

        // Hard-block destructive git options.
        if subcommand == "reset" && args.iter().any(|a| a == "--hard") {
            return CommandSafety::Blocked("git_reset_hard".to_string());
        }
        if subcommand == "clean" {
            return CommandSafety::Blocked("git_clean".to_string());
        }
        if subcommand == "push" && args.iter().any(|a| a == "--force" || a == "-f") {
            return CommandSafety::Blocked("git_push_force".to_string());
        }
        if subcommand == "checkout" && args.iter().any(|a| a == "-f" || a == "--force") {
            return CommandSafety::Destructive;
        }

        if has_path_escape(args, workspace_root).is_some() {
            return CommandSafety::Mutating;
        }

        // Read-only subcommands.
        if matches!(
            subcommand.as_str(),
            "status"
                | "log"
                | "diff"
                | "show"
                | "blame"
                | "ls-files"
                | "ls-tree"
                | "rev-parse"
                | "describe"
        ) {
            return CommandSafety::ReadOnly;
        }

        if subcommand == "remote" {
            return if git_remote_is_read_only(args) {
                CommandSafety::ReadOnly
            } else {
                CommandSafety::Mutating
            };
        }

        if subcommand == "config" {
            // `git config --get ...` is read-only; `git config section.key value` writes.
            if args
                .iter()
                .any(|a| a == "--get" || a == "--get-all" || a == "--list")
            {
                return CommandSafety::ReadOnly;
            }
            return CommandSafety::Mutating;
        }

        if subcommand == "stash" {
            return if matches!(
                args.get(1).map(|s| s.as_str()),
                Some("list" | "show" | "-h" | "--help")
            ) {
                CommandSafety::ReadOnly
            } else {
                CommandSafety::Mutating
            };
        }

        if subcommand == "tag" {
            return if args
                .iter()
                .any(|a| a == "-d" || a == "--delete" || a == "-f" || a == "--force")
            {
                CommandSafety::Destructive
            } else if args
                .iter()
                .skip(1)
                .any(|a| a == "-l" || a == "--list" || a.starts_with("-n"))
            {
                CommandSafety::ReadOnly
            } else if args.iter().skip(1).any(|a| !a.starts_with('-')) {
                // Creating a tag is mutating.
                CommandSafety::Mutating
            } else {
                CommandSafety::ReadOnly
            };
        }

        if subcommand == "branch" {
            return if args
                .iter()
                .any(|a| a == "-D" || a == "-d" || a == "--delete" || a == "-M" || a == "-m")
            {
                CommandSafety::Destructive
            } else if args.len() > 1 && !args[1].starts_with('-') {
                CommandSafety::Mutating
            } else {
                CommandSafety::ReadOnly
            };
        }

        // Mutating subcommands.
        if matches!(
            subcommand.as_str(),
            "add"
                | "commit"
                | "checkout"
                | "switch"
                | "restore"
                | "merge"
                | "rebase"
                | "pull"
                | "fetch"
                | "push"
                | "init"
                | "clone"
                | "cherry-pick"
        ) {
            return CommandSafety::Mutating;
        }

        // Destructive subcommands.
        if matches!(subcommand.as_str(), "reset" | "revert" | "rm") {
            return CommandSafety::Destructive;
        }

        CommandSafety::Mutating
    }

    pub fn classify_gh(&self, args: &[String], workspace_root: &Path) -> CommandSafety {
        if args.is_empty() {
            return CommandSafety::Blocked("empty_gh_command".to_string());
        }

        if let Some(safety) = blocked_argument_safety(args) {
            return safety;
        }

        let first = args[0].to_ascii_lowercase();
        let second = args.get(1).map(|s| s.to_ascii_lowercase());

        if first == "repo" && second.as_deref() == Some("delete") {
            return CommandSafety::Blocked("gh_repo_delete".to_string());
        }

        if has_path_escape(args, workspace_root).is_some() {
            return CommandSafety::Mutating;
        }

        // Read-only top-level commands.
        if matches!(first.as_str(), "status" | "version") {
            return CommandSafety::ReadOnly;
        }

        if first == "auth" {
            return match second.as_deref() {
                Some("status") => CommandSafety::ReadOnly,
                Some("token") => CommandSafety::Blocked("gh_auth_token".to_string()),
                _ => CommandSafety::Mutating,
            };
        }

        if first == "api" {
            // `gh api` is read-only by default; mutating HTTP methods require approval.
            return if gh_api_uses_mutating_method(args) {
                CommandSafety::Mutating
            } else {
                CommandSafety::ReadOnly
            };
        }

        // Read-only subcommands for pr/issue/run/release/repo.
        if matches!(
            first.as_str(),
            "pr" | "issue" | "run" | "release" | "repo" | "workflow"
        ) && matches!(
            second.as_deref(),
            Some("list")
                | Some("view")
                | Some("status")
                | Some("checks")
                | Some("diff")
                | Some("watch")
        ) {
            return CommandSafety::ReadOnly;
        }

        CommandSafety::Mutating
    }
}

fn gh_api_uses_mutating_method(args: &[String]) -> bool {
    let mut explicit_method: Option<String> = None;
    let mut has_field_params = false;
    let mut iter = args.iter().peekable();

    while let Some(arg) = iter.next() {
        if arg == "-X" || arg == "--method" {
            if let Some(method) = iter.next() {
                explicit_method = Some(method.to_string());
            }
            continue;
        }
        if let Some(method) = arg.strip_prefix("-X") {
            explicit_method = Some(method.strip_prefix('=').unwrap_or(method).to_string());
            continue;
        }
        if let Some(method) = arg.strip_prefix("--method=") {
            explicit_method = Some(method.to_string());
            continue;
        }

        if arg == "-f" || arg == "-F" || arg == "--field" || arg == "--raw-field" {
            has_field_params = true;
            let _ = iter.next();
            continue;
        }
        if (arg.starts_with("-f") && arg.len() > 2)
            || (arg.starts_with("-F") && arg.len() > 2)
            || arg.starts_with("--field=")
            || arg.starts_with("--raw-field=")
        {
            has_field_params = true;
        }
    }

    if explicit_method
        .as_deref()
        .is_some_and(http_method_is_mutating)
    {
        return true;
    }

    has_field_params
        && !explicit_method
            .as_deref()
            .is_some_and(http_method_is_read_only)
}

fn http_method_is_mutating(method: &str) -> bool {
    matches!(
        method.to_ascii_uppercase().as_str(),
        "POST" | "PUT" | "PATCH" | "DELETE"
    )
}

fn http_method_is_read_only(method: &str) -> bool {
    matches!(method.to_ascii_uppercase().as_str(), "GET" | "HEAD")
}

fn executable_name(program: &str) -> &str {
    program
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(program)
}

fn is_shell_interpreter(program: &str) -> bool {
    matches!(
        program,
        "sh" | "bash"
            | "zsh"
            | "fish"
            | "csh"
            | "ksh"
            | "dash"
            | "cmd"
            | "command"
            | "powershell"
            | "pwsh"
    )
}

fn parse_env_wrapped_command(args: &[String]) -> Option<(&str, &[String])> {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if arg == "--" {
            i += 1;
            break;
        }
        if arg == "-u"
            || arg == "--unset"
            || arg == "-S"
            || arg == "--split-string"
            || arg == "-C"
            || arg == "--chdir"
        {
            i += 2;
            continue;
        }
        if arg.starts_with("--unset=") || arg.starts_with("--chdir=") {
            i += 1;
            continue;
        }
        if arg.starts_with('-') {
            i += 1;
            continue;
        }
        if looks_like_env_assignment(arg) {
            i += 1;
            continue;
        }
        break;
    }

    args.get(i)
        .map(|program| (program.as_str(), &args[i + 1..]))
}

fn parse_env_split_args(args: &[String]) -> Option<Vec<String>> {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if arg == "--" {
            return None;
        }
        if arg == "-u" || arg == "--unset" || arg == "-C" || arg == "--chdir" {
            i += 2;
            continue;
        }
        if arg == "-S" || arg == "--split-string" {
            return args
                .get(i + 1)
                .map(|split| split.split_whitespace().map(str::to_string).collect());
        }
        if let Some(split) = arg.strip_prefix("--split-string=") {
            return Some(split.split_whitespace().map(str::to_string).collect());
        }
        if arg.starts_with("--unset=") || arg.starts_with("--chdir=") {
            i += 1;
            continue;
        }
        if arg.starts_with('-') {
            i += 1;
            continue;
        }
        if looks_like_env_assignment(arg) {
            i += 1;
            continue;
        }
        return None;
    }
    None
}

fn looks_like_env_assignment(arg: &str) -> bool {
    let Some((name, _value)) = arg.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name.chars().all(|c| c == '_' || c.is_ascii_alphanumeric())
        && !name.as_bytes()[0].is_ascii_digit()
}

fn contains_shell_metacharacter(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(
            c,
            ';' | '|' | '&' | '$' | '`' | '<' | '>' | '\n' | '\r' | '\0'
        )
    })
}

fn is_path_like(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Reject whitespace-containing values
    if s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    // Reject other clearly non-path-like free-form values (e.g. containing quotes or JSON-like braces/brackets)
    if s.chars().any(|c| matches!(c, '"' | '\'' | '{' | '}' | '[' | ']')) {
        return false;
    }
    true
}

fn is_path_like_extended(value: &str) -> bool {
    if is_path_like(value) {
        return true;
    }

    if value.is_empty() {
        return false;
    }
    if value.starts_with(['"', '\'']) {
        return false;
    }
    if value.chars().any(|c| {
        c.is_control() || matches!(c, ';' | '|' | '&' | '$' | '`' | '<' | '>')
    }) {
        return false;
    }
    if value
        .chars()
        .any(|c| matches!(c, '"' | '\'' | '{' | '}' | '[' | ']'))
    {
        return false;
    }
    if !value.chars().any(char::is_whitespace) {
        return false;
    }

    let has_separator = value.contains('/') || (cfg!(windows) && value.contains('\\'));
    let starts_with_drive = value.len() >= 2
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value.as_bytes()[1] == b':';
    let starts_with_tilde = value.starts_with('~');
    let starts_with_dot_slash = value.starts_with("./") || value.starts_with("../");

    has_separator || starts_with_drive || starts_with_tilde || starts_with_dot_slash
}

fn blocked_argument_safety(args: &[String]) -> Option<CommandSafety> {
    for arg in args {
        if contains_shell_metacharacter(arg) {
            return Some(CommandSafety::Blocked("shell_metacharacter".to_string()));
        }
        if let Some(value) = argument_path_value(arg) {
            if crate::workspace_tools::is_secret_like(Path::new(value)) {
                return Some(CommandSafety::Blocked("secret_like_argument".to_string()));
            }
        }
    }
    None
}

fn argument_path_value(arg: &str) -> Option<&str> {
    let value = if let Some(option) = arg.strip_prefix("--") {
        option.split_once('=').map(|(_, value)| value)
    } else if arg.starts_with('-') {
        arg.split_once('=')
            .map(|(_, value)| value)
            .or_else(|| arg.get(2..))
    } else {
        Some(arg)
    };

    value.filter(|v| is_path_like_extended(v))
}

#[derive(Debug, Clone, Copy, Default)]
struct RmFlags {
    recursive: bool,
    force: bool,
}

/// Parse the flag arguments recognized by `rm` (`--recursive`, `--force`, and
/// the `-r`/`-f` short-flag clusters). Unknown long/short flags are ignored —
/// this parser only tracks the flags that matter for the destructive contract.
/// Arguments following a literal `--` are end-of-options markers/operands and
/// are left for the caller to handle.
fn parse_rm_flags(args: &[String]) -> RmFlags {
    let mut flags = RmFlags::default();
    for arg in args {
        if arg == "--" {
            break;
        }
        if arg == "--recursive" {
            flags.recursive = true;
            continue;
        }
        if arg == "--force" {
            flags.force = true;
            continue;
        }
        if arg.strip_prefix("--").is_some() {
            continue;
        }
        if let Some(rest) = arg.strip_prefix('-') {
            for c in rest.chars() {
                match c {
                    'r' | 'R' => flags.recursive = true,
                    'f' => flags.force = true,
                    _ => {}
                }
            }
        }
    }
    flags
}

fn is_blocked_shell_pattern(program: &str, args: &[String]) -> bool {
    // rm -rf / or rm -rf /*  — and the equivalent long-form invocations
    // (`rm --recursive /`, with or without `--force`). The previous substring
    // matcher flagged `--preserve-root` as recursive (false positive) and
    // missed `rm --recursive /` without `--force` (false negative).
    if program == "rm" {
        let mut operands: Vec<&str> = Vec::new();
        let mut past_dashdash = false;
        for arg in args {
            if past_dashdash {
                operands.push(arg);
                continue;
            }
            if arg == "--" {
                past_dashdash = true;
                continue;
            }
            if arg.starts_with('-') {
                continue;
            }
            operands.push(arg);
        }
        let flags = parse_rm_flags(args);
        let root_target = operands.iter().any(|s| *s == "/" || *s == "/*");
        // `--force` is not required for the destructive contract: `rm --recursive /`
        // alone performs the same irreversible action. AGENTS.md hard-blocks
        // `rm -rf /` / `rm -rf /*`; this reuses the parser to cover the long form.
        if flags.recursive && root_target {
            return true;
        }
    }

    // Block low-level disk/filesystem tools regardless of arguments.
    if matches!(program, "dd" | "mkfs" | "fdisk" | "parted") {
        return true;
    }

    // curl piped to a shell is blocked via shell metacharacters, but also
    // block curl itself from the shell tool to encourage direct program use.
    if program == "curl" {
        return true;
    }

    false
}

fn classify_find_action(args: &[String]) -> Option<CommandSafety> {
    if args.iter().any(|a| a == "-delete") {
        return Some(CommandSafety::Destructive);
    }
    if args
        .iter()
        .any(|a| a == "-exec" || a == "-execdir" || a == "-ok" || a == "-okdir")
    {
        return Some(CommandSafety::Mutating);
    }
    None
}

fn is_read_only_shell_program(program: &str, args: &[String]) -> bool {
    if !matches!(
        program,
        "ls" | "cat"
            | "head"
            | "tail"
            | "find"
            | "grep"
            | "rg"
            | "pwd"
            | "echo"
            | "test"
            | "stat"
            | "file"
            | "which"
            | "wc"
            | "sort"
            | "uniq"
    ) {
        return false;
    }

    if program == "sort" && sort_has_output_destination(args) {
        return false;
    }

    if program == "uniq" && uniq_has_output_destination(args) {
        return false;
    }

    true
}

fn git_remote_is_read_only(args: &[String]) -> bool {
    let mut verb_index = 1;
    if matches!(
        args.get(verb_index).map(String::as_str),
        Some("-v" | "--verbose")
    ) {
        verb_index += 1;
    }

    matches!(
        args.get(verb_index).map(String::as_str),
        None | Some("show")
    )
}

fn sort_has_output_destination(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "-o"
            || arg == "--output"
            || (arg.starts_with("-o") && arg.len() > 2)
            || arg.starts_with("--output=")
    })
}

fn uniq_has_output_destination(args: &[String]) -> bool {
    let mut positional_count = 0;
    let mut options_ended = false;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if !options_ended && arg == "--" {
            options_ended = true;
        } else if !options_ended && uniq_option_takes_value(arg) {
            index += 1;
        } else if !options_ended && arg.starts_with('-') && arg != "-" {
            // Flag or option with an attached value.
        } else {
            positional_count += 1;
            if positional_count > 1 {
                return true;
            }
        }
        index += 1;
    }

    false
}

fn uniq_option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "-f" | "--skip-fields" | "-s" | "--skip-chars" | "-w" | "--check-chars"
    )
}

fn has_path_escape(args: &[String], workspace_root: &Path) -> Option<String> {
    has_path_escape_for_values(
        args.iter()
            .filter_map(|arg| argument_path_value(arg).filter(|value| !value.is_empty())),
        workspace_root,
    )
}

fn has_shell_path_escape(program: &str, args: &[String], workspace_root: &Path) -> Option<String> {
    has_path_escape_for_values(
        args.iter().enumerate().filter_map(|(index, _)| {
            shell_path_argument_value(program, args, index).filter(|value| !value.is_empty())
        }),
        workspace_root,
    )
}

fn has_path_escape_for_values<'a>(
    values: impl IntoIterator<Item = &'a str>,
    workspace_root: &Path,
) -> Option<String> {
    let workspace_canonical = match std::fs::canonicalize(workspace_root) {
        Ok(c) => c,
        Err(e) => return Some(format!("failed to canonicalize workspace root: {}", e)),
    };

    for path_string in values {
        let path = PathBuf::from(path_string);
        if !path.is_absolute() && path_string.contains("..") {
            return Some(format!(
                "relative path may escape workspace: {}",
                path_string
            ));
        }
        let candidate = if path.is_absolute() {
            path
        } else {
            workspace_canonical.join(&path)
        };
        if candidate_escapes_workspace(&candidate, &workspace_canonical) {
            return Some(format!("path escapes workspace: {}", path_string));
        }
    }
    None
}

fn shell_path_argument_value<'a>(
    program: &str,
    args: &'a [String],
    index: usize,
) -> Option<&'a str> {
    let arg = args[index].as_str();
    if !arg.chars().any(char::is_whitespace) {
        return argument_path_value(arg);
    }

    if let Some(value) = shell_path_option_value(program, args, index) {
        return Some(value);
    }
    if is_shell_file_operand(program, args, index) {
        return Some(arg);
    }

    None
}

fn shell_path_option_value<'a>(program: &str, args: &'a [String], index: usize) -> Option<&'a str> {
    let arg = args[index].as_str();
    let previous = index.checked_sub(1).and_then(|previous| args.get(previous));

    match program {
        "file" => arg
            .strip_prefix("--files-from=")
            .or_else(|| arg.strip_prefix("--magic-file="))
            .or_else(|| {
                matches!(
                    previous.map(String::as_str),
                    Some("-f" | "--files-from" | "-m" | "--magic-file")
                )
                .then_some(arg)
            }),
        "grep" => arg.strip_prefix("--file=").or_else(|| {
            matches!(previous.map(String::as_str), Some("-f" | "--file")).then_some(arg)
        }),
        "rg" => arg
            .strip_prefix("--pre=")
            .or_else(|| matches!(previous.map(String::as_str), Some("--pre")).then_some(arg)),
        _ => None,
    }
}

fn is_shell_file_operand(program: &str, args: &[String], index: usize) -> bool {
    if !matches!(
        program,
        "cat" | "head" | "tail" | "ls" | "find" | "grep" | "rg" | "stat" | "file" | "wc"
    ) {
        return false;
    }

    let mut options_ended = false;
    let mut positional_count = 0;
    for (current_index, arg) in args.iter().enumerate() {
        if !options_ended && arg == "--" {
            options_ended = true;
            continue;
        }
        if !options_ended && arg.starts_with('-') && arg != "-" {
            continue;
        }

        let is_file_operand = match program {
            // The first positional grep/rg argument is the pattern; every
            // following positional argument is a candidate file path.
            "grep" | "rg" => positional_count > 0,
            _ => true,
        };
        if current_index == index {
            return is_file_operand;
        }
        positional_count += 1;
    }

    false
}

fn candidate_escapes_workspace(candidate: &Path, workspace_canonical: &Path) -> bool {
    if let Ok(canonical) = std::fs::canonicalize(candidate) {
        return !canonical.starts_with(workspace_canonical);
    }

    if candidate.is_absolute()
        && !normalize_path_lexically(candidate).starts_with(workspace_canonical)
    {
        return true;
    }

    let mut existing_ancestor = candidate;
    while !path_exists_or_is_symlink(existing_ancestor) {
        let Some(parent) = existing_ancestor.parent() else {
            return false;
        };
        if parent == existing_ancestor {
            return false;
        }
        existing_ancestor = parent;
    }

    std::fs::canonicalize(existing_ancestor)
        .is_ok_and(|canonical| !canonical.starts_with(workspace_canonical))
}

fn path_exists_or_is_symlink(path: &Path) -> bool {
    path.exists() || std::fs::symlink_metadata(path).is_ok()
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Component::RootDir.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(OsStr::new(".."));
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRisk {
    Mutating,
    Destructive,
}

#[derive(Debug, Clone)]
pub struct CommandApprovalRequest {
    pub tool_name: &'static str,
    pub command_label: String,
    pub reason: String,
    pub risk: CommandRisk,
}

pub(crate) type CommandApprovalFuture<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

pub trait CommandApproval: Send + Sync {
    fn request_approval<'a>(&'a self, request: CommandApprovalRequest)
        -> CommandApprovalFuture<'a>;
}

#[derive(Debug, Serialize)]
pub struct CommandOutput {
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub needs_approval: Option<bool>,
    pub reason: Option<String>,
    pub message: String,
}

pub struct CommandExecutor {
    workspace_root: PathBuf,
    policy: CommandPolicy,
    approval: Option<Arc<dyn CommandApproval>>,
}

impl CommandExecutor {
    pub fn new(workspace_root: PathBuf, approval: Option<Arc<dyn CommandApproval>>) -> Self {
        Self {
            workspace_root,
            policy: CommandPolicy,
            approval,
        }
    }

    pub async fn run_shell(
        &self,
        program: String,
        args: Vec<String>,
        timeout_seconds: Option<u64>,
    ) -> Result<CommandOutput, ShellToolError> {
        let classify_program = program.clone();
        let classify_args = args.clone();
        let safety = self
            .classify_blocking(move |policy, workspace_root| {
                policy.classify_shell(&classify_program, &classify_args, &workspace_root)
            })
            .await?;
        let timeout = timeout_seconds
            .map(Duration::from_secs)
            .unwrap_or(COMMAND_TIMEOUT);
        self.run_with_approval(RunShellCommandTool::NAME, &program, &args, safety, timeout)
            .await
    }

    pub async fn run_git(
        &self,
        args: Vec<String>,
        timeout_seconds: Option<u64>,
    ) -> Result<CommandOutput, ShellToolError> {
        let classify_args = args.clone();
        let safety = self
            .classify_blocking(move |policy, workspace_root| {
                policy.classify_git(&classify_args, &workspace_root)
            })
            .await?;
        let timeout = timeout_seconds
            .map(Duration::from_secs)
            .unwrap_or(COMMAND_TIMEOUT);
        self.run_with_approval(RunGitCommandTool::NAME, "git", &args, safety, timeout)
            .await
    }

    pub async fn run_gh(
        &self,
        args: Vec<String>,
        timeout_seconds: Option<u64>,
    ) -> Result<CommandOutput, ShellToolError> {
        let classify_args = args.clone();
        let safety = self
            .classify_blocking(move |policy, workspace_root| {
                policy.classify_gh(&classify_args, &workspace_root)
            })
            .await?;
        let timeout = timeout_seconds
            .map(Duration::from_secs)
            .unwrap_or(COMMAND_TIMEOUT);
        self.run_with_approval(RunGithubCliCommandTool::NAME, "gh", &args, safety, timeout)
            .await
    }

    async fn classify_blocking<F>(&self, classify: F) -> Result<CommandSafety, ShellToolError>
    where
        F: FnOnce(CommandPolicy, PathBuf) -> CommandSafety + Send + 'static,
    {
        let policy = self.policy.clone();
        let workspace_root = self.workspace_root.clone();
        tokio::task::spawn_blocking(move || classify(policy, workspace_root))
            .await
            .map_err(|e| ShellToolError::Execution(format!("classification failed: {}", e)))
    }

    async fn run_with_approval(
        &self,
        tool_name: &'static str,
        program: &str,
        args: &[String],
        safety: CommandSafety,
        timeout: Duration,
    ) -> Result<CommandOutput, ShellToolError> {
        match safety {
            CommandSafety::ReadOnly => self.execute(program, args, timeout).await,
            CommandSafety::Blocked(reason) => Ok(CommandOutput {
                success: false,
                exit_code: -1,
                stdout: String::new(),
                stderr: String::new(),
                truncated: false,
                needs_approval: Some(false),
                reason: Some(reason.clone()),
                message: format!("Blocked: {}", reason),
            }),
            CommandSafety::Mutating | CommandSafety::Destructive => {
                let label = command_label(program, args);
                let approved = self.request_approval(tool_name, &label, &safety).await;
                if !approved {
                    Ok(CommandOutput {
                        success: false,
                        exit_code: -1,
                        stdout: String::new(),
                        stderr: String::new(),
                        truncated: false,
                        needs_approval: Some(true),
                        reason: Some("approval_denied".to_string()),
                        message: format!(
                            "Approval denied for `{}`. The user did not authorize this mutating command.",
                            label
                        ),
                    })
                } else {
                    self.execute(program, args, timeout).await
                }
            }
        }
    }

    async fn execute(
        &self,
        program: &str,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, ShellToolError> {
        if !self.workspace_root.exists() {
            return Err(ShellToolError::WorkspaceUnavailable(format!(
                "workspace does not exist: {}",
                self.workspace_root.display()
            )));
        }

        let mut command = tokio::process::Command::new(program);
        command
            .args(args)
            .current_dir(&self.workspace_root);

        let label = command_label(program, args);

        let output = match crate::subprocess_output::run_with_bounded_output(
            &label,
            command,
            timeout,
            COMMAND_OUTPUT_CAP,
            COMMAND_OUTPUT_CAP,
        )
        .await
        {
            Ok(output) => output,
            Err(crate::subprocess_output::SubprocessError::Spawn { source, .. }) => {
                return Err(ShellToolError::Execution(format!(
                    "failed to spawn `{}`: {}",
                    label, source
                )));
            }
            Err(crate::subprocess_output::SubprocessError::Wait { source, .. }) => {
                return Err(ShellToolError::Execution(format!(
                    "failed to run `{}`: {}",
                    label, source
                )));
            }
        };

        if output.timed_out {
            return Ok(CommandOutput {
                success: false,
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("Command `{}` timed out after {:?}", label, timeout),
                truncated: false,
                needs_approval: None,
                reason: Some("timeout".to_string()),
                message: format!("Command `{}` timed out after {:?}", label, timeout),
            });
        }

        let stdout_cap = if output.stdout_truncated {
            COMMAND_OUTPUT_CAP.saturating_sub(1)
        } else {
            COMMAND_OUTPUT_CAP
        };
        let stderr_cap = if output.stderr_truncated {
            COMMAND_OUTPUT_CAP.saturating_sub(1)
        } else {
            COMMAND_OUTPUT_CAP
        };
        let (stdout, stdout_truncated) = truncate_bytes_to_string(&output.stdout, stdout_cap);
        let (stderr, stderr_truncated) = truncate_bytes_to_string(&output.stderr, stderr_cap);
        let truncated = stdout_truncated
            || stderr_truncated
            || output.stdout_truncated
            || output.stderr_truncated;
        let exit_code = output.status.code().unwrap_or(-1);
        let success = output.status.success();

        let message = if success {
            format!("Command `{}` completed with exit code {}", label, exit_code)
        } else {
            let detail = if stderr.is_empty() {
                stdout.trim().to_string()
            } else {
                stderr.trim().to_string()
            };
            if detail.is_empty() {
                format!("Command `{}` failed with exit code {}", label, exit_code)
            } else {
                format!(
                    "Command `{}` failed with exit code {}: {}",
                    label, exit_code, detail
                )
            }
        };

        Ok(CommandOutput {
            success,
            exit_code,
            stdout,
            stderr,
            truncated,
            needs_approval: None,
            reason: if success {
                None
            } else {
                Some("non_zero_exit".to_string())
            },
            message,
        })
    }

    async fn request_approval(
        &self,
        tool_name: &'static str,
        command_label: &str,
        safety: &CommandSafety,
    ) -> bool {
        let Some(approval) = &self.approval else {
            return false;
        };

        let (reason, risk) = match safety {
            CommandSafety::Destructive => (
                "This command may destroy or overwrite data.".to_string(),
                CommandRisk::Destructive,
            ),
            CommandSafety::Mutating => (
                "This command may modify the workspace.".to_string(),
                CommandRisk::Mutating,
            ),
            _ => return false,
        };

        approval
            .request_approval(CommandApprovalRequest {
                tool_name,
                command_label: command_label.to_string(),
                reason,
                risk,
            })
            .await
    }
}

fn command_label(program: &str, args: &[String]) -> String {
    if args.is_empty() {
        program.to_string()
    } else {
        format!("{} {}", program, args.join(" "))
    }
}

fn truncate_bytes_to_string(bytes: &[u8], max: usize) -> (String, bool) {
    let (text, truncated) = truncate_text_bytes(&String::from_utf8_lossy(bytes), max);
    (text, truncated)
}

#[derive(Debug, Deserialize)]
pub struct RunShellCommandArgs {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct RunShellCommandTool {
    workspace_root: PathBuf,
    #[serde(skip)]
    approval: Option<Arc<dyn CommandApproval>>,
}

impl RunShellCommandTool {
    pub fn new(workspace_root: PathBuf, approval: Option<Arc<dyn CommandApproval>>) -> Self {
        Self {
            workspace_root,
            approval,
        }
    }
}

impl std::fmt::Debug for RunShellCommandTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunShellCommandTool")
            .field("workspace_root", &self.workspace_root)
            .field("approval", &self.approval.as_ref().map(|_| "<configured>"))
            .finish()
    }
}

impl Tool for RunShellCommandTool {
    const NAME: &'static str = "run_shell_command";

    type Error = ShellToolError;
    type Args = RunShellCommandArgs;
    type Output = CommandOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Run a non-shell program with arguments in the active workspace. Read-only commands run directly; mutating or destructive commands require one-time user approval. Shell interpreters and shell metacharacters are blocked.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "program": {
                        "type": "string",
                        "description": "Program name or path to execute directly (not a shell interpreter)."
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Command arguments."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Optional timeout in seconds (default 120)."
                    }
                },
                "required": ["program"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let executor = CommandExecutor::new(self.workspace_root.clone(), self.approval.clone());
        executor
            .run_shell(args.program, args.args, args.timeout_seconds)
            .await
    }
}

#[derive(Debug, Deserialize)]
pub struct RunGitCommandArgs {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct RunGitCommandTool {
    workspace_root: PathBuf,
    #[serde(skip)]
    approval: Option<Arc<dyn CommandApproval>>,
}

impl RunGitCommandTool {
    pub fn new(workspace_root: PathBuf, approval: Option<Arc<dyn CommandApproval>>) -> Self {
        Self {
            workspace_root,
            approval,
        }
    }
}

impl std::fmt::Debug for RunGitCommandTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunGitCommandTool")
            .field("workspace_root", &self.workspace_root)
            .field("approval", &self.approval.as_ref().map(|_| "<configured>"))
            .finish()
    }
}

impl Tool for RunGitCommandTool {
    const NAME: &'static str = "run_git_command";

    type Error = ShellToolError;
    type Args = RunGitCommandArgs;
    type Output = CommandOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Run a git command in the active workspace. Read-only commands run directly; mutating commands require one-time user approval. Destructive commands (push --force, reset --hard, clean) are blocked.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Git command arguments (without the leading 'git')."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Optional timeout in seconds (default 120)."
                    }
                },
                "required": ["args"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let executor = CommandExecutor::new(self.workspace_root.clone(), self.approval.clone());
        executor.run_git(args.args, args.timeout_seconds).await
    }
}

#[derive(Debug, Deserialize)]
pub struct RunGithubCliCommandArgs {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct RunGithubCliCommandTool {
    workspace_root: PathBuf,
    #[serde(skip)]
    approval: Option<Arc<dyn CommandApproval>>,
}

impl RunGithubCliCommandTool {
    pub fn new(workspace_root: PathBuf, approval: Option<Arc<dyn CommandApproval>>) -> Self {
        Self {
            workspace_root,
            approval,
        }
    }
}

impl std::fmt::Debug for RunGithubCliCommandTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunGithubCliCommandTool")
            .field("workspace_root", &self.workspace_root)
            .field("approval", &self.approval.as_ref().map(|_| "<configured>"))
            .finish()
    }
}

impl Tool for RunGithubCliCommandTool {
    const NAME: &'static str = "run_github_cli_command";

    type Error = ShellToolError;
    type Args = RunGithubCliCommandArgs;
    type Output = CommandOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Run a GitHub CLI (gh) command in the active workspace. Read-only commands run directly; mutating commands require one-time user approval. Destructive commands (repo delete) are blocked.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "gh command arguments (without the leading 'gh')."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Optional timeout in seconds (default 120)."
                    }
                },
                "required": ["args"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let executor = CommandExecutor::new(self.workspace_root.clone(), self.approval.clone());
        executor.run_gh(args.args, args.timeout_seconds).await
    }
}

pub fn create_run_shell_command_tool(
    workspace_root: PathBuf,
    approval: Option<Arc<dyn CommandApproval>>,
) -> RunShellCommandTool {
    RunShellCommandTool::new(workspace_root, approval)
}

pub fn create_run_git_command_tool(
    workspace_root: PathBuf,
    approval: Option<Arc<dyn CommandApproval>>,
) -> RunGitCommandTool {
    RunGitCommandTool::new(workspace_root, approval)
}

pub fn create_run_github_cli_command_tool(
    workspace_root: PathBuf,
    approval: Option<Arc<dyn CommandApproval>>,
) -> RunGithubCliCommandTool {
    RunGithubCliCommandTool::new(workspace_root, approval)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn workspace() -> PathBuf {
        std::env::current_dir().expect("current dir")
    }

    #[test]
    fn classify_shell_blocks_shell_interpreters() {
        let policy = CommandPolicy;
        for program in ["sh", "bash", "zsh", "fish", "powershell", "pwsh"] {
            let safety = policy.classify_shell(program, &[], &workspace());
            assert!(
                matches!(safety, CommandSafety::Blocked(_)),
                "{} should be blocked",
                program
            );
        }
        for program in ["/bin/bash", "/usr/bin/zsh"] {
            assert!(
                matches!(
                    policy.classify_shell(program, &[], &workspace()),
                    CommandSafety::Blocked(_)
                ),
                "{} should be blocked",
                program
            );
        }
        assert!(matches!(
            policy.classify_shell("env", &["bash".to_string()], &workspace()),
            CommandSafety::Blocked(_)
        ));
        assert!(matches!(
            policy.classify_shell(
                "env",
                &["PATH=/bin".to_string(), "/usr/bin/zsh".to_string()],
                &workspace()
            ),
            CommandSafety::Blocked(_)
        ));
        assert!(matches!(
            policy.classify_shell(
                "env",
                &["-S".to_string(), "bash -c echo hi".to_string()],
                &workspace()
            ),
            CommandSafety::Blocked(_)
        ));
        assert!(matches!(
            policy.classify_shell(
                "env",
                &["env".to_string(), "bash".to_string()],
                &workspace()
            ),
            CommandSafety::Blocked(_)
        ));
    }

    #[test]
    fn classify_shell_blocks_metacharacters() {
        let policy = CommandPolicy;
        let safety = policy.classify_shell("ls", &["-la;".to_string()], &workspace());
        assert!(matches!(safety, CommandSafety::Blocked(_)));
    }

    #[test]
    fn classifiers_block_secret_like_argument_paths() {
        let policy = CommandPolicy;
        let blocked = CommandSafety::Blocked("secret_like_argument".to_string());

        assert_eq!(
            policy.classify_shell("cat", &[".env".to_string()], &workspace()),
            blocked
        );
        assert_eq!(
            policy.classify_shell("cat", &["--input=.env".to_string()], &workspace()),
            blocked
        );
        assert_eq!(
            policy.classify_git(
                &["status".to_string(), "--config=.npmrc".to_string()],
                &workspace()
            ),
            blocked
        );
        assert_eq!(
            policy.classify_gh(
                &["status".to_string(), "--input=.env".to_string()],
                &workspace()
            ),
            blocked
        );

        // Allow free-form/whitespace-containing args that might contain secret keywords
        assert_ne!(
            policy.classify_git(
                &["commit".to_string(), "-m".to_string(), "fixed credentials bug".to_string()],
                &workspace()
            ),
            blocked
        );
        assert_ne!(
            policy.classify_shell(
                "echo",
                &["this is my secret message".to_string()],
                &workspace()
            ),
            blocked
        );
    }

    #[test]
    fn classify_shell_blocks_rm_rf_root() {
        let policy = CommandPolicy;
        let safety =
            policy.classify_shell("rm", &["-rf".to_string(), "/".to_string()], &workspace());
        assert!(matches!(safety, CommandSafety::Blocked(_)));

        let safety =
            policy.classify_shell("rm", &["-rf".to_string(), "/*".to_string()], &workspace());
        assert!(matches!(safety, CommandSafety::Blocked(_)));

        let safety = policy.classify_shell(
            "/bin/rm",
            &["-rf".to_string(), "/".to_string()],
            &workspace(),
        );
        assert!(matches!(safety, CommandSafety::Blocked(_)));
    }

    #[test]
    fn rm_rf_root_blocked() {
        assert!(is_blocked_shell_pattern("rm", &["-rf".to_string(), "/".to_string()]));
    }

    #[test]
    fn rm_fr_root_blocked() {
        assert!(is_blocked_shell_pattern("rm", &["-fr".to_string(), "/".to_string()]));
    }

    #[test]
    fn rm_long_recursive_force_root_blocked() {
        assert!(is_blocked_shell_pattern(
            "rm",
            &["--recursive".to_string(), "--force".to_string(), "/".to_string()],
        ));
    }

    #[test]
    fn rm_long_recursive_root_blocked() {
        assert!(is_blocked_shell_pattern(
            "rm",
            &["--recursive".to_string(), "/".to_string()],
        ));
    }

    #[test]
    fn rm_long_recursive_no_root_not_blocked_by_pattern() {
        assert!(!is_blocked_shell_pattern(
            "rm",
            &["--recursive".to_string(), "subdir/".to_string()],
        ));
    }

    #[test]
    fn rm_preserve_root_long_flag_does_not_set_recursive() {
        assert!(!is_blocked_shell_pattern(
            "rm",
            &["--preserve-root".to_string(), "/".to_string()],
        ));
    }

    #[test]
    fn rm_dashdash_does_not_treat_operands_as_flags() {
        assert!(!is_blocked_shell_pattern(
            "rm",
            &["--".to_string(), "--recursive".to_string(), "/".to_string()],
        ));
    }

    #[test]
    fn rm_glob_root_target_blocked() {
        assert!(is_blocked_shell_pattern(
            "rm",
            &["-rf".to_string(), "/*".to_string()],
        ));
    }

    #[test]
    fn rm_glob_subdir_target_not_blocked() {
        assert!(!is_blocked_shell_pattern(
            "rm",
            &["-rf".to_string(), "/tmp/*".to_string()],
        ));
    }

    #[test]
    fn classify_shell_blocks_dangerous_programs() {
        let policy = CommandPolicy;
        assert!(matches!(
            policy.classify_shell("dd", &[], &workspace()),
            CommandSafety::Blocked(_)
        ));
        assert!(matches!(
            policy.classify_shell("curl", &["https://example.com".to_string()], &workspace()),
            CommandSafety::Blocked(_)
        ));
    }

    #[test]
    fn classify_shell_allows_read_only_programs() {
        let policy = CommandPolicy;
        assert_eq!(
            policy.classify_shell("ls", &["-la".to_string()], &workspace()),
            CommandSafety::ReadOnly
        );
        assert_eq!(
            policy.classify_shell("cat", &["file.txt".to_string()], &workspace()),
            CommandSafety::ReadOnly
        );
        assert_eq!(
            policy.classify_shell("git", &["status".to_string()], &workspace()),
            CommandSafety::ReadOnly
        );
    }

    #[test]
    fn classify_shell_detects_sort_and_uniq_output_destinations() {
        let policy = CommandPolicy;

        for args in [
            vec![
                "-o".to_string(),
                "sorted.txt".to_string(),
                "input.txt".to_string(),
            ],
            vec!["-osorted.txt".to_string(), "input.txt".to_string()],
            vec![
                "--output".to_string(),
                "sorted.txt".to_string(),
                "input.txt".to_string(),
            ],
            vec!["--output=sorted.txt".to_string(), "input.txt".to_string()],
        ] {
            assert_eq!(
                policy.classify_shell("sort", &args, &workspace()),
                CommandSafety::Mutating
            );
        }

        assert_eq!(
            policy.classify_shell(
                "uniq",
                &["-".to_string(), "unique.txt".to_string()],
                &workspace()
            ),
            CommandSafety::Mutating
        );
        assert_eq!(
            policy.classify_shell(
                "uniq",
                &[
                    "--skip-fields".to_string(),
                    "1".to_string(),
                    "input.txt".to_string(),
                ],
                &workspace()
            ),
            CommandSafety::ReadOnly
        );
    }

    #[test]
    fn classify_shell_requires_approval_for_find_actions() {
        let policy = CommandPolicy;
        assert_eq!(
            policy.classify_shell(
                "find",
                &[".".to_string(), "-delete".to_string()],
                &workspace()
            ),
            CommandSafety::Destructive
        );
        assert_eq!(
            policy.classify_shell(
                "find",
                &[
                    ".".to_string(),
                    "-exec".to_string(),
                    "rm".to_string(),
                    "{}".to_string(),
                    "+".to_string()
                ],
                &workspace()
            ),
            CommandSafety::Mutating
        );
    }

    #[test]
    fn classify_shell_preserves_git_and_gh_hard_blocks() {
        let policy = CommandPolicy;
        assert!(matches!(
            policy.classify_shell(
                "git",
                &["push".to_string(), "--force".to_string()],
                &workspace()
            ),
            CommandSafety::Blocked(_)
        ));
        assert!(matches!(
            policy.classify_shell(
                "/usr/bin/git",
                &["reset".to_string(), "--hard".to_string()],
                &workspace()
            ),
            CommandSafety::Blocked(_)
        ));
        assert!(matches!(
            policy.classify_shell(
                "gh",
                &["repo".to_string(), "delete".to_string()],
                &workspace()
            ),
            CommandSafety::Blocked(_)
        ));
        assert!(matches!(
            policy.classify_shell(
                "env",
                &["gh".to_string(), "repo".to_string(), "delete".to_string()],
                &workspace()
            ),
            CommandSafety::Blocked(_)
        ));
        assert!(matches!(
            policy.classify_shell(
                "env",
                &["--split-string=gh repo delete".to_string()],
                &workspace()
            ),
            CommandSafety::Blocked(_)
        ));
    }

    #[test]
    fn classify_shell_treats_unknown_as_mutating() {
        let policy = CommandPolicy;
        assert_eq!(
            policy.classify_shell("mkdir", &["new-dir".to_string()], &workspace()),
            CommandSafety::Mutating
        );
    }

    #[test]
    fn classify_shell_requires_approval_for_external_path() {
        let policy = CommandPolicy;
        let safety = policy.classify_shell("cat", &["/etc/passwd".to_string()], &workspace());
        assert_eq!(safety, CommandSafety::Mutating);

        let safety = policy.classify_shell("cat", &["../outside.txt".to_string()], &workspace());
        assert_eq!(safety, CommandSafety::Mutating);
    }

    #[test]
    fn classify_shell_requires_approval_for_external_spaced_path() {
        let outside = tempfile::tempdir().expect("outside");
        let spaced_dir = outside.path().join("some spaced dir");
        std::fs::create_dir(&spaced_dir).expect("spaced dir");
        let spaced_path = spaced_dir.join("passwd");
        std::fs::write(&spaced_path, "secret").expect("spaced file");

        let policy = CommandPolicy;
        let safety = policy.classify_shell(
            "cat",
            &[spaced_path.to_string_lossy().into_owned()],
            &workspace(),
        );
        assert_eq!(safety, CommandSafety::Mutating);
    }

    #[test]
    fn classify_shell_permits_safe_in_workspace_spaced_path() {
        let workspace = tempfile::tempdir().expect("workspace");
        let spaced_path = workspace.path().join("some file.txt");
        std::fs::write(&spaced_path, "safe").expect("workspace file");

        let policy = CommandPolicy;
        let safety = policy.classify_shell(
            "cat",
            &[spaced_path.to_string_lossy().into_owned()],
            workspace.path(),
        );
        assert_eq!(safety, CommandSafety::ReadOnly);
    }

    #[test]
    fn classify_shell_permits_relative_spaced_filename() {
        let workspace = tempfile::tempdir().expect("workspace");
        let files_dir = workspace.path().join("some files");
        std::fs::create_dir(&files_dir).expect("workspace files directory");
        std::fs::write(files_dir.join("notes.txt"), "safe").expect("workspace file");

        let policy = CommandPolicy;
        let safety = policy.classify_shell(
            "cat",
            &["some files/notes.txt".to_string()],
            workspace.path(),
        );
        assert_eq!(safety, CommandSafety::ReadOnly);
    }

    #[test]
    fn classify_shell_requires_approval_for_flag_with_spaced_value() {
        let outside = tempfile::tempdir().expect("outside");
        let spaced_dir = outside.path().join("some spaced dir");
        std::fs::create_dir(&spaced_dir).expect("spaced dir");
        let spaced_path = spaced_dir.join("x");
        std::fs::write(&spaced_path, "secret").expect("spaced file");

        let policy = CommandPolicy;
        let safety = policy.classify_shell(
            "file",
            &[format!("--files-from={}", spaced_path.to_string_lossy())],
            &workspace(),
        );
        assert_eq!(safety, CommandSafety::Mutating);
    }

    #[test]
    fn classify_shell_requires_approval_for_flag_style_path_escape() {
        let policy = CommandPolicy;

        let safety = policy.classify_shell(
            "cat",
            &["--output=../outside.txt".to_string()],
            &workspace(),
        );
        assert_eq!(safety, CommandSafety::Mutating);

        let safety =
            policy.classify_shell("cat", &["--file=/etc/passwd".to_string()], &workspace());
        assert_eq!(safety, CommandSafety::Mutating);

        let safety = policy.classify_shell("cat", &["-o../outside.txt".to_string()], &workspace());
        assert_eq!(safety, CommandSafety::Mutating);

        let safety = policy.classify_shell("cat", &["--verbose".to_string()], &workspace());
        assert_eq!(safety, CommandSafety::ReadOnly);

        let safety = policy.classify_shell("cat", &["-v".to_string()], &workspace());
        assert_eq!(safety, CommandSafety::ReadOnly);

        let safety = policy.classify_shell("cat", &["--output=file.txt".to_string()], &workspace());
        assert_eq!(safety, CommandSafety::ReadOnly);
    }

    #[cfg(unix)]
    #[test]
    fn classify_shell_requires_approval_for_symlink_path_escape() {
        let workspace = tempfile::tempdir().expect("workspace");
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("secret.txt"), "secret").expect("outside file");
        std::os::unix::fs::symlink(outside.path(), workspace.path().join("outside-link"))
            .expect("symlink");

        let policy = CommandPolicy;
        assert_eq!(
            policy.classify_shell(
                "cat",
                &["outside-link/secret.txt".to_string()],
                workspace.path()
            ),
            CommandSafety::Mutating
        );
        assert_eq!(
            policy.classify_shell(
                "cat",
                &["outside-link/new-file.txt".to_string()],
                workspace.path()
            ),
            CommandSafety::Mutating
        );
    }

    #[cfg(unix)]
    #[test]
    fn classify_shell_requires_approval_for_relative_spaced_symlink_escape() {
        let workspace = tempfile::tempdir().expect("workspace");
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("secret file.txt"), "secret").expect("outside file");
        std::os::unix::fs::symlink(outside.path(), workspace.path().join("outside link"))
            .expect("symlink");

        let policy = CommandPolicy;
        assert_eq!(
            policy.classify_shell(
                "cat",
                &["outside link/secret file.txt".to_string()],
                workspace.path()
            ),
            CommandSafety::Mutating
        );
    }

    #[test]
    fn classify_git_blocks_destructive_options() {
        let policy = CommandPolicy;
        assert!(matches!(
            policy.classify_git(&["push".to_string(), "--force".to_string()], &workspace()),
            CommandSafety::Blocked(_)
        ));
        assert!(matches!(
            policy.classify_git(&["push".to_string(), "-f".to_string()], &workspace()),
            CommandSafety::Blocked(_)
        ));
        assert!(matches!(
            policy.classify_git(&["reset".to_string(), "--hard".to_string()], &workspace()),
            CommandSafety::Blocked(_)
        ));
        assert!(matches!(
            policy.classify_git(&["clean".to_string(), "-fd".to_string()], &workspace()),
            CommandSafety::Blocked(_)
        ));
    }

    #[test]
    fn classify_git_allows_read_only_subcommands() {
        let policy = CommandPolicy;
        assert_eq!(
            policy.classify_git(&["status".to_string()], &workspace()),
            CommandSafety::ReadOnly
        );
        assert_eq!(
            policy.classify_git(&["log".to_string(), "--oneline".to_string()], &workspace()),
            CommandSafety::ReadOnly
        );
    }

    #[test]
    fn classify_git_remote_read_only_forms() {
        let policy = CommandPolicy;
        // `git remote`, `git remote -v`, `git remote --verbose`, and
        // `git remote show <name>` are read-only.
        assert_eq!(
            policy.classify_git(&["remote".to_string()], &workspace()),
            CommandSafety::ReadOnly
        );
        assert_eq!(
            policy.classify_git(
                &["remote".to_string(), "-v".to_string()],
                &workspace()
            ),
            CommandSafety::ReadOnly
        );
        assert_eq!(
            policy.classify_git(
                &["remote".to_string(), "--verbose".to_string()],
                &workspace()
            ),
            CommandSafety::ReadOnly
        );
        assert_eq!(
            policy.classify_git(
                &[
                    "remote".to_string(),
                    "show".to_string(),
                    "origin".to_string()
                ],
                &workspace()
            ),
            CommandSafety::ReadOnly
        );
        assert_eq!(
            policy.classify_git(
                &[
                    "remote".to_string(),
                    "-v".to_string(),
                    "show".to_string(),
                    "origin".to_string(),
                ],
                &workspace()
            ),
            CommandSafety::ReadOnly
        );
    }

    #[test]
    fn classify_git_remote_add_show_url_is_mutating() {
        // Regression: `git remote add show <url>` must NOT be classified as
        // read-only just because `show` appears in the arg list.
        let policy = CommandPolicy;
        assert_eq!(
            policy.classify_git(
                &[
                    "remote".to_string(),
                    "add".to_string(),
                    "show".to_string(),
                    "https://example.com".to_string()
                ],
                &workspace()
            ),
            CommandSafety::Mutating
        );
        assert_eq!(
            policy.classify_git(
                &[
                    "remote".to_string(),
                    "set-url".to_string(),
                    "origin".to_string(),
                    "https://example.com".to_string()
                ],
                &workspace()
            ),
            CommandSafety::Mutating
        );
    }

    #[test]
    fn classify_git_remote_verbose_mutations_are_mutating() {
        let policy = CommandPolicy;
        for args in [
            vec![
                "remote".to_string(),
                "-v".to_string(),
                "add".to_string(),
                "show".to_string(),
                "https://example.com".to_string(),
            ],
            vec![
                "remote".to_string(),
                "--verbose".to_string(),
                "update".to_string(),
            ],
        ] {
            assert_eq!(
                policy.classify_git(&args, &workspace()),
                CommandSafety::Mutating
            );
        }
    }

    #[test]
    fn classify_git_treats_mutating_subcommands_as_mutating() {
        let policy = CommandPolicy;
        assert_eq!(
            policy.classify_git(&["add".to_string(), ".".to_string()], &workspace()),
            CommandSafety::Mutating
        );
        assert_eq!(
            policy.classify_git(
                &["commit".to_string(), "-m".to_string(), "x".to_string()],
                &workspace()
            ),
            CommandSafety::Mutating
        );
    }

    #[test]
    fn classify_git_treats_stash_writes_as_mutating() {
        let policy = CommandPolicy;
        for args in [
            vec!["stash".to_string()],
            vec!["stash".to_string(), "push".to_string()],
            vec!["stash".to_string(), "save".to_string()],
            vec![
                "stash".to_string(),
                "branch".to_string(),
                "saved".to_string(),
            ],
        ] {
            assert_eq!(
                policy.classify_git(&args, &workspace()),
                CommandSafety::Mutating
            );
        }
        for args in [
            vec!["stash".to_string(), "list".to_string()],
            vec!["stash".to_string(), "show".to_string()],
            vec!["stash".to_string(), "-h".to_string()],
            vec!["stash".to_string(), "--help".to_string()],
        ] {
            assert_eq!(
                policy.classify_git(&args, &workspace()),
                CommandSafety::ReadOnly
            );
        }
    }

    #[test]
    fn classify_git_allows_tag_listings() {
        let policy = CommandPolicy;
        for args in [
            vec!["tag".to_string()],
            vec!["tag".to_string(), "-l".to_string()],
            vec!["tag".to_string(), "--list".to_string()],
            vec!["tag".to_string(), "-l".to_string(), "v*".to_string()],
        ] {
            assert_eq!(
                policy.classify_git(&args, &workspace()),
                CommandSafety::ReadOnly
            );
        }
        assert_eq!(
            policy.classify_git(&["tag".to_string(), "v1.0.0".to_string()], &workspace()),
            CommandSafety::Mutating
        );
        assert_eq!(
            policy.classify_git(
                &["tag".to_string(), "-d".to_string(), "v1.0.0".to_string()],
                &workspace()
            ),
            CommandSafety::Destructive
        );
    }

    #[test]
    fn classify_git_treats_branch_delete_as_destructive() {
        let policy = CommandPolicy;
        assert_eq!(
            policy.classify_git(
                &[
                    "branch".to_string(),
                    "-d".to_string(),
                    "feature".to_string()
                ],
                &workspace()
            ),
            CommandSafety::Destructive
        );
    }

    #[test]
    fn classify_git_treats_checkout_force_as_destructive() {
        let policy = CommandPolicy;
        assert_eq!(
            policy.classify_git(
                &["checkout".to_string(), "-f".to_string(), "main".to_string()],
                &workspace()
            ),
            CommandSafety::Destructive
        );
        assert_eq!(
            policy.classify_git(
                &[
                    "checkout".to_string(),
                    "--force".to_string(),
                    "main".to_string()
                ],
                &workspace()
            ),
            CommandSafety::Destructive
        );
    }

    #[test]
    fn classify_gh_blocks_repo_delete() {
        let policy = CommandPolicy;
        assert!(matches!(
            policy.classify_gh(&["repo".to_string(), "delete".to_string()], &workspace()),
            CommandSafety::Blocked(_)
        ));
    }

    #[test]
    fn classify_gh_allows_read_only_views() {
        let policy = CommandPolicy;
        assert_eq!(
            policy.classify_gh(&["pr".to_string(), "list".to_string()], &workspace()),
            CommandSafety::ReadOnly
        );
        assert_eq!(
            policy.classify_gh(&["status".to_string()], &workspace()),
            CommandSafety::ReadOnly
        );
    }

    #[test]
    fn classify_gh_treats_auth_login_as_mutating() {
        let policy = CommandPolicy;
        assert_eq!(
            policy.classify_gh(&["auth".to_string(), "login".to_string()], &workspace()),
            CommandSafety::Mutating
        );
        assert_eq!(
            policy.classify_gh(&["auth".to_string(), "status".to_string()], &workspace()),
            CommandSafety::ReadOnly
        );
        assert!(matches!(
            policy.classify_gh(&["auth".to_string(), "token".to_string()], &workspace()),
            CommandSafety::Blocked(_)
        ));
        assert_eq!(
            policy.classify_gh(&["auth".to_string(), "setup-git".to_string()], &workspace()),
            CommandSafety::Mutating
        );
    }

    #[test]
    fn classify_gh_treats_api_post_as_mutating() {
        let policy = CommandPolicy;
        for args in [
            vec![
                "api".to_string(),
                "-XPOST".to_string(),
                "repos/foo".to_string(),
            ],
            vec![
                "api".to_string(),
                "-X".to_string(),
                "POST".to_string(),
                "repos/foo".to_string(),
            ],
            vec![
                "api".to_string(),
                "--method".to_string(),
                "PATCH".to_string(),
                "repos/foo".to_string(),
            ],
            vec![
                "api".to_string(),
                "--method=DELETE".to_string(),
                "repos/foo".to_string(),
            ],
            vec![
                "api".to_string(),
                "repos/foo/issues/1/comments".to_string(),
                "-f".to_string(),
                "body=hi".to_string(),
            ],
            vec![
                "api".to_string(),
                "repos/foo/issues/1/comments".to_string(),
                "--field".to_string(),
                "body=hi".to_string(),
            ],
            vec![
                "api".to_string(),
                "repos/foo/issues/1/comments".to_string(),
                "-Fbody=hi".to_string(),
            ],
            vec![
                "api".to_string(),
                "repos/foo/issues/1/comments".to_string(),
                "--raw-field=body=hi".to_string(),
            ],
        ] {
            assert_eq!(
                policy.classify_gh(&args, &workspace()),
                CommandSafety::Mutating
            );
        }
        assert_eq!(
            policy.classify_gh(&["api".to_string(), "repos/foo".to_string()], &workspace()),
            CommandSafety::ReadOnly
        );
        assert_eq!(
            policy.classify_gh(
                &[
                    "api".to_string(),
                    "-X".to_string(),
                    "GET".to_string(),
                    "search/issues".to_string(),
                    "-f".to_string(),
                    "q=repo:cli/cli is:open".to_string()
                ],
                &workspace()
            ),
            CommandSafety::ReadOnly
        );
        assert_eq!(
            policy.classify_gh(
                &[
                    "api".to_string(),
                    "--method=HEAD".to_string(),
                    "repos/foo".to_string(),
                    "--field".to_string(),
                    "probe=true".to_string()
                ],
                &workspace()
            ),
            CommandSafety::ReadOnly
        );
    }

    #[test]
    fn classify_gh_treats_create_as_mutating() {
        let policy = CommandPolicy;
        assert_eq!(
            policy.classify_gh(&["pr".to_string(), "create".to_string()], &workspace()),
            CommandSafety::Mutating
        );
    }

    #[tokio::test]
    async fn executor_runs_read_only_command_directly() {
        let executor = CommandExecutor::new(workspace(), None);
        let output = executor
            .run_shell("echo".to_string(), vec!["hello".to_string()], Some(5))
            .await
            .expect("execution should succeed");
        assert!(output.success);
        assert!(output.stdout.contains("hello"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bounded_capture_truncates_large_stdout_without_deadlock() {
        let executor = CommandExecutor::new(workspace(), None);
        let output = executor
            .execute(
                "sh",
                &["-c".to_string(), "yes x | head -c 131072".to_string()],
                Duration::from_secs(2),
            )
            .await
            .expect("execution should succeed");

        assert!(output.success);
        assert!(output.truncated);
        assert!(output.stdout.len() <= COMMAND_OUTPUT_CAP + 100);
    }

    #[tokio::test]
    async fn mutating_command_without_approval_is_denied() {
        let executor = CommandExecutor::new(workspace(), None);
        let output = executor
            .run_shell("mkdir".to_string(), vec!["some-dir".to_string()], Some(5))
            .await
            .expect("tool should return output");
        assert!(!output.success);
        assert_eq!(output.reason.as_deref(), Some("approval_denied"));
        assert_eq!(output.needs_approval, Some(true));
    }

    #[tokio::test]
    async fn mutating_command_with_approval_runs() {
        let approved = Arc::new(AtomicBool::new(true));
        let approval = Arc::new(AlwaysApproval {
            approved: approved.clone(),
        });
        let executor = CommandExecutor::new(workspace(), Some(approval));
        let output = executor
            .run_shell("echo".to_string(), vec!["approved".to_string()], Some(5))
            .await
            .expect("execution should succeed");
        assert!(output.success);
        assert!(output.stdout.contains("approved"));
    }

    #[tokio::test]
    async fn blocked_command_returns_blocked_reason() {
        let executor = CommandExecutor::new(workspace(), None);
        let output = executor
            .run_shell(
                "bash".to_string(),
                vec!["-c".to_string(), "echo hi".to_string()],
                Some(5),
            )
            .await
            .expect("tool should return output");
        assert!(!output.success);
        assert_eq!(output.reason.as_deref(), Some("shell_interpreter"));
    }

    struct AlwaysApproval {
        approved: Arc<AtomicBool>,
    }

    impl CommandApproval for AlwaysApproval {
        fn request_approval<'a>(
            &'a self,
            _request: CommandApprovalRequest,
        ) -> CommandApprovalFuture<'a> {
            let approved = self.approved.clone();
            Box::pin(async move { approved.load(Ordering::SeqCst) })
        }
    }
}
