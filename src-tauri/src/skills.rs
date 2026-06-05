use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use tracing;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub argument_hint: Option<String>,
    #[serde(default)]
    pub user_invocable: Option<bool>,
    #[serde(default)]
    pub disable_model_invocation: Option<bool>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub license: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub body: String,
    pub argument_hint: Option<String>,
    pub user_invocable: bool,
    pub disable_model_invocation: bool,
    pub allowed_tools: Vec<String>,
    pub timeout_seconds: Option<u64>,
    pub license: Option<String>,
    pub scripts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillSource {
    Workspace,
    Global,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub scripts: Vec<String>,
    pub user_invocable: bool,
    pub argument_hint: Option<String>,
}

impl From<&Skill> for SkillSummary {
    fn from(skill: &Skill) -> Self {
        SkillSummary {
            name: skill.name.clone(),
            description: skill.description.clone(),
            source: skill.source.clone(),
            scripts: skill.scripts.clone(),
            user_invocable: skill.user_invocable,
            argument_hint: skill.argument_hint.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RunSkillScriptArgs {
    pub skill: String,
    pub script: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSkillScriptTool {
    pub available_skills: Vec<Skill>,
    pub workspace_path: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct RunSkillScriptOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub truncated: bool,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct RunSkillScriptError(String);

impl std::fmt::Display for RunSkillScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for RunSkillScriptError {}

impl Tool for RunSkillScriptTool {
    const NAME: &'static str = "run_skill_script";

    type Error = RunSkillScriptError;
    type Args = RunSkillScriptArgs;
    type Output = RunSkillScriptOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        let scriptable: Vec<String> = self
            .available_skills
            .iter()
            .filter(|s| !s.scripts.is_empty())
            .map(|s| {
                let scripts = s.scripts.join(", ");
                format!("{}: [{}]", s.name, scripts)
            })
            .collect();

        let scripts_desc = if scriptable.is_empty() {
            "No scriptable skills are currently active.".to_string()
        } else {
            format!(
                "The following skills expose scripts you may run: {}. Pass the exact skill name and exact script filename.",
                scriptable.join("; ")
            )
        };

        ToolDefinition {
            name: Self::NAME.to_string(),
            description: format!(
                "Run an executable script bundled with a skill (from its scripts/ dir). {}. The script runs in the workspace dir if available, with per-skill timeout and 16KiB output caps. Returns captured stdout, stderr, exit code and truncation flag. Only use when the active/invoked skill's instructions explicitly direct you to execute one of its scripts.",
                scripts_desc
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "skill": {
                        "type": "string",
                        "description": "Exact name of the skill that owns the script (must be one that lists scripts)."
                    },
                    "script": {
                        "type": "string",
                        "description": "Exact filename of the script to execute (e.g. 'hello', 'run.sh'). Must be listed for that skill."
                    }
                },
                "required": ["skill", "script"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let skill = match self.available_skills.iter().find(|s| s.name == args.skill) {
            Some(s) => s,
            None => {
                return Ok(RunSkillScriptOutput {
                    success: false,
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: -1,
                    truncated: false,
                    error: Some(format!("Unknown skill '{}'", args.skill)),
                });
            }
        };

        if !skill.scripts.contains(&args.script) {
            return Ok(RunSkillScriptOutput {
                success: false,
                stdout: String::new(),
                stderr: String::new(),
                exit_code: -1,
                truncated: false,
                error: Some(format!(
                    "Script '{}' not found for skill '{}'. Available: {}",
                    args.script,
                    args.skill,
                    skill.scripts.join(", ")
                )),
            });
        }

        match run_skill_script(skill, &args.script, self.workspace_path.as_deref()).await {
            Ok(res) => Ok(RunSkillScriptOutput {
                success: true,
                stdout: res.stdout,
                stderr: res.stderr,
                exit_code: res.exit_code,
                truncated: res.truncated,
                error: None,
            }),
            Err(e) => Ok(RunSkillScriptOutput {
                success: false,
                stdout: String::new(),
                stderr: e.clone(),
                exit_code: -1,
                truncated: false,
                error: Some(e),
            }),
        }
    }
}

fn normalize_crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn parse_skill_file(
    skill_dir: &Path,
    source: SkillSource,
    skills_root: &Path,
) -> Result<Option<Skill>, String> {
    let skill_md_path = skill_dir.join("SKILL.md");
    if !skill_md_path.exists() {
        return Ok(None);
    }

    let canonical_skills_root = match fs::canonicalize(skills_root) {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!(
                "Failed to canonicalize skills root {}: {}",
                skills_root.display(),
                e
            );
            return Ok(None);
        }
    };

    let canonical_skill_dir = match fs::canonicalize(skill_dir) {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!(
                "Failed to canonicalize skill dir {}: {}",
                skill_dir.display(),
                e
            );
            return Ok(None);
        }
    };

    if !canonical_skill_dir.starts_with(&canonical_skills_root) {
        tracing::warn!(
            "Symlink escape detected: skill dir {} is outside skills root {}",
            canonical_skill_dir.display(),
            canonical_skills_root.display()
        );
        return Ok(None);
    }

    let raw = match fs::read_to_string(&skill_md_path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            tracing::warn!(
                "Permission denied reading {}: {}",
                skill_md_path.display(),
                e
            );
            return Ok(None);
        }
        Err(e) => {
            tracing::warn!(
                "Failed to read {}: {}",
                skill_md_path.display(),
                e
            );
            return Ok(None);
        }
    };

    let content = normalize_crlf(&raw);

    let content = content.strip_prefix("---\n").or_else(|| content.strip_prefix("---\r\n")).unwrap_or(content.as_str());
    let parts: Vec<&str> = content.splitn(2, "\n---\n").collect();
    if parts.len() < 2 {
        tracing::warn!(
            "Missing frontmatter in {}: expected closing `---` delimiter",
            skill_md_path.display()
        );
        return Ok(None);
    }

    let frontmatter_str = parts[0].trim();
    if frontmatter_str.is_empty() {
        tracing::warn!(
            "Empty frontmatter in {}",
            skill_md_path.display()
        );
        return Ok(None);
    }

    let frontmatter: SkillFrontmatter = match serde_yaml::from_str(frontmatter_str) {
        Ok(fm) => fm,
        Err(e) => {
            tracing::warn!(
                "YAML parse error in {}: {}",
                skill_md_path.display(),
                e
            );
            return Ok(None);
        }
    };

    let folder_name = skill_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    if frontmatter.name != folder_name {
        tracing::warn!(
            "Skill name '{}' does not match folder '{}' in {}; skipping",
            frontmatter.name,
            folder_name,
            skill_dir.display()
        );
        return Ok(None);
    }

    let canonical_body_path = match fs::canonicalize(&skill_md_path) {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!(
                "Failed to canonicalize {}: {}",
                skill_md_path.display(),
                e
            );
            return Ok(None);
        }
    };

    if !canonical_body_path.starts_with(&canonical_skill_dir) {
        tracing::warn!(
            "Symlink escape detected for {}: body path {} is outside skill dir {}",
            skill_md_path.display(),
            canonical_body_path.display(),
            canonical_skill_dir.display()
        );
        return Ok(None);
    }

    let body = parts[1].trim().to_string();

    let scripts = discover_scripts(&canonical_skill_dir);

    Ok(Some(Skill {
        name: frontmatter.name,
        description: frontmatter.description,
        source,
        body,
        argument_hint: frontmatter.argument_hint,
        user_invocable: frontmatter.user_invocable.unwrap_or(true),
        disable_model_invocation: frontmatter.disable_model_invocation.unwrap_or(false),
        allowed_tools: frontmatter.allowed_tools.unwrap_or_default(),
        timeout_seconds: frontmatter.timeout_seconds,
        license: frontmatter.license,
        scripts,
    }))
}

fn discover_scripts(skill_dir: &Path) -> Vec<String> {
    let scripts_dir = skill_dir.join("scripts");
    if !scripts_dir.is_dir() {
        return Vec::new();
    }

    let entries = match fs::read_dir(&scripts_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut scripts = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) {
                scripts.push(name);
            }
        }
    }

    scripts.sort();
    scripts
}

pub fn discover_skills(workspace_path: Option<&Path>, global_skills_dir: Option<&Path>) -> Vec<Skill> {
    let mut skills = Vec::new();
    let mut seen_names: Vec<String> = Vec::new();

    if let Some(workspace) = workspace_path {
        let workspace_skills_dir = workspace.join(".agents").join("skills");
        if workspace_skills_dir.is_dir() {
            match discover_skills_in_dir(&workspace_skills_dir, SkillSource::Workspace) {
                Ok(workspace_skills) => {
                    for skill in workspace_skills {
                        if seen_names.contains(&skill.name) {
                            tracing::warn!(
                                "Duplicate skill name '{}'; workspace wins over global",
                                skill.name
                            );
                            continue;
                        }
                        seen_names.push(skill.name.clone());
                        skills.push(skill);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to scan workspace skills: {}", e);
                }
            }
        }
    }

    if let Some(global_dir) = global_skills_dir {
        if global_dir.is_dir() {
            match discover_skills_in_dir(global_dir, SkillSource::Global) {
                Ok(global_skills) => {
                    for skill in global_skills {
                        if seen_names.contains(&skill.name) {
                            continue;
                        }
                        seen_names.push(skill.name.clone());
                        skills.push(skill);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to scan global skills: {}", e);
                }
            }
        }
    }

    skills
}

fn discover_skills_in_dir(skills_dir: &Path, source: SkillSource) -> Result<Vec<Skill>, String> {
    let entries = match fs::read_dir(skills_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            tracing::warn!("Permission denied reading skills dir: {}", skills_dir.display());
            return Ok(Vec::new());
        }
        Err(e) => return Err(format!("Failed to read skills dir: {}", e)),
    };

    let mut skills = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                tracing::warn!("Failed to read entry in {}: {}", skills_dir.display(), e);
                continue;
            }
        };

        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        match parse_skill_file(&path, source.clone(), skills_dir) {
            Ok(Some(skill)) => skills.push(skill),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("Error parsing skill at {}: {}", path.display(), e);
            }
        }
    }

    Ok(skills)
}

pub fn global_skills_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("gospel").join("skills"))
}

pub fn list_skill_summaries(
    workspace_path: Option<&Path>,
    global_dir: Option<&Path>,
) -> Vec<SkillSummary> {
    discover_skills(workspace_path, global_dir)
        .iter()
        .map(SkillSummary::from)
        .collect()
}

static STOPWORDS: once_cell::sync::Lazy<Vec<String>> = once_cell::sync::Lazy::new(|| {
    let data = include_str!("skills/stopwords.json");
    serde_json::from_str(data).unwrap_or_default()
});

const MATCH_SCORE_THRESHOLD: f64 = 0.1;
const MATCH_TOP_N: usize = 3;

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty() && token.len() > 1)
        .filter(|token| !STOPWORDS.contains(&token.to_string()))
        .map(|s| s.to_string())
        .collect()
}

fn token_overlap_score(query_tokens: &[String], skill_tokens: &[String]) -> f64 {
    if query_tokens.is_empty() || skill_tokens.is_empty() {
        return 0.0;
    }

    let skill_set: std::collections::HashSet<&String> = skill_tokens.iter().collect();
    let matched = query_tokens.iter().filter(|t| skill_set.contains(t)).count();

    matched as f64 / query_tokens.len() as f64
}

#[derive(Debug, Clone)]
pub struct SkillMatch {
    pub skill: Skill,
    pub score: f64,
}

pub fn match_skills(prompt: &str, skills: &[Skill]) -> Vec<SkillMatch> {
    let query_tokens = tokenize(prompt);
    if query_tokens.is_empty() {
        return Vec::new();
    }

    let mut matches: Vec<SkillMatch> = skills
        .iter()
        .filter(|skill| !skill.disable_model_invocation)
        .map(|skill| {
            let skill_text = format!("{} {}", skill.name, skill.description);
            let skill_tokens = tokenize(&skill_text);
            let score = token_overlap_score(&query_tokens, &skill_tokens);
            SkillMatch {
                skill: skill.clone(),
                score,
            }
        })
        .filter(|m| m.score >= MATCH_SCORE_THRESHOLD)
        .collect();

    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let a_ws = a.skill.source == SkillSource::Workspace;
                let b_ws = b.skill.source == SkillSource::Workspace;
                b_ws.cmp(&a_ws)
            })
    });

    matches.truncate(MATCH_TOP_N);
    matches
}

pub fn format_skills_preamble_section(matches: &[SkillMatch]) -> Option<String> {
    if matches.is_empty() {
        return None;
    }

    let mut section = String::from("## Active Skills\n\n");
    for m in matches {
        section.push_str(&format!(
            "- **{}**: {}",
            m.skill.name, m.skill.description
        ));
        section.push('\n');
    }

    Some(section)
}

pub fn format_invoked_skill_preamble(skill: &Skill) -> String {
    format!(
        "## Invoked Skill: {}\n\n{}",
        skill.name, skill.body
    )
}

const DEFAULT_SCRIPT_TIMEOUT: u64 = 30;
const SCRIPT_OUTPUT_CAP: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ScriptResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub truncated: bool,
}

fn detect_interpreter(script_path: &Path) -> Result<String, String> {
    let content = match fs::read_to_string(script_path) {
        Ok(c) => c,
        Err(e) => return Err(format!("Failed to read script: {}", e)),
    };

    let first_line = content.lines().next().unwrap_or("");
    if first_line.starts_with("#!") {
        let shebang = first_line[2..].trim().to_string();
        if !shebang.is_empty() {
            let tokens: Vec<&str> = shebang.split_whitespace().collect();
            let interpreter = if tokens[0].ends_with("/env") || tokens[0] == "env" {
                tokens[1..].join(" ")
            } else {
                tokens.join(" ")
            };
            return Ok(interpreter);
        }
    }

    match script_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("mjs") | Some("js") => Ok("node".to_string()),
        Some("sh") => Ok("bash".to_string()),
        _ => Err(format!(
            "No shebang and unrecognized extension for script: {}",
            script_path.display()
        )),
    }
}

pub async fn run_skill_script(
    skill: &Skill,
    script_name: &str,
    workspace_path: Option<&Path>,
) -> Result<ScriptResult, String> {
    let skill_dir = {
        let base = if skill.source == SkillSource::Workspace {
            workspace_path
                .ok_or("Workspace path is required for workspace skills")?
                .join(".agents")
                .join("skills")
                .join(&skill.name)
        } else {
            global_skills_dir()
                .ok_or("Global skills directory is not available")?
                .join(&skill.name)
        };
        match fs::canonicalize(&base) {
            Ok(p) => p,
            Err(e) => return Err(format!("Failed to resolve skill directory: {}", e)),
        }
    };

    let scripts_dir = skill_dir.join("scripts");
    let script_path = scripts_dir.join(script_name);

    let canonical_script = match fs::canonicalize(&script_path) {
        Ok(p) => p,
        Err(e) => return Err(format!("Script not found '{}': {}", script_name, e)),
    };

    let canonical_scripts_dir = match fs::canonicalize(&scripts_dir) {
        Ok(p) => p,
        Err(e) => return Err(format!("Failed to resolve scripts directory: {}", e)),
    };

    if !canonical_script.starts_with(&canonical_scripts_dir) {
        return Err(format!(
            "Script '{}' escapes the skill directory",
            script_name
        ));
    }

    if !canonical_script.is_file() {
        return Err(format!("'{}' is not a file", script_name));
    }

    let interpreter = detect_interpreter(&canonical_script)?;
    let interpreter_parts: Vec<&str> = interpreter.split_whitespace().collect();

    let timeout_secs = skill.timeout_seconds.unwrap_or(DEFAULT_SCRIPT_TIMEOUT);

    let mut cmd = tokio::process::Command::new(interpreter_parts[0]);
    for arg in &interpreter_parts[1..] {
        cmd.arg(arg);
    }
    cmd.kill_on_drop(true);
    cmd.arg(&canonical_script);

    if let Some(ws) = workspace_path {
        cmd.current_dir(ws);
    }

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn script '{}': {}", script_name, e))?;

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        child.wait_with_output(),
    )
    .await;

    let output = match result {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => return Err(format!("Script execution failed: {}", e)),
        Err(_) => {
            return Err(format!(
                "Script '{}' timed out after {} seconds",
                script_name, timeout_secs
            ));
        }
    };

    let (stdout, stdout_truncated) = truncate_bytes_to_string(&output.stdout, SCRIPT_OUTPUT_CAP);
    let (stderr, stderr_truncated) = truncate_bytes_to_string(&output.stderr, SCRIPT_OUTPUT_CAP);

    Ok(ScriptResult {
        stdout,
        stderr,
        exit_code: output.status.code().unwrap_or(-1),
        truncated: stdout_truncated || stderr_truncated,
    })
}

fn truncate_bytes_to_string(bytes: &[u8], max: usize) -> (String, bool) {
    if bytes.len() <= max {
        return (String::from_utf8_lossy(bytes).into_owned(), false);
    }

    let truncated = &bytes[..max];
    let suffix = "\n\n[truncated]";
    let mut result = String::from_utf8_lossy(truncated).into_owned();
    result.push_str(suffix);
    (result, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_skill(
        base: &Path,
        folder: &str,
        name: &str,
        description: &str,
        body: &str,
    ) -> PathBuf {
        let skill_dir = base.join(folder);
        fs::create_dir_all(&skill_dir).unwrap();
        let content = format!(
            "---\nname: {}\ndescription: {}\n---\n\n{}",
            name, description, body
        );
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();
        skill_dir
    }

    fn write_skill_with_crlf(
        base: &Path,
        folder: &str,
        name: &str,
        description: &str,
        body: &str,
    ) -> PathBuf {
        let skill_dir = base.join(folder);
        fs::create_dir_all(&skill_dir).unwrap();
        let content = format!(
            "---\r\nname: {}\r\ndescription: {}\r\n---\r\n\r\n{}",
            name, description, body
        );
        fs::write(skill_dir.join("SKILL.md"), content.as_bytes()).unwrap();
        skill_dir
    }

    #[test]
    fn discovers_workspace_skills() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills");
        write_skill(&skills_dir, "tdd", "tdd", "Test driven development", "# TDD");
        write_skill(
            &skills_dir,
            "diagnose",
            "diagnose",
            "Diagnose bugs",
            "# Diagnose",
        );

        let skills = discover_skills(Some(dir.path()), None);
        assert_eq!(skills.len(), 2);
        assert!(skills.iter().any(|s| s.name == "tdd"));
        assert!(skills.iter().any(|s| s.name == "diagnose"));
    }

    #[test]
    fn discovers_global_skills() {
        let workspace = tempdir().unwrap();
        let global = tempdir().unwrap();
        write_skill(
            global.path(),
            "global-skill",
            "global-skill",
            "A global skill",
            "# Global",
        );

        let skills = discover_skills(Some(workspace.path()), Some(global.path()));
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "global-skill");
        assert_eq!(skills[0].source, SkillSource::Global);
    }

    #[test]
    fn workspace_wins_over_global_at_equal_score() {
        let workspace = tempdir().unwrap();
        let global = tempdir().unwrap();
        let ws_skills = workspace.path().join(".agents").join("skills");
        write_skill(
            &ws_skills,
            "shared",
            "shared",
            "Workspace version",
            "# WS",
        );
        write_skill(global.path(), "shared", "shared", "Global version", "# GL");

        let skills = discover_skills(Some(workspace.path()), Some(global.path()));
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].source, SkillSource::Workspace);
    }

    #[test]
    fn rejects_name_folder_mismatch() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills");
        write_skill(
            &skills_dir,
            "my-folder",
            "different-name",
            "Mismatched",
            "# Body",
        );

        let skills = discover_skills(Some(dir.path()), None);
        assert_eq!(skills.len(), 0);
    }

    #[test]
    #[cfg(unix)]
    fn rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let outside = tempdir().unwrap();
        let outside_skill = outside.path().join("escape-skill");
        fs::create_dir_all(&outside_skill).unwrap();
        fs::write(
            outside_skill.join("SKILL.md"),
            "---\nname: escape-skill\ndescription: Escape\n---\n\nbody",
        )
        .unwrap();

        let link = skills_dir.join("escape-skill");
        symlink(&outside_skill, &link).unwrap();

        let canonical_link = fs::canonicalize(&link).unwrap();
        let canonical_skills_dir = fs::canonicalize(&skills_dir).unwrap();
        assert!(
            !canonical_link.starts_with(&canonical_skills_dir),
            "test setup: symlink should escape skills dir"
        );

        let skills = discover_skills(Some(dir.path()), None);
        assert_eq!(skills.len(), 0);
    }

    #[test]
    fn normalizes_crlf_before_parsing() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills");
        write_skill_with_crlf(
            &skills_dir,
            "crlf-skill",
            "crlf-skill",
            "CRLF test",
            "# Body",
        );

        let skills = discover_skills(Some(dir.path()), None);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].body, "# Body");
    }

    #[test]
    fn skips_missing_frontmatter() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills");
        let skill_dir = skills_dir.join("no-fm");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "Just a body, no frontmatter").unwrap();

        let skills = discover_skills(Some(dir.path()), None);
        assert_eq!(skills.len(), 0);
    }

    #[test]
    fn skips_yaml_parse_error() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills");
        let skill_dir = skills_dir.join("bad-yaml");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: [invalid yaml: {{\n---\n\nbody",
        )
        .unwrap();

        let skills = discover_skills(Some(dir.path()), None);
        assert_eq!(skills.len(), 0);
    }

    #[test]
    fn skips_permission_error() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills");
        let skill_dir = skills_dir.join("no-perm");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: no-perm\ndescription: test\n---\n\nbody",
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                skill_dir.join("SKILL.md"),
                fs::Permissions::from_mode(0o000),
            )
            .unwrap();
        }

        let skills = discover_skills(Some(dir.path()), None);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                skill_dir.join("SKILL.md"),
                fs::Permissions::from_mode(0o644),
            )
            .unwrap();
        }

        assert_eq!(skills.len(), 0);
    }

    #[test]
    fn discovers_scripts_in_skill() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills");
        let skill_dir = skills_dir.join("with-scripts");
        fs::create_dir_all(skill_dir.join("scripts")).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: with-scripts\ndescription: Has scripts\n---\n\nbody",
        )
        .unwrap();
        fs::write(skill_dir.join("scripts").join("load-context"), "#!/bin/bash\necho hi").unwrap();
        fs::write(skill_dir.join("scripts").join("run.sh"), "#!/bin/bash\necho run").unwrap();

        let skills = discover_skills(Some(dir.path()), None);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].scripts.len(), 2);
        assert!(skills[0].scripts.contains(&"load-context".to_string()));
        assert!(skills[0].scripts.contains(&"run.sh".to_string()));
    }

    #[test]
    fn skill_summary_includes_all_fields() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills");
        write_skill(&skills_dir, "my-skill", "my-skill", "A skill", "# Body");

        let summaries = list_skill_summaries(Some(dir.path()), None);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].name, "my-skill");
        assert_eq!(summaries[0].description, "A skill");
        assert_eq!(summaries[0].source, SkillSource::Workspace);
        assert!(summaries[0].user_invocable);
    }

    #[test]
    fn parses_kebab_case_frontmatter_fields() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills");
        let skill_dir = skills_dir.join("kebab-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: kebab-skill\ndescription: Has kebab\nargument-hint: \"[foo] [bar]\"\nuser-invocable: false\ndisable-model-invocation: true\nallowed-tools:\n  - read_file\n  - search_code\ntimeout-seconds: 42\nlicense: MIT\n---\n\nbody here",
        )
        .unwrap();

        let skills = discover_skills(Some(dir.path()), None);
        assert_eq!(skills.len(), 1);
        let s = &skills[0];
        assert_eq!(s.argument_hint.as_deref(), Some("[foo] [bar]"));
        assert_eq!(s.user_invocable, false);
        assert_eq!(s.disable_model_invocation, true);
        assert_eq!(s.allowed_tools, vec!["read_file".to_string(), "search_code".to_string()]);
        assert_eq!(s.timeout_seconds, Some(42));
        assert_eq!(s.license.as_deref(), Some("MIT"));
    }

    #[test]
    fn all_15_repo_skills_are_discoverable() {
        let repo_skills_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".agents")
            .join("skills");

        if !repo_skills_dir.is_dir() {
            return;
        }

        let skills = discover_skills_in_dir(&repo_skills_dir, SkillSource::Workspace).unwrap();
        assert!(
            skills.len() >= 15,
            "Expected at least 15 skills, found {}",
            skills.len()
        );
    }

    fn make_skill(name: &str, description: &str, source: SkillSource) -> Skill {
        Skill {
            name: name.to_string(),
            description: description.to_string(),
            source,
            body: String::new(),
            argument_hint: None,
            user_invocable: true,
            disable_model_invocation: false,
            allowed_tools: Vec::new(),
            timeout_seconds: None,
            license: None,
            scripts: Vec::new(),
        }
    }

    #[test]
    fn match_tdd_for_test_prompt() {
        let skills = vec![make_skill(
            "tdd",
            "Test-driven development with red-green-refactor loop. Use when user wants to build features or fix bugs using TDD.",
            SkillSource::Workspace,
        )];

        let matched = match_skills("write a failing test for the new feature", &skills);
        assert!(!matched.is_empty());
        assert_eq!(matched[0].skill.name, "tdd");
    }

    #[test]
    fn match_diagnose_for_debug_prompt() {
        let skills = vec![make_skill(
            "diagnose",
            "Disciplined diagnosis loop for hard bugs and performance regressions. Use when user says diagnose this debug this reports a bug.",
            SkillSource::Workspace,
        )];

        let matched = match_skills("diagnose this race condition bug", &skills);
        assert!(!matched.is_empty());
        assert_eq!(matched[0].skill.name, "diagnose");
    }

    #[test]
    fn no_match_for_hi() {
        let skills = vec![make_skill(
            "tdd",
            "Test-driven development with red-green-refactor loop.",
            SkillSource::Workspace,
        )];

        let matched = match_skills("hi", &skills);
        assert!(matched.is_empty());
    }

    #[test]
    fn matcher_workspace_wins_over_global_at_equal_score() {
        let skills = vec![
            make_skill(
                "tdd",
                "Test-driven development with red-green-refactor loop.",
                SkillSource::Global,
            ),
            make_skill(
                "tdd",
                "Test-driven development with red-green-refactor loop.",
                SkillSource::Workspace,
            ),
        ];

        let matched = match_skills("write a failing test", &skills);
        assert!(!matched.is_empty());
        assert_eq!(matched[0].skill.source, SkillSource::Workspace);
    }

    #[test]
    fn match_respects_threshold() {
        let skills = vec![make_skill(
            "caveman",
            "Ultra-compressed communication mode.",
            SkillSource::Workspace,
        )];

        let matched = match_skills("hello world", &skills);
        assert!(matched.is_empty());
    }

    #[test]
    fn top_n_limits_results() {
        let skills = vec![
            make_skill("a", "test driven development testing", SkillSource::Workspace),
            make_skill("b", "test driven development testing", SkillSource::Workspace),
            make_skill("c", "test driven development testing", SkillSource::Workspace),
            make_skill("d", "test driven development testing", SkillSource::Workspace),
        ];

        let matched = match_skills("test driven development", &skills);
        assert!(matched.len() <= MATCH_TOP_N);
    }

    #[test]
    fn format_skills_preamble_section_includes_names() {
        let matched = vec![SkillMatch {
            skill: make_skill("tdd", "Test driven development.", SkillSource::Workspace),
            score: 0.5,
        }];

        let section = format_skills_preamble_section(&matched).unwrap();
        assert!(section.contains("## Active Skills"));
        assert!(section.contains("**tdd**"));
        assert!(section.contains("Test driven development."));
    }

    #[test]
    fn format_invoked_skill_preamble_includes_body() {
        let mut skill = make_skill("tdd", "TDD skill.", SkillSource::Workspace);
        skill.body = "# TDD\n\nDo the thing.".to_string();

        let preamble = format_invoked_skill_preamble(&skill);
        assert!(preamble.contains("## Invoked Skill: tdd"));
        assert!(preamble.contains("# TDD"));
        assert!(preamble.contains("Do the thing."));
    }

    #[test]
    fn detect_interpreter_for_sh_with_shebang() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("test.sh");
        fs::write(&script, "#!/bin/bash\necho hello").unwrap();
        assert_eq!(detect_interpreter(&script).unwrap(), "/bin/bash");
    }

    #[test]
    fn detect_interpreter_for_mjs_without_shebang() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("test.mjs");
        fs::write(&script, "console.log('hi')").unwrap();
        assert_eq!(detect_interpreter(&script).unwrap(), "node");
    }

    #[test]
    fn detect_interpreter_for_js_without_shebang() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("test.js");
        fs::write(&script, "console.log('hi')").unwrap();
        assert_eq!(detect_interpreter(&script).unwrap(), "node");
    }

    #[test]
    fn detect_interpreter_rejects_unknown_extension() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("test.py");
        fs::write(&script, "print('hi')").unwrap();
        assert!(detect_interpreter(&script).is_err());
    }

    #[test]
    fn truncate_bytes_to_string_within_limit() {
        let (result, truncated) = truncate_bytes_to_string(b"hello", 100);
        assert_eq!(result, "hello");
        assert!(!truncated);
    }

    #[test]
    fn truncate_bytes_to_string_over_limit() {
        let data = vec![b'a'; 20000];
        let (result, truncated) = truncate_bytes_to_string(&data, 16 * 1024);
        assert!(truncated);
        assert!(result.contains("[truncated]"));
        assert!(result.len() <= 16 * 1024 + 100);
    }

    #[tokio::test]
    async fn run_skill_script_executes_bash_script() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills");
        let skill_dir = skills_dir.join("test-skill");
        let scripts_dir = skill_dir.join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();

        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: test-skill\ndescription: test\n---\n\nbody",
        )
        .unwrap();

        #[cfg(unix)]
        {
            let script_path = scripts_dir.join("hello");
            fs::write(&script_path, "#!/bin/bash\necho hello from script").unwrap();
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();

            let mut skill = make_skill("test-skill", "test", SkillSource::Workspace);
            skill.timeout_seconds = Some(5);

            let result = run_skill_script(&skill, "hello", Some(dir.path())).await.unwrap();
            assert_eq!(result.exit_code, 0);
            assert!(result.stdout.contains("hello from script"));
            assert!(!result.truncated);
        }
    }

    #[tokio::test]
    async fn run_skill_script_rejects_escape() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills");
        let skill_dir = skills_dir.join("test-skill");
        let scripts_dir = skill_dir.join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();

        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: test-skill\ndescription: test\n---\n\nbody",
        )
        .unwrap();

        let outside = tempdir().unwrap();
        let outside_script = outside.path().join("escape.sh");
        fs::write(&outside_script, "#!/bin/bash\necho escape").unwrap();

        let link = scripts_dir.join("escape.sh");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside_script, &link).unwrap();

        let skill = make_skill("test-skill", "test", SkillSource::Workspace);
        let result = run_skill_script(&skill, "escape.sh", Some(dir.path())).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("escapes"));
    }

    #[tokio::test]
    async fn run_skill_script_returns_error_for_missing_script() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".agents").join("skills");
        let skill_dir = skills_dir.join("test-skill");
        fs::create_dir_all(skill_dir.join("scripts")).unwrap();

        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: test-skill\ndescription: test\n---\n\nbody",
        )
        .unwrap();

        let skill = make_skill("test-skill", "test", SkillSource::Workspace);
        let result = run_skill_script(&skill, "nonexistent.sh", Some(dir.path())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_skill_script_tool_returns_failure_for_unknown_skill() {
        let tool = RunSkillScriptTool {
            available_skills: vec![],
            workspace_path: None,
        };
        let out = tool
            .call(RunSkillScriptArgs {
                skill: "nope".into(),
                script: "foo.sh".into(),
            })
            .await
            .unwrap();
        assert!(!out.success);
        assert!(out.error.unwrap().contains("Unknown skill"));
    }

    #[tokio::test]
    async fn run_skill_script_tool_includes_scripts_in_definition() {
        let mut sk = make_skill("with-scr", "d", SkillSource::Global);
        sk.scripts = vec!["do-it".to_string()];
        let tool = RunSkillScriptTool {
            available_skills: vec![sk],
            workspace_path: None,
        };
        let def = tool.definition("".into()).await;
        assert_eq!(def.name, "run_skill_script");
        assert!(def.description.contains("with-scr: [do-it]"));
    }
}
