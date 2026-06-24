use globset::{Glob, GlobMatcher, GlobSet, GlobSetBuilder};
use once_cell::sync::Lazy;
use regex::Regex;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::cmp::Ordering;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

pub const WORKSPACE_TOOLS_SYSTEM_PROMPT: &str = r#"
## Live Workspace Tools

You can inspect the active Gospel workspace with live tools.

### Use These Tools For Source Of Truth

- Use `find_files` to discover relevant files.
- Use `list_directory` to inspect directory structure.
- Use `search_code` to find where text or symbols appear.
- Use `read_file` to verify exact code and content.
- Use `source_edit` to apply one exact replacement to an existing safe UTF-8 source file after verifying the current contents.

### Context Search

- Use `context_search` for broad natural-language retrieval across source, docs, and project artifacts.
- Context Search returns ranked results with path, line range, content type, snippet, and score.
- **Always verify important claims with live workspace tools** (read_file, search_code) after using Context Search.
- Use Context Search to find likely relevant areas, then use live tools to confirm details.

### Guidance

- Live workspace tools are the source of truth for file contents.
- Prefer `find_files` or `search_code` before making broad claims.
- If corpus tools are available, use them for fast structural orientation and live reads for verification.
- Stay within the active workspace.
- `source_edit` is for narrow in-place changes only. It requires one exact `old_text` match, rejects unsafe targets, and returns a capped diff preview.
"#;

pub const HARNESS_CONTROL_AREA_SYSTEM_PROMPT: &str = r#"
## Harness Control Area (Persistent Substrate)

The `.gospel/` directory is the persistent harness substrate for this workspace. It stores durable control artifacts that survive across turns and sessions.

### Primary Artifact: `.gospel/PLAN.md`

When working on multi-step or long-horizon tasks, maintain a system plan at `.gospel/PLAN.md`. This is the agent's outer persistent record of goal, progress, evidence, and next actions.

**Required lightweight structure** (use these exact headings):

```markdown
# Plan

## Goal
<one-sentence description of what we are trying to accomplish>

## Steps
- [x] <completed step>
- [ ] <current or future step>

## Evidence / Verification
<what has been verified so far — test results, manual checks, tool outputs>

## Open Questions / Risks
<blockers, unknowns, decisions still needed>

## Next Action
<the single most important thing to do next>
```

### Guidance

- Use `read_file` with path `.gospel/PLAN.md` to read the current plan at the start of a task or when resuming.
- Use `write_harness_file` with path `.gospel/PLAN.md` to create or update the plan after meaningful progress.
- Keep the plan concise and current. Update Steps, Evidence, and Next Action as work progresses.
- The plan is the source of truth for what has been done and what remains. Prefer updating it over relying on conversational memory.
- **When a skill is active** (e.g. /tdd, /diagnose, or any other invoked or matched skill), still maintain the plan. The skill governs the workflow discipline; the plan is the outer persistent record that tracks goal, progress, and evidence across turns. Initialize the plan when the skill starts, update it after each meaningful step, and mark it complete when the skill's work is done.
- The `.gospel/corpus/` subdirectory is managed internally and should not be edited directly.
"#;

const READ_DEFAULT_LINE_CAP: usize = 200;
const READ_ABSOLUTE_LINE_CAP: usize = 400;
const READ_RESPONSE_BYTES_CAP: usize = 64 * 1024;
const READ_FILE_BYTES_CAP: u64 = 1024 * 1024;
const SOURCE_EDIT_FILE_BYTES_CAP: u64 = 1024 * 1024;
const SOURCE_EDIT_DIFF_PREVIEW_BYTES_CAP: usize = 16 * 1024;
const SOURCE_EDIT_CONTEXT_LINES: usize = 3;
const SEARCH_DEFAULT_MATCH_CAP: usize = 50;
const SEARCH_ABSOLUTE_MATCH_CAP: usize = 200;
const SEARCH_FILE_SCAN_CAP: usize = 500;
const SEARCH_TOTAL_BYTES_CAP: u64 = 16 * 1024 * 1024;
const SEARCH_FILE_BYTES_CAP: u64 = 256 * 1024;
const FIND_DEFAULT_RESULTS_CAP: usize = 100;
const FIND_ABSOLUTE_RESULTS_CAP: usize = 500;
const LIST_DEFAULT_DEPTH: usize = 2;
const LIST_ABSOLUTE_DEPTH: usize = 5;
const LIST_DEFAULT_ENTRIES_CAP: usize = 200;
const LIST_ABSOLUTE_ENTRIES_CAP: usize = 1000;
const VISITED_ENTRY_CAP: usize = 5000;
const DISPLAY_LINE_CHAR_CAP: usize = 500;
const BINARY_SAMPLE_BYTES: usize = 4096;

static HIDDEN_ALLOWLIST: Lazy<GlobSet> = Lazy::new(|| {
    build_globset(&[
        ".github",
        ".github/**",
        ".vscode",
        ".vscode/**",
        ".devcontainer",
        ".devcontainer/**",
        ".cargo",
        ".cargo/**",
        ".agents",
        ".agents/**",
        ".opencode",
        ".opencode/**",
        ".gospel",
        ".gospel/**",
        ".gitignore",
        ".gitattributes",
        ".gitmodules",
        ".editorconfig",
        ".env.example",
        ".env.sample",
        ".env.template",
        ".nvmrc",
        ".tool-versions",
    ])
});

const HIDDEN_ALLOWLISTED_FILENAMES: &[&str] = &[
    ".gitignore",
    ".gitattributes",
    ".gitmodules",
    ".editorconfig",
    ".env.example",
    ".env.sample",
    ".env.template",
    ".nvmrc",
    ".tool-versions",
];

#[derive(Debug, Error)]
pub enum WorkspaceToolError {
    #[error("workspace root is unavailable: {0}")]
    WorkspaceUnavailable(String),
    #[error("workspace tool internal error: {0}")]
    Internal(String),
}

#[derive(Debug)]
enum AccessFailure {
    Blocked(String),
    NotFound(String),
    Io(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone)]
struct ResolvedPath {
    absolute_path: PathBuf,
    relative_path: PathBuf,
    exists: bool,
    is_dir: bool,
    is_file: bool,
    is_symlink: bool,
}

#[derive(Debug, Clone)]
struct WorkspaceAccess {
    root: PathBuf,
    canonical_root: PathBuf,
}

impl WorkspaceAccess {
    fn new(root: &Path) -> Result<Self, WorkspaceToolError> {
        let canonical_root = fs::canonicalize(root).map_err(|e| {
            WorkspaceToolError::WorkspaceUnavailable(format!(
                "failed to canonicalize workspace root {}: {}",
                root.display(),
                e
            ))
        })?;

        if !canonical_root.is_dir() {
            return Err(WorkspaceToolError::WorkspaceUnavailable(format!(
                "workspace root is not a directory: {}",
                canonical_root.display()
            )));
        }

        Ok(Self {
            root: canonical_root.clone(),
            canonical_root,
        })
    }

    fn resolve_path(&self, input: Option<&str>) -> Result<ResolvedPath, AccessFailure> {
        let trimmed = input.map(str::trim).filter(|value| !value.is_empty());
        let requested = trimmed
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    self.root.join(path)
                }
            })
            .unwrap_or_else(|| self.root.clone());

        let normalized = normalize_path(&requested);

        if normalized.exists() {
            let canonical = fs::canonicalize(&normalized)
                .map_err(|e| AccessFailure::Io(format!("Failed to resolve path: {}", e)))?;
            if !canonical.starts_with(&self.canonical_root) {
                return Err(AccessFailure::Blocked(
                    "Path escapes the active workspace via a symlink.".to_string(),
                ));
            }

            let symlink_metadata = fs::symlink_metadata(&normalized)
                .map_err(|e| AccessFailure::Io(format!("Failed to inspect path: {}", e)))?;
            let metadata = fs::metadata(&normalized)
                .map_err(|e| AccessFailure::Io(format!("Failed to inspect path: {}", e)))?;
            let relative_path = canonical
                .strip_prefix(&self.canonical_root)
                .map(PathBuf::from)
                .map_err(|_| {
                    AccessFailure::Blocked("Path escapes the active workspace.".to_string())
                })?;

            return Ok(ResolvedPath {
                absolute_path: canonical,
                relative_path,
                exists: true,
                is_dir: metadata.is_dir(),
                is_file: metadata.is_file(),
                is_symlink: symlink_metadata.file_type().is_symlink(),
            });
        }

        let mut ancestor = normalized.as_path();
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or_else(|| {
                AccessFailure::NotFound("Path does not exist in the active workspace.".to_string())
            })?;
        }

        let canonical_ancestor = fs::canonicalize(ancestor)
            .map_err(|e| AccessFailure::Io(format!("Failed to resolve path ancestor: {}", e)))?;
        if !canonical_ancestor.starts_with(&self.canonical_root) {
            return Err(AccessFailure::Blocked(
                "Path escapes the active workspace via a symlink.".to_string(),
            ));
        }

        let relative_path = normalized
            .strip_prefix(&self.root)
            .map(PathBuf::from)
            .map_err(|_| {
                AccessFailure::Blocked("Path escapes the active workspace.".to_string())
            })?;

        Ok(ResolvedPath {
            absolute_path: normalized,
            relative_path,
            exists: false,
            is_dir: false,
            is_file: false,
            is_symlink: false,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct ReadFileArgs {
    path: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileTool {
    workspace_root: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct ReadFileOutput {
    success: bool,
    message: String,
    reason: Option<String>,
    path: Option<String>,
    size_bytes: Option<u64>,
    start_line: Option<usize>,
    end_line: Option<usize>,
    total_lines: Option<usize>,
    truncated: bool,
    content: Option<String>,
}

impl Tool for ReadFileTool {
    const NAME: &'static str = "read_file";

    type Error = WorkspaceToolError;
    type Args = ReadFileArgs;
    type Output = ReadFileOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read a safe text file from the active workspace. Returns line-numbered content with caps and truncation metadata.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative or absolute file path inside the active workspace."
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "Optional 1-based inclusive start line."
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "Optional 1-based inclusive end line."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let access = WorkspaceAccess::new(&self.workspace_root)?;
        let resolved = match access.resolve_path(Some(&args.path)) {
            Ok(resolved) => resolved,
            Err(error) => return Ok(read_failure(error)),
        };

        if !resolved.exists {
            return Ok(read_failure(AccessFailure::NotFound(
                "File does not exist in the active workspace.".to_string(),
            )));
        }

        if !resolved.is_file {
            return Ok(read_failure(AccessFailure::Blocked(
                "Path points to a directory, not a file.".to_string(),
            )));
        }

        if resolved.is_symlink {
            return Ok(read_failure(AccessFailure::Blocked(
                "Symlinked files cannot be read from chat.".to_string(),
            )));
        }

        if let Some(reason) = blocked_path_reason(&resolved.relative_path, PathKind::File, false) {
            return Ok(read_failure(AccessFailure::Blocked(reason)));
        }

        let metadata = fs::metadata(&resolved.absolute_path)
            .map_err(|e| WorkspaceToolError::Internal(format!("Failed to read metadata: {}", e)))?;
        if metadata.len() > READ_FILE_BYTES_CAP {
            let mut output = read_failure(AccessFailure::Blocked(format!(
                "File is too large to read safely ({} bytes).",
                metadata.len()
            )));
            output.reason = Some("oversized".to_string());
            return Ok(output);
        }

        let bytes = fs::read(&resolved.absolute_path)
            .map_err(|e| WorkspaceToolError::Internal(format!("Failed to read file: {}", e)))?;
        if is_binary(&bytes) {
            let mut output = read_failure(AccessFailure::Blocked(
                "Binary files cannot be read from chat.".to_string(),
            ));
            output.reason = Some("binary".to_string());
            return Ok(output);
        }

        let text = match String::from_utf8(bytes.clone()) {
            Ok(text) => text,
            Err(_) => {
                let mut output = read_failure(AccessFailure::Blocked(
                    "Binary files cannot be read from chat.".to_string(),
                ));
                output.reason = Some("binary".to_string());
                return Ok(output);
            }
        };

        let lines: Vec<&str> = text.lines().collect();
        let total_lines = lines.len();
        if total_lines == 0 {
            if args.start_line.is_some() || args.end_line.is_some() {
                return Ok(read_invalid_range(
                    "Cannot read a line range from an empty file.",
                ));
            }

            return Ok(ReadFileOutput {
                success: true,
                message: format!(
                    "Read empty file {}.",
                    display_rel_path(&resolved.relative_path)
                ),
                reason: None,
                path: Some(display_rel_path(&resolved.relative_path)),
                size_bytes: Some(metadata.len()),
                start_line: Some(0),
                end_line: Some(0),
                total_lines: Some(0),
                truncated: false,
                content: Some(String::new()),
            });
        }

        let start_line = args.start_line.unwrap_or(1);
        if start_line == 0 {
            return Ok(read_invalid_range("start_line must be 1 or greater."));
        }

        if start_line > total_lines {
            return Ok(read_invalid_range(
                "start_line is beyond the end of the file.",
            ));
        }

        let requested_end = match args.end_line {
            Some(0) => {
                return Ok(read_invalid_range("end_line must be 1 or greater."));
            }
            Some(end_line) if end_line < start_line => {
                return Ok(read_invalid_range(
                    "end_line must be greater than or equal to start_line.",
                ));
            }
            Some(end_line) => end_line,
            None => start_line.saturating_add(READ_DEFAULT_LINE_CAP.saturating_sub(1)),
        };

        let hard_end = start_line
            .saturating_add(READ_ABSOLUTE_LINE_CAP.saturating_sub(1))
            .min(total_lines);
        let mut effective_end = requested_end.min(hard_end).min(total_lines);
        let mut truncated = requested_end > effective_end;

        let mut numbered = Vec::new();
        let mut current_bytes = 0usize;
        let mut last_line = start_line.saturating_sub(1);

        for line_number in start_line..=effective_end {
            let line = truncate_line(lines[line_number - 1], DISPLAY_LINE_CHAR_CAP);
            if line.truncated {
                truncated = true;
            }

            let entry = format!("{}: {}", line_number, line.text);
            let entry_len = entry.len();
            let separator_len = if numbered.is_empty() { 0 } else { 1 };
            if current_bytes + separator_len + entry_len > READ_RESPONSE_BYTES_CAP {
                truncated = true;
                effective_end = last_line;
                break;
            }

            current_bytes += separator_len + entry_len;
            numbered.push(entry);
            last_line = line_number;
        }

        Ok(ReadFileOutput {
            success: true,
            message: format!(
                "Read {} lines from {}.",
                if numbered.is_empty() {
                    0
                } else {
                    effective_end - start_line + 1
                },
                display_rel_path(&resolved.relative_path)
            ),
            reason: None,
            path: Some(display_rel_path(&resolved.relative_path)),
            size_bytes: Some(metadata.len()),
            start_line: Some(start_line),
            end_line: Some(effective_end),
            total_lines: Some(total_lines),
            truncated,
            content: Some(numbered.join("\n")),
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchCodeArgs {
    pattern: String,
    path: Option<String>,
    include_glob: Option<String>,
    max_results: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchCodeTool {
    workspace_root: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct SearchCodeMatch {
    path: String,
    line: usize,
    text: String,
}

#[derive(Debug, Serialize)]
pub struct SearchCodeOutput {
    success: bool,
    message: String,
    reason: Option<String>,
    matches: Vec<SearchCodeMatch>,
    truncated: bool,
    scanned_files: usize,
    skipped_files: usize,
}

impl Tool for SearchCodeTool {
    const NAME: &'static str = "search_code";

    type Error = WorkspaceToolError;
    type Args = SearchCodeArgs;
    type Output = SearchCodeOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search safe text files in the active workspace with a regular expression. Returns matching lines with workspace-relative paths.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Regular expression pattern to search for."
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional workspace-relative or absolute file or directory scope."
                    },
                    "include_glob": {
                        "type": "string",
                        "description": "Optional glob applied relative to the scoped path."
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Optional maximum result count."
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let access = WorkspaceAccess::new(&self.workspace_root)?;
        let scope = match access.resolve_path(args.path.as_deref()) {
            Ok(scope) => scope,
            Err(error) => {
                let reason = access_failure_reason(&error).to_string();
                return Ok(search_failure(error, &reason));
            }
        };

        if !scope.exists {
            return Ok(search_failure(
                AccessFailure::NotFound(
                    "Search scope does not exist in the active workspace.".to_string(),
                ),
                "not_found",
            ));
        }

        let scope_kind = if scope.is_file {
            PathKind::File
        } else if scope.is_dir {
            PathKind::Directory
        } else if scope.is_symlink {
            PathKind::Symlink
        } else {
            PathKind::Other
        };
        if let Some(reason) = blocked_path_reason(&scope.relative_path, scope_kind, true) {
            return Ok(search_failure(AccessFailure::Blocked(reason), "blocked"));
        }
        if scope.is_symlink {
            return Ok(search_failure(
                AccessFailure::Blocked(
                    "Symlink scopes are not supported by workspace search.".to_string(),
                ),
                "blocked",
            ));
        }

        let regex = match Regex::new(&args.pattern) {
            Ok(regex) => regex,
            Err(error) => {
                return Ok(SearchCodeOutput {
                    success: false,
                    message: format!("Invalid regex pattern: {}", error),
                    reason: Some("invalid_regex".to_string()),
                    matches: vec![],
                    truncated: false,
                    scanned_files: 0,
                    skipped_files: 0,
                });
            }
        };

        let include_matcher = match args.include_glob.as_deref() {
            Some(pattern) => Some(match Glob::new(pattern) {
                Ok(glob) => glob.compile_matcher(),
                Err(error) => {
                    return Ok(SearchCodeOutput {
                        success: false,
                        message: format!("Invalid include_glob pattern: {}", error),
                        reason: Some("invalid_glob".to_string()),
                        matches: vec![],
                        truncated: false,
                        scanned_files: 0,
                        skipped_files: 0,
                    });
                }
            }),
            None => None,
        };

        let max_results = clamp_limit(
            args.max_results,
            SEARCH_DEFAULT_MATCH_CAP,
            SEARCH_ABSOLUTE_MATCH_CAP,
        );
        let scope_match_base = if scope.is_file {
            scope
                .relative_path
                .parent()
                .map(PathBuf::from)
                .unwrap_or_default()
        } else {
            scope.relative_path.clone()
        };

        let mut state = SearchState {
            regex: &regex,
            include_matcher: include_matcher.as_ref(),
            matches: Vec::new(),
            scanned_files: 0,
            skipped_files: 0,
            scanned_bytes: 0,
            visited_entries: 0,
            truncated: false,
            max_results,
            scope_match_base,
            scope_is_file: scope.is_file,
        };

        if scope.is_file {
            state.visited_entries = 1;
            search_file(&scope, &mut state)?;
        } else if scope.is_dir {
            walk_search_directory(&access, &scope.absolute_path, &mut state)?;
        }

        Ok(SearchCodeOutput {
            success: true,
            message: format!(
                "Found {} matches across {} files.",
                state.matches.len(),
                state.scanned_files
            ),
            reason: None,
            matches: state.matches,
            truncated: state.truncated,
            scanned_files: state.scanned_files,
            skipped_files: state.skipped_files,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct FindFilesArgs {
    glob: String,
    path: Option<String>,
    max_results: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindFilesTool {
    workspace_root: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct FindFilesOutput {
    success: bool,
    message: String,
    reason: Option<String>,
    files: Vec<String>,
    truncated: bool,
    scanned_entries: usize,
}

impl Tool for FindFilesTool {
    const NAME: &'static str = "find_files";

    type Error = WorkspaceToolError;
    type Args = FindFilesArgs;
    type Output = FindFilesOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Find safe files in the active workspace with a glob pattern. Returns workspace-relative file paths only.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "glob": {
                        "type": "string",
                        "description": "Glob pattern to match relative to the scoped path."
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional workspace-relative or absolute directory or file scope."
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Optional maximum result count."
                    }
                },
                "required": ["glob"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let access = WorkspaceAccess::new(&self.workspace_root)?;
        let scope = match access.resolve_path(args.path.as_deref()) {
            Ok(scope) => scope,
            Err(error) => {
                let reason = access_failure_reason(&error).to_string();
                return Ok(find_failure(error, &reason));
            }
        };

        if !scope.exists {
            return Ok(find_failure(
                AccessFailure::NotFound(
                    "Find scope does not exist in the active workspace.".to_string(),
                ),
                "not_found",
            ));
        }

        let scope_kind = if scope.is_file {
            PathKind::File
        } else if scope.is_dir {
            PathKind::Directory
        } else if scope.is_symlink {
            PathKind::Symlink
        } else {
            PathKind::Other
        };
        if let Some(reason) = blocked_path_reason(&scope.relative_path, scope_kind, true) {
            return Ok(find_failure(AccessFailure::Blocked(reason), "blocked"));
        }
        if scope.is_symlink {
            return Ok(find_failure(
                AccessFailure::Blocked(
                    "Symlink scopes are not supported by workspace discovery.".to_string(),
                ),
                "blocked",
            ));
        }

        let matcher = match Glob::new(&args.glob) {
            Ok(glob) => glob.compile_matcher(),
            Err(error) => {
                return Ok(FindFilesOutput {
                    success: false,
                    message: format!("Invalid glob pattern: {}", error),
                    reason: Some("invalid_glob".to_string()),
                    files: vec![],
                    truncated: false,
                    scanned_entries: 0,
                });
            }
        };

        let max_results = clamp_limit(
            args.max_results,
            FIND_DEFAULT_RESULTS_CAP,
            FIND_ABSOLUTE_RESULTS_CAP,
        );
        let scope_match_base = if scope.is_file {
            scope
                .relative_path
                .parent()
                .map(PathBuf::from)
                .unwrap_or_default()
        } else {
            scope.relative_path.clone()
        };

        let mut state = FindState {
            matcher,
            files: Vec::new(),
            visited_entries: 0,
            truncated: false,
            max_results,
            scope_match_base,
            scope_is_file: scope.is_file,
        };

        if scope.is_file {
            state.visited_entries = 1;
            consider_found_file(&scope, &mut state);
        } else if scope.is_dir {
            walk_find_directory(&access, &scope.absolute_path, &mut state)?;
        }

        state.files.sort();
        if state.files.len() > state.max_results {
            state.truncated = true;
            state.files.truncate(state.max_results);
        }

        Ok(FindFilesOutput {
            success: true,
            message: format!("Found {} matching files.", state.files.len()),
            reason: None,
            files: state.files,
            truncated: state.truncated,
            scanned_entries: state.visited_entries,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct ListDirectoryArgs {
    path: Option<String>,
    depth: Option<usize>,
    max_entries: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDirectoryTool {
    workspace_root: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct DirectoryEntryOutput {
    path: String,
    name: String,
    kind: String,
    size_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ListDirectoryOutput {
    success: bool,
    message: String,
    reason: Option<String>,
    entries: Vec<DirectoryEntryOutput>,
    truncated: bool,
    visited_entries: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceRootInventoryItem {
    pub path: String,
    pub kind: String,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceRootInventory {
    pub entries: Vec<WorkspaceRootInventoryItem>,
    pub visited_entries: usize,
    pub truncated: bool,
}

impl Tool for ListDirectoryTool {
    const NAME: &'static str = "list_directory";

    type Error = WorkspaceToolError;
    type Args = ListDirectoryArgs;
    type Output = ListDirectoryOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List safe files and directories from the active workspace. Returns deterministic directory-first ordering and does not recurse into symlinked directories.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Optional workspace-relative or absolute directory scope."
                    },
                    "depth": {
                        "type": "integer",
                        "description": "Optional recursion depth."
                    },
                    "max_entries": {
                        "type": "integer",
                        "description": "Optional maximum entry count."
                    }
                },
                "required": []
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let access = WorkspaceAccess::new(&self.workspace_root)?;
        let scope = match access.resolve_path(args.path.as_deref()) {
            Ok(scope) => scope,
            Err(error) => {
                let reason = access_failure_reason(&error).to_string();
                return Ok(list_failure(error, &reason));
            }
        };

        if !scope.exists {
            return Ok(list_failure(
                AccessFailure::NotFound(
                    "Directory scope does not exist in the active workspace.".to_string(),
                ),
                "not_found",
            ));
        }

        if !scope.is_dir {
            return Ok(list_failure(
                AccessFailure::Blocked("Path points to a file, not a directory.".to_string()),
                "not_directory",
            ));
        }

        if let Some(reason) = blocked_path_reason(&scope.relative_path, PathKind::Directory, true) {
            return Ok(list_failure(AccessFailure::Blocked(reason), "blocked"));
        }
        if scope.is_symlink {
            return Ok(list_failure(
                AccessFailure::Blocked(
                    "Symlink scopes are not supported by directory listing.".to_string(),
                ),
                "blocked",
            ));
        }

        let max_depth = clamp_limit(args.depth, LIST_DEFAULT_DEPTH, LIST_ABSOLUTE_DEPTH);
        let max_entries = clamp_limit(
            args.max_entries,
            LIST_DEFAULT_ENTRIES_CAP,
            LIST_ABSOLUTE_ENTRIES_CAP,
        );

        let mut state = ListState {
            entries: Vec::new(),
            visited_entries: 0,
            truncated: false,
            max_entries,
        };

        walk_list_directory(&access, &scope.absolute_path, max_depth, &mut state)?;

        Ok(ListDirectoryOutput {
            success: true,
            message: format!("Listed {} entries.", state.entries.len()),
            reason: None,
            entries: state.entries,
            truncated: state.truncated,
            visited_entries: state.visited_entries,
        })
    }
}

pub fn workspace_root_inventory(
    workspace_root: &Path,
    max_entries: usize,
) -> Result<WorkspaceRootInventory, WorkspaceToolError> {
    let access = WorkspaceAccess::new(workspace_root)?;
    let entries = read_sorted_directory_entries(workspace_root)?;
    let mut visited_entries = 0usize;
    let mut truncated = false;
    let mut snapshot_entries = Vec::new();

    for entry in entries.into_iter() {
        visited_entries += 1;
        if visited_entries > VISITED_ENTRY_CAP {
            truncated = true;
            break;
        }

        let relative_path = entry.relative_path(&access);
        if entry.is_symlink {
            continue;
        }
        if blocked_path_reason(&relative_path, entry.path_kind(), true).is_some() {
            continue;
        }

        snapshot_entries.push(WorkspaceRootInventoryItem {
            path: display_rel_path(&relative_path),
            kind: entry.kind_name(),
            size_bytes: if entry.is_file() { Some(entry.size_bytes) } else { None },
        });

        if snapshot_entries.len() >= max_entries.max(1) {
            truncated = true;
            break;
        }
    }

    Ok(WorkspaceRootInventory {
        entries: snapshot_entries,
        visited_entries,
        truncated,
    })
}

impl std::fmt::Display for WorkspaceRootInventory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for entry in &self.entries {
            writeln!(f, "{} ({})", entry.path, entry.kind)?;
        }
        if self.truncated {
            writeln!(f, "... (truncated)")?;
        }
        Ok(())
    }
}

pub fn create_read_file_tool(workspace_root: PathBuf) -> ReadFileTool {
    ReadFileTool { workspace_root }
}

pub fn create_search_code_tool(workspace_root: PathBuf) -> SearchCodeTool {
    SearchCodeTool { workspace_root }
}

pub fn create_find_files_tool(workspace_root: PathBuf) -> FindFilesTool {
    FindFilesTool { workspace_root }
}

pub fn create_list_directory_tool(workspace_root: PathBuf) -> ListDirectoryTool {
    ListDirectoryTool { workspace_root }
}

pub fn build_base_workspace_tools(workspace_path: PathBuf) -> Vec<Box<dyn rig::tool::ToolDyn>> {
    vec![
        Box::new(create_read_file_tool(workspace_path.clone())),
        Box::new(create_search_code_tool(workspace_path.clone())),
        Box::new(create_find_files_tool(workspace_path.clone())),
        Box::new(create_list_directory_tool(workspace_path)),
    ]
}

#[derive(Debug, Deserialize)]
pub struct SourceEditArgs {
    path: String,
    old_text: String,
    new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEditTool {
    workspace_root: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct SourceEditOutput {
    success: bool,
    message: String,
    reason: Option<String>,
    path: Option<String>,
    changed: bool,
    replacements: usize,
    start_line: Option<usize>,
    end_line: Option<usize>,
    size_bytes: Option<u64>,
    diff_preview: Option<String>,
    truncated: bool,
}

impl Tool for SourceEditTool {
    const NAME: &'static str = "source_edit";

    type Error = WorkspaceToolError;
    type Args = SourceEditArgs;
    type Output = SourceEditOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Apply exactly one in-place replacement to an existing safe UTF-8 workspace source file. The target must not be hidden control data, generated/noisy output, a lockfile, a secret-like file, a symlink, binary, oversized, or outside the active workspace.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative or absolute file path inside the active workspace."
                    },
                    "old_text": {
                        "type": "string",
                        "description": "Exact existing text to replace. It must occur exactly once."
                    },
                    "new_text": {
                        "type": "string",
                        "description": "Replacement text."
                    }
                },
                "required": ["path", "old_text", "new_text"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let access = WorkspaceAccess::new(&self.workspace_root)?;
        let path = args.path.trim();
        if path.is_empty() {
            return Ok(source_edit_failure(
                "blocked",
                "Path must not be empty.",
                None,
            ));
        }

        if args.old_text.is_empty() {
            return Ok(source_edit_failure(
                "invalid_replacement",
                "old_text must not be empty.",
                None,
            ));
        }

        if args.old_text == args.new_text {
            return Ok(source_edit_failure(
                "no_op",
                "old_text and new_text are identical; no edit was applied.",
                None,
            ));
        }

        let resolved = match access.resolve_path(Some(path)) {
            Ok(resolved) => resolved,
            Err(error) => {
                return Ok(source_edit_failure(
                    access_failure_reason(&error),
                    &match error {
                        AccessFailure::Blocked(m)
                        | AccessFailure::NotFound(m)
                        | AccessFailure::Io(m) => m,
                    },
                    None,
                ));
            }
        };

        if !resolved.exists {
            return Ok(source_edit_failure(
                "not_found",
                "File does not exist in the active workspace.",
                None,
            ));
        }

        let display_path = display_rel_path(&resolved.relative_path);

        if !resolved.is_file {
            return Ok(source_edit_failure(
                "blocked",
                "Path points to a directory, not a file.",
                Some(display_path),
            ));
        }

        if resolved.is_symlink {
            return Ok(source_edit_failure(
                "blocked",
                "Symlinked files cannot be edited from chat.",
                Some(display_path),
            ));
        }

        if let Some(reason) = source_edit_blocked_path_reason(&resolved.relative_path) {
            return Ok(source_edit_failure("blocked", &reason, Some(display_path)));
        }

        let metadata = fs::metadata(&resolved.absolute_path)
            .map_err(|e| WorkspaceToolError::Internal(format!("Failed to read metadata: {}", e)))?;
        if metadata.len() > SOURCE_EDIT_FILE_BYTES_CAP {
            return Ok(source_edit_failure(
                "oversized",
                &format!(
                    "File is too large to edit safely ({} bytes).",
                    metadata.len()
                ),
                Some(display_path),
            ));
        }

        let bytes = fs::read(&resolved.absolute_path)
            .map_err(|e| WorkspaceToolError::Internal(format!("Failed to read file: {}", e)))?;
        if bytes.contains(&0) {
            return Ok(source_edit_failure(
                "binary",
                "Binary files cannot be edited from chat.",
                Some(display_path),
            ));
        }

        let original = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                return Ok(source_edit_failure(
                    "invalid_utf8",
                    "File is not valid UTF-8 and cannot be edited from chat.",
                    Some(display_path),
                ));
            }
        };

        let mut matches = original.match_indices(&args.old_text);
        let first_match = matches.next();
        let second_match = matches.next();
        let Some((start_index, _)) = first_match else {
            return Ok(source_edit_failure(
                "no_match",
                "old_text was not found in the target file.",
                Some(display_path),
            ));
        };

        if second_match.is_some() {
            return Ok(source_edit_failure(
                "ambiguous_match",
                "old_text occurs more than once; source_edit requires exactly one match.",
                Some(display_path),
            ));
        }

        let end_index = start_index + args.old_text.len();
        let start_line = source_edit_line_number(&original, start_index);
        let end_line = source_edit_line_number(
            &original,
            if args.old_text.ends_with('\n') {
                end_index.saturating_sub(1)
            } else {
                end_index
            },
        );
        let (diff_preview, truncated) = source_edit_diff_preview(
            &display_path,
            &original,
            start_index,
            end_index,
            &args.old_text,
            &args.new_text,
        );

        let mut edited = original.clone();
        edited.replace_range(start_index..end_index, &args.new_text);

        write_atomic_replacement(
            &resolved.absolute_path,
            edited.as_bytes(),
            metadata.permissions(),
        )
        .map_err(|e| WorkspaceToolError::Internal(format!("Failed to write file: {}", e)))?;

        Ok(SourceEditOutput {
            success: true,
            message: format!("Edited {} with one exact replacement.", display_path),
            reason: None,
            path: Some(display_path),
            changed: true,
            replacements: 1,
            start_line: Some(start_line),
            end_line: Some(end_line),
            size_bytes: Some(edited.len() as u64),
            diff_preview: Some(diff_preview),
            truncated,
        })
    }
}

pub fn create_source_edit_tool(workspace_root: PathBuf) -> SourceEditTool {
    SourceEditTool { workspace_root }
}

const HARNESS_PREFIX: &str = ".gospel/";
const HARNESS_CORPUS_PREFIX: &str = ".gospel/corpus/";
const HARNESS_CONTENT_BYTES_CAP: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct WriteHarnessFileArgs {
    path: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteHarnessFileTool {
    workspace_root: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct WriteHarnessFileOutput {
    success: bool,
    message: String,
    reason: Option<String>,
    path: Option<String>,
    size_bytes: Option<u64>,
}

impl Tool for WriteHarnessFileTool {
    const NAME: &'static str = "write_harness_file";

    type Error = WorkspaceToolError;
    type Args = WriteHarnessFileArgs;
    type Output = WriteHarnessFileOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Create or update a file inside the harness control substrate (.gospel/). Use this to maintain PLAN.md and other harness artifacts. The path must be under .gospel/.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative path under .gospel/ (e.g. '.gospel/PLAN.md')."
                    },
                    "content": {
                        "type": "string",
                        "description": "Full content to write to the file."
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let access = WorkspaceAccess::new(&self.workspace_root)?;

        let trimmed_path = args.path.trim();
        if trimmed_path.is_empty() {
            return Ok(harness_failure("blocked", "Path must not be empty."));
        }

        let normalized = normalize_path(&PathBuf::from(trimmed_path));
        let relative_str = path_to_slash(&normalized);

        if !relative_str.starts_with(HARNESS_PREFIX) {
            return Ok(harness_failure(
                "blocked",
                &format!("Path must be under .gospel/ prefix. Got: {}", trimmed_path),
            ));
        }

        if relative_str.starts_with(HARNESS_CORPUS_PREFIX) || relative_str == ".gospel/corpus" {
            return Ok(harness_failure(
                "blocked",
                &format!(
                    "Writing to .gospel/corpus/ is prohibited. Got: {}",
                    trimmed_path
                ),
            ));
        }

        if args.content.len() > HARNESS_CONTENT_BYTES_CAP {
            return Ok(harness_failure(
                "oversized",
                &format!(
                    "Content exceeds the 1 MiB cap ({} bytes).",
                    args.content.len()
                ),
            ));
        }

        let resolved = match access.resolve_path(Some(trimmed_path)) {
            Ok(resolved) => resolved,
            Err(error) => {
                return Ok(harness_failure(
                    access_failure_reason(&error),
                    &match error {
                        AccessFailure::Blocked(m)
                        | AccessFailure::NotFound(m)
                        | AccessFailure::Io(m) => m,
                    },
                ));
            }
        };

        let (canonical_gospel, canonical_corpus) =
            match validate_harness_write_target(&access, &resolved.absolute_path) {
                Ok(paths) => paths,
                Err(error) => {
                    return Ok(harness_failure(
                        access_failure_reason(&error),
                        &match error {
                            AccessFailure::Blocked(m)
                            | AccessFailure::NotFound(m)
                            | AccessFailure::Io(m) => m,
                        },
                    ));
                }
            };

        if let Some(parent) = resolved.absolute_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return Ok(harness_failure(
                    "io_error",
                    &format!("Failed to create parent directories: {}", e),
                ));
            }
        }
        let write_target = &resolved.absolute_path;
        let canonical_parent = match write_target.parent() {
            Some(parent) => match fs::canonicalize(parent) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(harness_failure(
                        "io_error",
                        &format!("Failed to canonicalize write target parent: {}", e),
                    ));
                }
            },
            None => {
                return Ok(harness_failure(
                    "blocked",
                    "Write target has no parent directory.",
                ));
            }
        };
        if !canonical_parent.starts_with(&canonical_gospel) {
            return Ok(harness_failure(
                "blocked",
                "Resolved path escapes .gospel/ via symlink.",
            ));
        }
        if canonical_parent.starts_with(&canonical_corpus) {
            return Ok(harness_failure(
                "blocked",
                "Resolved path points into .gospel/corpus/ via symlink.",
            ));
        }

        let write_result = (|| -> Result<(), String> {
            use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let count = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);

            let parent_dir = resolved
                .absolute_path
                .parent()
                .ok_or_else(|| "Write target has no parent directory.".to_string())?;
            let file_name = resolved
                .absolute_path
                .file_name()
                .unwrap_or_default()
                .to_os_string();
            let mut temp_name = std::ffi::OsString::from(".");
            temp_name.push(&file_name);
            temp_name.push(format!(".tmp.{}.{}", std::process::id(), count));
            let temp_path = parent_dir.join(temp_name);

            let mut file = fs::File::create(&temp_path)
                .map_err(|e| format!("Failed to create temp file for atomic write: {}", e))?;
            file.write_all(args.content.as_bytes()).map_err(|e| {
                let _ = fs::remove_file(&temp_path);
                format!("Failed to write content to temp file: {}", e)
            })?;
            file.sync_all().map_err(|e| {
                let _ = fs::remove_file(&temp_path);
                format!("Failed to sync temp file: {}", e)
            })?;
            drop(file);

            fs::rename(&temp_path, &resolved.absolute_path).map_err(|e| {
                let _ = fs::remove_file(&temp_path);
                format!("Failed to rename temp file to target: {}", e)
            })?;

            if let Ok(dir_handle) = fs::File::open(parent_dir) {
                let _ = dir_handle.sync_all();
            }

            Ok(())
        })();

        if let Err(e) = write_result {
            return Ok(harness_failure(
                "io_error",
                &format!("Failed to write file: {}", e),
            ));
        }

        Ok(WriteHarnessFileOutput {
            success: true,
            message: format!(
                "Wrote {} bytes to {}.",
                args.content.len(),
                display_rel_path(&resolved.relative_path)
            ),
            reason: None,
            path: Some(display_rel_path(&resolved.relative_path)),
            size_bytes: Some(args.content.len() as u64),
        })
    }
}

pub fn create_write_harness_file_tool(workspace_root: PathBuf) -> WriteHarnessFileTool {
    WriteHarnessFileTool { workspace_root }
}

#[derive(Debug, Deserialize)]
pub struct ContextSearchArgs {
    query: String,
    limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSearchTool {
    workspace_root: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct ContextSearchOutput {
    success: bool,
    results: Vec<ContextSearchResultDto>,
    query: String,
    total_results: usize,
    index_available: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContextSearchResultDto {
    path: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
    content_type: String,
    snippet: String,
    score: f64,
}

impl Tool for ContextSearchTool {
    const NAME: &'static str = "context_search";

    type Error = WorkspaceToolError;
    type Args = ContextSearchArgs;
    type Output = ContextSearchOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search the pre-built Context Search index for relevant source code, documentation, and project artifacts using natural language queries. Use this for broad retrieval to find likely relevant areas, then verify important hits with live workspace tools (read_file, search_code) before making claims. Returns ranked results with path, line range, content type, snippet, and score.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language search query (e.g. 'authentication middleware', 'error handling pattern', 'database connection setup')."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 10, max: 50)."
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let query = args.query.trim().to_string();
        if query.is_empty() {
            return Ok(ContextSearchOutput {
                success: false,
                results: vec![],
                query: query.clone(),
                total_results: 0,
                index_available: false,
            });
        }

        let limit = args.limit.unwrap_or(10).min(50);

        // Try to open the context search index
        let index =
            match crate::context_search::ContextSearchIndex::open_if_exists(&self.workspace_root) {
                Ok(index) => index,
                Err(_) => {
                    return Ok(ContextSearchOutput {
                        success: false,
                        results: vec![],
                        query,
                        total_results: 0,
                        index_available: false,
                    });
                }
            };

        // Check if index has any data
        let stats = match index.get_stats() {
            Ok(stats) => stats,
            Err(_) => {
                return Ok(ContextSearchOutput {
                    success: false,
                    results: vec![],
                    query,
                    total_results: 0,
                    index_available: false,
                });
            }
        };

        if stats.chunk_count == 0 {
            return Ok(ContextSearchOutput {
                success: true,
                results: vec![],
                query,
                total_results: 0,
                index_available: true,
            });
        }

        // Perform search
        match index.search(&query, limit) {
            Ok(search_results) => {
                let results: Vec<ContextSearchResultDto> = search_results
                    .into_iter()
                    .map(|r| ContextSearchResultDto {
                        path: r.chunk.source_path,
                        start_line: r.chunk.start_line,
                        end_line: r.chunk.end_line,
                        content_type: r.chunk.source_type,
                        snippet: truncate_snippet(&r.chunk.content, 300),
                        score: r.rank,
                    })
                    .collect();

                let total = results.len();
                Ok(ContextSearchOutput {
                    success: true,
                    results,
                    query,
                    total_results: total,
                    index_available: true,
                })
            }
            Err(_) => Ok(ContextSearchOutput {
                success: false,
                results: vec![],
                query,
                total_results: 0,
                index_available: true,
            }),
        }
    }
}

fn truncate_snippet(content: &str, max_chars: usize) -> String {
    let mut chars = content.chars();
    let snippet: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", snippet)
    } else {
        snippet
    }
}

pub fn create_context_search_tool(workspace_root: PathBuf) -> ContextSearchTool {
    ContextSearchTool { workspace_root }
}

fn validate_harness_write_target(
    access: &WorkspaceAccess,
    target_path: &Path,
) -> Result<(PathBuf, PathBuf), AccessFailure> {
    let canonical_gospel = access.canonical_root.join(".gospel");
    let canonical_corpus = canonical_gospel.join("corpus");
    let relative_target = target_path
        .strip_prefix(&access.canonical_root)
        .map_err(|_| AccessFailure::Blocked("Path escapes the active workspace.".to_string()))?;

    let mut current = access.canonical_root.clone();
    for component in relative_target.components() {
        current.push(component.as_os_str());

        match fs::symlink_metadata(&current) {
            Ok(_) => {
                let canonical_current = fs::canonicalize(&current).map_err(|e| {
                    AccessFailure::Io(format!(
                        "Failed to canonicalize harness path component {}: {}",
                        current.display(),
                        e
                    ))
                })?;

                if !canonical_current.starts_with(&canonical_gospel) {
                    return Err(AccessFailure::Blocked(
                        "Resolved path escapes .gospel/ via symlink.".to_string(),
                    ));
                }
                if canonical_current.starts_with(&canonical_corpus) {
                    return Err(AccessFailure::Blocked(
                        "Resolved path points into .gospel/corpus/ via symlink.".to_string(),
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(AccessFailure::Io(format!(
                    "Failed to inspect harness path component {}: {}",
                    current.display(),
                    error
                )));
            }
        }
    }

    Ok((canonical_gospel, canonical_corpus))
}

fn harness_failure(reason: &str, message: &str) -> WriteHarnessFileOutput {
    WriteHarnessFileOutput {
        success: false,
        message: message.to_string(),
        reason: Some(reason.to_string()),
        path: None,
        size_bytes: None,
    }
}

fn source_edit_failure(reason: &str, message: &str, path: Option<String>) -> SourceEditOutput {
    SourceEditOutput {
        success: false,
        message: message.to_string(),
        reason: Some(reason.to_string()),
        path,
        changed: false,
        replacements: 0,
        start_line: None,
        end_line: None,
        size_bytes: None,
        diff_preview: None,
        truncated: false,
    }
}

fn source_edit_blocked_path_reason(path: &Path) -> Option<String> {
    let rendered = display_rel_path(path);
    if rendered == ".gospel" || rendered.starts_with(".gospel/") {
        return Some(
            "Files under .gospel/ are harness control data and cannot be edited by source_edit."
                .to_string(),
        );
    }

    if is_secret_like(path) {
        return Some("Secret-like files are blocked from source edits.".to_string());
    }

    if has_hidden_directory_component(path) {
        return Some("Hidden control directories are blocked from source edits.".to_string());
    }

    if has_ignored_directory_component(path) {
        return Some("Generated or noisy directories are blocked from source edits.".to_string());
    }

    if is_lockfile(path) {
        return Some("Lockfiles are blocked from source edits.".to_string());
    }

    if is_generated_file(path) {
        return Some("Generated files are blocked from source edits.".to_string());
    }

    None
}

fn source_edit_line_number(text: &str, byte_index: usize) -> usize {
    text[..byte_index.min(text.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn source_edit_diff_preview(
    path: &str,
    original: &str,
    start_index: usize,
    end_index: usize,
    old_text: &str,
    new_text: &str,
) -> (String, bool) {
    let before = &original[..start_index];
    let after = &original[end_index..];
    let start_line = source_edit_line_number(original, start_index);
    let mut lines = vec![format!("@@ {}:{} @@", path, start_line)];
    let mut line_truncated = false;

    let mut push_line = |prefix: &str, line: &str| {
        let truncated_line = truncate_line(line, DISPLAY_LINE_CHAR_CAP);
        line_truncated |= truncated_line.truncated;
        lines.push(format!("{}{}", prefix, truncated_line.text));
    };

    let before_lines: Vec<&str> = before.lines().collect();
    let context_start = before_lines.len().saturating_sub(SOURCE_EDIT_CONTEXT_LINES);
    for line in &before_lines[context_start..] {
        push_line("  ", line);
    }

    for line in diff_text_lines(old_text) {
        push_line("-", line);
    }

    for line in diff_text_lines(new_text) {
        push_line("+", line);
    }

    for line in after.lines().take(SOURCE_EDIT_CONTEXT_LINES) {
        push_line("  ", line);
    }

    let (preview, byte_truncated) =
        truncate_text_bytes(&lines.join("\n"), SOURCE_EDIT_DIFF_PREVIEW_BYTES_CAP);
    (preview, line_truncated || byte_truncated)
}

fn diff_text_lines(text: &str) -> Vec<&str> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        vec![""]
    } else {
        lines
    }
}

fn has_hidden_directory_component(path: &Path) -> bool {
    let components: Vec<_> = path.components().collect();
    components
        .iter()
        .take(components.len().saturating_sub(1))
        .any(|component| match component {
            Component::Normal(part) => part.to_string_lossy().starts_with('.'),
            _ => false,
        })
}

fn is_lockfile(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    matches!(
        name.to_string_lossy().to_ascii_lowercase().as_str(),
        "cargo.lock"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "bun.lock"
            | "bun.lockb"
            | "deno.lock"
            | "poetry.lock"
            | "pipfile.lock"
            | "gemfile.lock"
            | "composer.lock"
            | "go.sum"
    )
}

fn is_generated_file(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let lower = name.to_string_lossy().to_ascii_lowercase();
    is_broad_ignored_file(path)
        || lower.ends_with(".map")
        || lower.contains(".generated.")
        || lower.contains(".gen.")
        || lower.ends_with("_generated.rs")
        || lower.ends_with("_generated.go")
        || lower.ends_with(".pb.go")
        || lower.ends_with("_pb2.py")
}

fn write_atomic_replacement(
    target: &Path,
    content: &[u8],
    permissions: fs::Permissions,
) -> Result<(), String> {
    let parent_dir = target
        .parent()
        .ok_or_else(|| "Write target has no parent directory.".to_string())?;
    let file_name = target.file_name().unwrap_or_default();
    let mut prefix = std::ffi::OsString::from(".");
    prefix.push(file_name);
    prefix.push(".tmp.");

    let mut temp_file = tempfile::Builder::new()
        .prefix(&prefix)
        .tempfile_in(parent_dir)
        .map_err(|e| format!("Failed to create temp file for atomic edit: {}", e))?;
    temp_file
        .as_file()
        .set_permissions(permissions)
        .map_err(|e| format!("Failed to preserve file permissions: {}", e))?;
    temp_file
        .write_all(content)
        .map_err(|e| format!("Failed to write edited content: {}", e))?;
    temp_file
        .as_file()
        .sync_all()
        .map_err(|e| format!("Failed to sync edited content: {}", e))?;

    let persisted = temp_file
        .persist(target)
        .map_err(|e| format!("Failed to replace target file: {}", e.error))?;
    drop(persisted);

    if let Ok(dir_handle) = fs::File::open(parent_dir) {
        let _ = dir_handle.sync_all();
    }

    Ok(())
}

pub(crate) fn truncate_text_bytes(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }

    let suffix = "\n\n[truncated]";
    let suffix_bytes = suffix.len();
    let allowed_bytes = max_bytes.saturating_sub(suffix_bytes);
    let mut last_boundary = 0;
    for (index, _) in text.char_indices() {
        if index > allowed_bytes {
            break;
        }
        last_boundary = index;
    }

    let truncated = text[..last_boundary].trim_end().to_string();
    (format!("{}{}", truncated, suffix), true)
}

fn build_globset(patterns: &[&str]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).expect("valid workspace safety glob"));
    }
    builder.build().expect("valid workspace safety globset")
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn display_rel_path(path: &Path) -> String {
    let rendered = path_to_slash(path);
    if rendered.is_empty() {
        ".".to_string()
    } else {
        rendered
    }
}

fn path_to_slash(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            Component::CurDir => None,
            Component::ParentDir => Some("..".to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn blocked_path_reason(path: &Path, kind: PathKind, broad_tool: bool) -> Option<String> {
    if path.as_os_str().is_empty() {
        return None;
    }

    if kind == PathKind::File && is_secret_like(path) {
        return Some("Secret-like files are blocked from chat tools.".to_string());
    }

    if has_hidden_component(path) && !is_hidden_allowlisted_path(path) {
        return Some("Hidden path is blocked by the workspace safety policy.".to_string());
    }

    if broad_tool {
        if kind == PathKind::Directory && has_ignored_directory_component(path) {
            return Some(
                "Generated or noisy directories are skipped by broad workspace tools.".to_string(),
            );
        }
        if kind == PathKind::File && is_broad_ignored_file(path) {
            return Some(
                "Generated or minified files are skipped by broad workspace tools.".to_string(),
            );
        }
    }

    None
}

fn has_hidden_component(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(part) => part.to_string_lossy().starts_with('.'),
        _ => false,
    })
}

fn is_hidden_allowlisted_path(path: &Path) -> bool {
    if HIDDEN_ALLOWLIST.is_match(path_to_slash(path)) {
        return true;
    }

    let file_name = path.file_name().map(|name| name.to_string_lossy());
    let mut allowed_hidden_file = false;

    for component in path.components() {
        let Component::Normal(part) = component else {
            continue;
        };
        let part = part.to_string_lossy();
        if !part.starts_with('.') {
            continue;
        }

        if file_name.as_deref() == Some(part.as_ref())
            && HIDDEN_ALLOWLISTED_FILENAMES.contains(&part.as_ref())
        {
            allowed_hidden_file = true;
            continue;
        }

        return false;
    }

    allowed_hidden_file
}

fn has_ignored_directory_component(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(part) => matches!(
            part.to_string_lossy().as_ref(),
            "node_modules" | "target" | "dist" | "build" | ".next" | ".nuxt" | "coverage" | "tmp"
        ),
        _ => false,
    })
}

fn is_broad_ignored_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase());
    matches!(
        name.as_deref(),
        Some(name)
            if name.ends_with(".min.js")
                || name.ends_with(".min.css")
                || name.ends_with(".min.mjs")
                || name.ends_with(".min.cjs")
    )
}

fn is_secret_like(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let lower = name.to_string_lossy().to_ascii_lowercase();

    if lower == ".env" || lower == ".npmrc" {
        return true;
    }

    if lower.starts_with(".env.") {
        return !matches!(
            lower.as_str(),
            ".env.example" | ".env.sample" | ".env.template"
        );
    }

    if lower.starts_with("id_rsa") || lower.starts_with("id_ed25519") {
        return true;
    }

    if lower.starts_with("credentials") || lower.starts_with("secrets") {
        return true;
    }

    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("pem")
            | Some("key")
            | Some("crt")
            | Some("cer")
            | Some("p12")
            | Some("pfx")
            | Some("der")
            | Some("jks")
            | Some("keystore")
    )
}

fn is_binary(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(BINARY_SAMPLE_BYTES)];
    sample.contains(&0) || std::str::from_utf8(sample).is_err()
}

fn clamp_limit(requested: Option<usize>, default: usize, maximum: usize) -> usize {
    requested.unwrap_or(default).min(maximum).max(1)
}

fn read_failure(error: AccessFailure) -> ReadFileOutput {
    let (reason, message) = match error {
        AccessFailure::Blocked(message) => ("blocked", message),
        AccessFailure::NotFound(message) => ("not_found", message),
        AccessFailure::Io(message) => ("io_error", message),
    };

    ReadFileOutput {
        success: false,
        message,
        reason: Some(reason.to_string()),
        path: None,
        size_bytes: None,
        start_line: None,
        end_line: None,
        total_lines: None,
        truncated: false,
        content: None,
    }
}

fn read_invalid_range(message: &str) -> ReadFileOutput {
    ReadFileOutput {
        success: false,
        message: message.to_string(),
        reason: Some("invalid_range".to_string()),
        path: None,
        size_bytes: None,
        start_line: None,
        end_line: None,
        total_lines: None,
        truncated: false,
        content: None,
    }
}

fn access_failure_reason(error: &AccessFailure) -> &'static str {
    match error {
        AccessFailure::Blocked(_) => "blocked",
        AccessFailure::NotFound(_) => "not_found",
        AccessFailure::Io(_) => "io_error",
    }
}

fn search_failure(error: AccessFailure, reason: &str) -> SearchCodeOutput {
    let message = match error {
        AccessFailure::Blocked(message)
        | AccessFailure::NotFound(message)
        | AccessFailure::Io(message) => message,
    };

    SearchCodeOutput {
        success: false,
        message,
        reason: Some(reason.to_string()),
        matches: vec![],
        truncated: false,
        scanned_files: 0,
        skipped_files: 0,
    }
}

fn find_failure(error: AccessFailure, reason: &str) -> FindFilesOutput {
    let message = match error {
        AccessFailure::Blocked(message)
        | AccessFailure::NotFound(message)
        | AccessFailure::Io(message) => message,
    };

    FindFilesOutput {
        success: false,
        message,
        reason: Some(reason.to_string()),
        files: vec![],
        truncated: false,
        scanned_entries: 0,
    }
}

fn list_failure(error: AccessFailure, reason: &str) -> ListDirectoryOutput {
    let message = match error {
        AccessFailure::Blocked(message)
        | AccessFailure::NotFound(message)
        | AccessFailure::Io(message) => message,
    };

    ListDirectoryOutput {
        success: false,
        message,
        reason: Some(reason.to_string()),
        entries: vec![],
        truncated: false,
        visited_entries: 0,
    }
}

struct TruncatedLine {
    text: String,
    truncated: bool,
}

fn truncate_line(line: &str, max_chars: usize) -> TruncatedLine {
    let char_count = line.chars().count();
    if char_count <= max_chars {
        return TruncatedLine {
            text: line.to_string(),
            truncated: false,
        };
    }

    let keep = max_chars.saturating_sub(3);
    let text = line.chars().take(keep).collect::<String>();
    TruncatedLine {
        text: format!("{}...", text),
        truncated: true,
    }
}

struct SearchState<'a> {
    regex: &'a Regex,
    include_matcher: Option<&'a GlobMatcher>,
    matches: Vec<SearchCodeMatch>,
    scanned_files: usize,
    skipped_files: usize,
    scanned_bytes: u64,
    visited_entries: usize,
    truncated: bool,
    max_results: usize,
    scope_match_base: PathBuf,
    scope_is_file: bool,
}

fn search_file(
    scope: &ResolvedPath,
    state: &mut SearchState<'_>,
) -> Result<(), WorkspaceToolError> {
    if let Some(reason) = blocked_path_reason(&scope.relative_path, PathKind::File, true) {
        state.skipped_files += 1;
        let _ = reason;
        return Ok(());
    }

    let metadata = fs::metadata(&scope.absolute_path).map_err(|e| {
        WorkspaceToolError::Internal(format!("Failed to inspect search file: {}", e))
    })?;
    if metadata.len() > SEARCH_FILE_BYTES_CAP {
        state.skipped_files += 1;
        return Ok(());
    }
    if state.scanned_files >= SEARCH_FILE_SCAN_CAP
        || state.scanned_bytes + metadata.len() > SEARCH_TOTAL_BYTES_CAP
    {
        state.truncated = true;
        return Ok(());
    }

    let match_path = match_relative_to_scope(
        &scope.relative_path,
        &state.scope_match_base,
        state.scope_is_file,
    );
    if let Some(matcher) = state.include_matcher {
        if !matcher.is_match(match_path.as_path()) {
            return Ok(());
        }
    }

    let bytes = match fs::read(&scope.absolute_path) {
        Ok(bytes) => bytes,
        Err(_) => {
            state.skipped_files += 1;
            return Ok(());
        }
    };
    if is_binary(&bytes) {
        state.skipped_files += 1;
        return Ok(());
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => {
            state.skipped_files += 1;
            return Ok(());
        }
    };

    state.scanned_files += 1;
    state.scanned_bytes += metadata.len();

    for (index, line) in text.lines().enumerate() {
        if state.regex.is_match(line) {
            state.matches.push(SearchCodeMatch {
                path: display_rel_path(&scope.relative_path),
                line: index + 1,
                text: truncate_line(line, DISPLAY_LINE_CHAR_CAP).text,
            });
            if state.matches.len() >= state.max_results {
                state.truncated = true;
                return Ok(());
            }
        }
    }

    Ok(())
}

fn walk_search_directory(
    access: &WorkspaceAccess,
    directory: &Path,
    state: &mut SearchState<'_>,
) -> Result<(), WorkspaceToolError> {
    for entry in read_sorted_directory_entries(directory)? {
        if state.truncated {
            return Ok(());
        }

        state.visited_entries += 1;
        if state.visited_entries > VISITED_ENTRY_CAP {
            state.truncated = true;
            return Ok(());
        }

        if entry.is_symlink {
            if entry.is_file() {
                state.skipped_files += 1;
            }
            continue;
        }

        let relative_path = entry.relative_path(access);
        let kind = entry.path_kind();
        if let Some(_reason) = blocked_path_reason(&relative_path, kind, true) {
            if kind == PathKind::File {
                state.skipped_files += 1;
            }
            continue;
        }

        if entry.is_directory() {
            if !entry.is_symlink {
                walk_search_directory(access, &entry.absolute_path, state)?;
            }
            continue;
        }

        if entry.is_file() {
            let resolved = ResolvedPath {
                absolute_path: entry.absolute_path.clone(),
                relative_path,
                exists: true,
                is_dir: false,
                is_file: true,
                is_symlink: entry.is_symlink,
            };
            search_file(&resolved, state)?;
        }
    }

    Ok(())
}

struct FindState {
    matcher: GlobMatcher,
    files: Vec<String>,
    visited_entries: usize,
    truncated: bool,
    max_results: usize,
    scope_match_base: PathBuf,
    scope_is_file: bool,
}

fn consider_found_file(scope: &ResolvedPath, state: &mut FindState) {
    if state.truncated {
        return;
    }

    if let Some(_reason) = blocked_path_reason(&scope.relative_path, PathKind::File, true) {
        return;
    }

    let match_path = match_relative_to_scope(
        &scope.relative_path,
        &state.scope_match_base,
        state.scope_is_file,
    );
    if state.matcher.is_match(match_path.as_path()) {
        state.files.push(display_rel_path(&scope.relative_path));
    }
}

fn walk_find_directory(
    access: &WorkspaceAccess,
    directory: &Path,
    state: &mut FindState,
) -> Result<(), WorkspaceToolError> {
    for entry in read_sorted_directory_entries(directory)? {
        if state.truncated {
            return Ok(());
        }

        state.visited_entries += 1;
        if state.visited_entries > VISITED_ENTRY_CAP {
            state.truncated = true;
            return Ok(());
        }

        let relative_path = entry.relative_path(access);
        let kind = entry.path_kind();
        if entry.is_symlink {
            continue;
        }
        if let Some(_reason) = blocked_path_reason(&relative_path, kind, true) {
            continue;
        }

        if entry.is_directory() {
            if !entry.is_symlink {
                walk_find_directory(access, &entry.absolute_path, state)?;
            }
            continue;
        }

        if entry.is_file() {
            let resolved = ResolvedPath {
                absolute_path: entry.absolute_path.clone(),
                relative_path,
                exists: true,
                is_dir: false,
                is_file: true,
                is_symlink: entry.is_symlink,
            };
            consider_found_file(&resolved, state);
        }
    }

    Ok(())
}

struct ListState {
    entries: Vec<DirectoryEntryOutput>,
    visited_entries: usize,
    truncated: bool,
    max_entries: usize,
}

fn walk_list_directory(
    access: &WorkspaceAccess,
    directory: &Path,
    depth: usize,
    state: &mut ListState,
) -> Result<(), WorkspaceToolError> {
    if depth == 0 || state.truncated {
        return Ok(());
    }

    for entry in read_sorted_directory_entries(directory)? {
        if state.truncated {
            return Ok(());
        }

        state.visited_entries += 1;
        if state.visited_entries > VISITED_ENTRY_CAP {
            state.truncated = true;
            return Ok(());
        }

        let relative_path = entry.relative_path(access);
        let kind = entry.path_kind();
        if entry.is_symlink {
            continue;
        }
        if let Some(_reason) = blocked_path_reason(&relative_path, kind, true) {
            continue;
        }

        state.entries.push(DirectoryEntryOutput {
            path: display_rel_path(&relative_path),
            name: entry.name.clone(),
            kind: entry.kind_name(),
            size_bytes: if entry.is_file() {
                Some(entry.size_bytes)
            } else {
                None
            },
        });
        if state.entries.len() >= state.max_entries {
            state.truncated = true;
            return Ok(());
        }

        if entry.is_directory() && !entry.is_symlink {
            walk_list_directory(access, &entry.absolute_path, depth - 1, state)?;
        }
    }

    Ok(())
}

fn match_relative_to_scope(path: &Path, scope_base: &Path, file_scope: bool) -> PathBuf {
    if file_scope {
        path.file_name().map(PathBuf::from).unwrap_or_default()
    } else if scope_base.as_os_str().is_empty() {
        path.to_path_buf()
    } else {
        path.strip_prefix(scope_base)
            .map(PathBuf::from)
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

#[derive(Debug)]
struct DirectoryEntryView {
    absolute_path: PathBuf,
    name: String,
    is_symlink: bool,
    target_is_dir: bool,
    target_is_file: bool,
    size_bytes: u64,
}

impl DirectoryEntryView {
    fn relative_path(&self, access: &WorkspaceAccess) -> PathBuf {
        self.absolute_path
            .strip_prefix(&access.root)
            .map(PathBuf::from)
            .unwrap_or_else(|_| self.absolute_path.clone())
    }

    fn is_directory(&self) -> bool {
        self.target_is_dir
    }

    fn is_file(&self) -> bool {
        self.target_is_file
    }

    fn path_kind(&self) -> PathKind {
        if self.is_symlink {
            PathKind::Symlink
        } else if self.target_is_dir {
            PathKind::Directory
        } else if self.target_is_file {
            PathKind::File
        } else {
            PathKind::Other
        }
    }

    fn kind_name(&self) -> String {
        match self.path_kind() {
            PathKind::Directory => "directory",
            PathKind::File => "file",
            PathKind::Symlink => "symlink",
            PathKind::Other => "other",
        }
        .to_string()
    }
}

fn read_sorted_directory_entries(
    directory: &Path,
) -> Result<Vec<DirectoryEntryView>, WorkspaceToolError> {
    let mut entries = Vec::new();
    let read_dir = fs::read_dir(directory)
        .map_err(|e| WorkspaceToolError::Internal(format!("Failed to read directory: {}", e)))?;

    for entry in read_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let absolute_path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let symlink_metadata = match fs::symlink_metadata(&absolute_path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let metadata = match fs::metadata(&absolute_path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        entries.push(DirectoryEntryView {
            absolute_path,
            name,
            is_symlink: symlink_metadata.file_type().is_symlink(),
            target_is_dir: metadata.is_dir(),
            target_is_file: metadata.is_file(),
            size_bytes: metadata.len(),
        });
    }

    entries.sort_by(|left, right| {
        directory_sort_rank(left)
            .cmp(&directory_sort_rank(right))
            .then_with(|| natural_path_cmp(&left.name, &right.name))
    });

    Ok(entries)
}

fn directory_sort_rank(entry: &DirectoryEntryView) -> u8 {
    match entry.path_kind() {
        PathKind::Directory => 0,
        PathKind::File => 1,
        PathKind::Symlink => 2,
        PathKind::Other => 3,
    }
}

fn natural_path_cmp(left: &str, right: &str) -> Ordering {
    left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::tool::Tool;
    use std::fs;
    use tempfile::tempdir;

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    fn write_file(path: &Path, content: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn truncate_snippet_is_utf8_safe() {
        assert_eq!(truncate_snippet("aé🙂bc", 3), "aé🙂...");
        assert_eq!(truncate_snippet("short", 10), "short");
    }

    #[test]
    fn base_workspace_tools_returns_four_tools() {
        let tools = build_base_workspace_tools(PathBuf::from("/tmp/test-workspace"));
        assert_eq!(tools.len(), 4);
        let names: Vec<String> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"read_file".to_string()));
        assert!(names.contains(&"search_code".to_string()));
        assert!(names.contains(&"find_files".to_string()));
        assert!(names.contains(&"list_directory".to_string()));
    }

    #[tokio::test]
    async fn read_file_supports_relative_and_absolute_paths() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("src/main.rs");
        write_file(&file_path, b"fn main() {}\n");
        let tool = create_read_file_tool(dir.path().to_path_buf());

        let relative = tool
            .call(ReadFileArgs {
                path: "src/main.rs".to_string(),
                start_line: None,
                end_line: None,
            })
            .await
            .unwrap();
        assert!(relative.success);
        assert_eq!(relative.path.as_deref(), Some("src/main.rs"));

        let absolute = tool
            .call(ReadFileArgs {
                path: file_path.display().to_string(),
                start_line: None,
                end_line: None,
            })
            .await
            .unwrap();
        assert!(absolute.success);
        assert_eq!(absolute.path.as_deref(), Some("src/main.rs"));
    }

    #[tokio::test]
    async fn read_file_rejects_invalid_ranges_and_truncates() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("notes.txt");
        let content = (1..=450)
            .map(|line| format!("line {}", line))
            .collect::<Vec<_>>()
            .join("\n");
        write_file(&file_path, content.as_bytes());
        let tool = create_read_file_tool(dir.path().to_path_buf());

        let invalid = tool
            .call(ReadFileArgs {
                path: "notes.txt".to_string(),
                start_line: Some(300),
                end_line: Some(200),
            })
            .await
            .unwrap();
        assert!(!invalid.success);
        assert_eq!(invalid.reason.as_deref(), Some("invalid_range"));

        let truncated = tool
            .call(ReadFileArgs {
                path: "notes.txt".to_string(),
                start_line: Some(1),
                end_line: Some(450),
            })
            .await
            .unwrap();
        assert!(truncated.success);
        assert!(truncated.truncated);
        assert_eq!(truncated.end_line, Some(400));
    }

    #[tokio::test]
    async fn read_file_blocks_binary_and_secret_files() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("image.bin"), &[0, 159, 146, 150]);
        write_file(&dir.path().join(".env"), b"TOKEN=abc\n");
        let tool = create_read_file_tool(dir.path().to_path_buf());

        let binary = tool
            .call(ReadFileArgs {
                path: "image.bin".to_string(),
                start_line: None,
                end_line: None,
            })
            .await
            .unwrap();
        assert!(!binary.success);
        assert_eq!(binary.reason.as_deref(), Some("binary"));

        let secret = tool
            .call(ReadFileArgs {
                path: ".env".to_string(),
                start_line: None,
                end_line: None,
            })
            .await
            .unwrap();
        assert!(!secret.success);
        assert_eq!(secret.reason.as_deref(), Some("blocked"));
    }

    #[test]
    fn hidden_allowlist_includes_directory_itself() {
        for path in [
            ".github",
            ".vscode",
            ".devcontainer",
            ".cargo",
            ".agents",
            ".opencode",
            ".gospel",
        ] {
            assert!(
                blocked_path_reason(Path::new(path), PathKind::Directory, true).is_none(),
                "{path} should be allowed"
            );
        }
    }

    #[test]
    fn hidden_allowlist_includes_config_files_at_any_depth() {
        for path in [
            ".gitignore",
            "src/.gitignore",
            "packages/app/.editorconfig",
            "examples/.env.example",
            "tools/.nvmrc",
        ] {
            assert!(
                blocked_path_reason(Path::new(path), PathKind::File, false).is_none(),
                "{path} should be allowed"
            );
        }

        assert!(
            blocked_path_reason(Path::new("src/.hidden/.gitignore"), PathKind::File, false,)
                .is_some()
        );
    }

    #[test]
    fn truncate_text_bytes_reserves_space_for_suffix() {
        let (truncated, did_truncate) = truncate_text_bytes("abcdefghijklmnopqrstuv", 20);

        assert!(did_truncate);
        assert_eq!(truncated.len(), 20);
        assert_eq!(truncated, "abcdefg\n\n[truncated]");
    }

    #[test]
    fn source_edit_diff_preview_reports_line_truncation() {
        let old_text = "a".repeat(DISPLAY_LINE_CHAR_CAP + 1);
        let (preview, did_truncate) = source_edit_diff_preview(
            "src/main.rs",
            &old_text,
            0,
            old_text.len(),
            &old_text,
            "short",
        );

        assert!(did_truncate);
        assert!(preview.contains("..."));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn read_file_blocks_symlink_escape() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("secret.txt");
        write_file(&outside_file, b"nope\n");
        symlink(&outside_file, dir.path().join("linked.txt")).unwrap();
        let tool = create_read_file_tool(dir.path().to_path_buf());

        let output = tool
            .call(ReadFileArgs {
                path: "linked.txt".to_string(),
                start_line: None,
                end_line: None,
            })
            .await
            .unwrap();

        assert!(!output.success);
        assert_eq!(output.reason.as_deref(), Some("blocked"));
    }

    #[tokio::test]
    async fn source_edit_applies_one_exact_replacement() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("src/main.rs");
        write_file(&file_path, b"fn main() {\n    println!(\"old\");\n}\n");
        let tool = create_source_edit_tool(dir.path().to_path_buf());

        let output = tool
            .call(SourceEditArgs {
                path: "src/main.rs".to_string(),
                old_text: "println!(\"old\")".to_string(),
                new_text: "println!(\"new\")".to_string(),
            })
            .await
            .unwrap();

        assert!(output.success);
        assert!(output.changed);
        assert_eq!(output.replacements, 1);
        assert_eq!(output.path.as_deref(), Some("src/main.rs"));
        assert_eq!(
            fs::read_to_string(&file_path).unwrap(),
            "fn main() {\n    println!(\"new\");\n}\n"
        );
        let preview = output.diff_preview.unwrap();
        assert!(preview.contains("-println!(\"old\")"));
        assert!(preview.contains("+println!(\"new\")"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn source_edit_does_not_follow_preexisting_temp_symlink() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let file_path = dir.path().join("src/main.rs");
        let outside_file = outside.path().join("outside.txt");
        write_file(&file_path, b"old\n");
        write_file(&outside_file, b"outside\n");

        let parent = file_path.parent().unwrap();
        for count in 0..64 {
            let temp_name = format!(".main.rs.tmp.{}.{}", std::process::id(), count);
            symlink(&outside_file, parent.join(temp_name)).unwrap();
        }

        let tool = create_source_edit_tool(dir.path().to_path_buf());
        let output = tool
            .call(SourceEditArgs {
                path: "src/main.rs".to_string(),
                old_text: "old".to_string(),
                new_text: "new".to_string(),
            })
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "new\n");
        assert!(!fs::symlink_metadata(&file_path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(&outside_file).unwrap(), "outside\n");
    }

    #[tokio::test]
    async fn source_edit_rejects_no_match_ambiguous_match_and_no_op() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("src/lib.rs"), b"old\nold\n");
        let tool = create_source_edit_tool(dir.path().to_path_buf());

        let no_match = tool
            .call(SourceEditArgs {
                path: "src/lib.rs".to_string(),
                old_text: "missing".to_string(),
                new_text: "new".to_string(),
            })
            .await
            .unwrap();
        assert!(!no_match.success);
        assert_eq!(no_match.reason.as_deref(), Some("no_match"));

        let ambiguous = tool
            .call(SourceEditArgs {
                path: "src/lib.rs".to_string(),
                old_text: "old".to_string(),
                new_text: "new".to_string(),
            })
            .await
            .unwrap();
        assert!(!ambiguous.success);
        assert_eq!(ambiguous.reason.as_deref(), Some("ambiguous_match"));

        let no_op = tool
            .call(SourceEditArgs {
                path: "src/lib.rs".to_string(),
                old_text: "old".to_string(),
                new_text: "old".to_string(),
            })
            .await
            .unwrap();
        assert!(!no_op.success);
        assert_eq!(no_op.reason.as_deref(), Some("no_op"));
    }

    #[tokio::test]
    async fn source_edit_rejects_blocked_targets() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join(".gospel/PLAN.md"), b"plan\n");
        write_file(&dir.path().join(".github/workflows/ci.yml"), b"name: ci\n");
        write_file(&dir.path().join("target/out.rs"), b"old\n");
        write_file(&dir.path().join("Cargo.lock"), b"old\n");
        write_file(&dir.path().join("src/app.generated.ts"), b"old\n");
        write_file(&dir.path().join(".env"), b"TOKEN=old\n");
        let tool = create_source_edit_tool(dir.path().to_path_buf());

        for path in [
            ".gospel/PLAN.md",
            ".github/workflows/ci.yml",
            "target/out.rs",
            "Cargo.lock",
            "src/app.generated.ts",
            ".env",
        ] {
            let output = tool
                .call(SourceEditArgs {
                    path: path.to_string(),
                    old_text: "old".to_string(),
                    new_text: "new".to_string(),
                })
                .await
                .unwrap();
            assert!(!output.success, "{path} should be blocked");
            assert_eq!(output.reason.as_deref(), Some("blocked"));
        }
    }

    #[tokio::test]
    async fn source_edit_rejects_binary_invalid_utf8_and_oversized_files() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("binary.dat"), b"old\0value");
        write_file(&dir.path().join("invalid.txt"), &[0xff, 0xfe]);
        write_file(
            &dir.path().join("large.txt"),
            &vec![b'a'; SOURCE_EDIT_FILE_BYTES_CAP as usize + 1],
        );
        let tool = create_source_edit_tool(dir.path().to_path_buf());

        let binary = tool
            .call(SourceEditArgs {
                path: "binary.dat".to_string(),
                old_text: "old".to_string(),
                new_text: "new".to_string(),
            })
            .await
            .unwrap();
        assert!(!binary.success);
        assert_eq!(binary.reason.as_deref(), Some("binary"));

        let invalid = tool
            .call(SourceEditArgs {
                path: "invalid.txt".to_string(),
                old_text: "old".to_string(),
                new_text: "new".to_string(),
            })
            .await
            .unwrap();
        assert!(!invalid.success);
        assert_eq!(invalid.reason.as_deref(), Some("invalid_utf8"));

        let oversized = tool
            .call(SourceEditArgs {
                path: "large.txt".to_string(),
                old_text: "a".to_string(),
                new_text: "b".to_string(),
            })
            .await
            .unwrap();
        assert!(!oversized.success);
        assert_eq!(oversized.reason.as_deref(), Some("oversized"));
    }

    #[tokio::test]
    async fn search_code_handles_invalid_regex_and_safe_scopes() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("src/lib.rs"), b"pub fn target() {}\n");
        write_file(&dir.path().join("target/build.log"), b"target\n");
        let tool = create_search_code_tool(dir.path().to_path_buf());

        let invalid = tool
            .call(SearchCodeArgs {
                pattern: "(".to_string(),
                path: None,
                include_glob: None,
                max_results: None,
            })
            .await
            .unwrap();
        assert!(!invalid.success);
        assert_eq!(invalid.reason.as_deref(), Some("invalid_regex"));

        let output = tool
            .call(SearchCodeArgs {
                pattern: "target".to_string(),
                path: None,
                include_glob: Some("src/**/*.rs".to_string()),
                max_results: None,
            })
            .await
            .unwrap();
        assert!(output.success);
        assert_eq!(output.matches.len(), 1);
        assert_eq!(output.matches[0].path, "src/lib.rs");
    }

    #[test]
    fn search_file_preserves_existing_truncation_when_blocked() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join(".env");
        write_file(&file_path, b"TOKEN=abc\n");
        let regex = Regex::new("TOKEN").unwrap();
        let scope = ResolvedPath {
            absolute_path: file_path,
            relative_path: PathBuf::from(".env"),
            exists: true,
            is_dir: false,
            is_file: true,
            is_symlink: false,
        };
        let mut state = SearchState {
            regex: &regex,
            include_matcher: None,
            matches: Vec::new(),
            scanned_files: 0,
            skipped_files: 0,
            scanned_bytes: 0,
            visited_entries: 0,
            truncated: true,
            max_results: 10,
            scope_match_base: PathBuf::new(),
            scope_is_file: true,
        };

        search_file(&scope, &mut state).unwrap();

        assert!(state.truncated);
        assert_eq!(state.skipped_files, 1);
        assert_eq!(state.scanned_files, 0);
    }

    #[tokio::test]
    async fn find_files_respects_glob_and_result_caps() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("src/a.rs"), b"");
        write_file(&dir.path().join("src/b.ts"), b"");
        write_file(&dir.path().join("src/nested/c.rs"), b"");
        let tool = create_find_files_tool(dir.path().to_path_buf());

        let output = tool
            .call(FindFilesArgs {
                glob: "**/*.rs".to_string(),
                path: Some("src".to_string()),
                max_results: Some(1),
            })
            .await
            .unwrap();
        assert!(output.success);
        assert_eq!(output.files.len(), 1);
        assert!(output.truncated);
        assert_eq!(output.files[0], "src/a.rs");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn list_directory_sorts_and_skips_symlink_recursion() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("src/main.rs"), b"");
        write_file(&dir.path().join("README.md"), b"");
        let other = tempdir().unwrap();
        fs::create_dir_all(other.path().join("deep")).unwrap();
        symlink(other.path(), dir.path().join("linked-dir")).unwrap();

        let tool = create_list_directory_tool(dir.path().to_path_buf());
        let output = tool
            .call(ListDirectoryArgs {
                path: None,
                depth: Some(3),
                max_entries: None,
            })
            .await
            .unwrap();
        assert!(output.success);
        assert_eq!(output.entries[0].path, "src");
        assert_eq!(output.entries[0].kind, "directory");
        assert!(output
            .entries
            .iter()
            .all(|entry| entry.path != "linked-dir/deep"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn list_directory_uses_canonical_paths_for_symlink_ancestor() {
        let dir = tempdir().unwrap();
        write_file(&dir.path().join("real/src/main.rs"), b"");
        symlink(dir.path().join("real"), dir.path().join("alias")).unwrap();

        let tool = create_list_directory_tool(dir.path().to_path_buf());
        let output = tool
            .call(ListDirectoryArgs {
                path: Some("alias/src".to_string()),
                depth: Some(1),
                max_entries: None,
            })
            .await
            .unwrap();

        assert!(output.success);
        assert!(output
            .entries
            .iter()
            .any(|entry| entry.path == "real/src/main.rs"));
        assert!(output
            .entries
            .iter()
            .all(|entry| entry.path != "alias/src/main.rs"));
    }

    #[tokio::test]
    async fn write_harness_file_creates_file_and_directories() {
        let dir = tempdir().unwrap();
        let tool = create_write_harness_file_tool(dir.path().to_path_buf());

        let output = tool
            .call(WriteHarnessFileArgs {
                path: ".gospel/PLAN.md".to_string(),
                content: "# Plan\n\n## Goal\nTest.\n".to_string(),
            })
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(output.path.as_deref(), Some(".gospel/PLAN.md"));
        assert_eq!(output.size_bytes, Some(22));
        let written = fs::read_to_string(dir.path().join(".gospel/PLAN.md")).unwrap();
        assert_eq!(written, "# Plan\n\n## Goal\nTest.\n");
    }

    #[tokio::test]
    async fn write_harness_file_rejects_path_outside_gospel() {
        let dir = tempdir().unwrap();
        let tool = create_write_harness_file_tool(dir.path().to_path_buf());

        let output = tool
            .call(WriteHarnessFileArgs {
                path: "src/main.rs".to_string(),
                content: "fn main() {}".to_string(),
            })
            .await
            .unwrap();

        assert!(!output.success);
        assert_eq!(output.reason.as_deref(), Some("blocked"));
    }

    #[tokio::test]
    async fn write_harness_file_rejects_oversized_content() {
        let dir = tempdir().unwrap();
        let tool = create_write_harness_file_tool(dir.path().to_path_buf());
        let big_content = "x".repeat(1024 * 1024 + 1);

        let output = tool
            .call(WriteHarnessFileArgs {
                path: ".gospel/PLAN.md".to_string(),
                content: big_content,
            })
            .await
            .unwrap();

        assert!(!output.success);
        assert_eq!(output.reason.as_deref(), Some("oversized"));
    }

    #[tokio::test]
    async fn write_harness_file_rejects_empty_path() {
        let dir = tempdir().unwrap();
        let tool = create_write_harness_file_tool(dir.path().to_path_buf());

        let output = tool
            .call(WriteHarnessFileArgs {
                path: "".to_string(),
                content: "content".to_string(),
            })
            .await
            .unwrap();

        assert!(!output.success);
        assert_eq!(output.reason.as_deref(), Some("blocked"));
    }

    #[tokio::test]
    async fn write_harness_file_allows_nested_paths_under_gospel() {
        let dir = tempdir().unwrap();
        let tool = create_write_harness_file_tool(dir.path().to_path_buf());

        let output = tool
            .call(WriteHarnessFileArgs {
                path: ".gospel/notes/design.md".to_string(),
                content: "# Design notes".to_string(),
            })
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(output.path.as_deref(), Some(".gospel/notes/design.md"));
        let written = fs::read_to_string(dir.path().join(".gospel/notes/design.md")).unwrap();
        assert_eq!(written, "# Design notes");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_harness_file_rejects_symlink_escape() {
        let dir = tempdir().unwrap();
        let gospel_dir = dir.path().join(".gospel");
        fs::create_dir_all(&gospel_dir).unwrap();
        symlink(dir.path(), gospel_dir.join("escape")).unwrap();
        let tool = create_write_harness_file_tool(dir.path().to_path_buf());

        let output = tool
            .call(WriteHarnessFileArgs {
                path: ".gospel/escape/src/pwned.rs".to_string(),
                content: "fn main() {}".to_string(),
            })
            .await
            .unwrap();

        assert!(!output.success);
        assert_eq!(output.reason.as_deref(), Some("blocked"));
        assert!(!dir.path().join("src").exists());
        assert!(!dir.path().join("src/pwned.rs").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_harness_file_rejects_corpus_symlink_alias() {
        let dir = tempdir().unwrap();
        let corpus_dir = dir.path().join(".gospel/corpus");
        fs::create_dir_all(&corpus_dir).unwrap();
        symlink(&corpus_dir, dir.path().join(".gospel/alias")).unwrap();
        let tool = create_write_harness_file_tool(dir.path().to_path_buf());

        let output = tool
            .call(WriteHarnessFileArgs {
                path: ".gospel/alias/manifest.json".to_string(),
                content: "{}".to_string(),
            })
            .await
            .unwrap();

        assert!(!output.success);
        assert_eq!(output.reason.as_deref(), Some("blocked"));
        assert!(!corpus_dir.join("manifest.json").exists());
    }
}
