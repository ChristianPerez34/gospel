use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;

pub(crate) const DEFAULT_MAX_TURNS: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AgentModelRef {
    Name(String),
    Detailed {
        #[serde(default)]
        provider: Option<String>,
        model: String,
    },
}

impl AgentModelRef {
    pub fn model_name(&self) -> &str {
        match self {
            Self::Name(model) => model,
            Self::Detailed { model, .. } => model,
        }
    }

    pub fn provider<'a>(&'a self, fallback: &'a str) -> &'a str {
        match self {
            Self::Name(_) => fallback,
            Self::Detailed { provider, .. } => provider.as_deref().unwrap_or(fallback),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub model: Option<AgentModelRef>,
    #[serde(default)]
    pub worker_model: Option<AgentModelRef>,
    #[serde(default)]
    pub orchestration: Option<String>,
    #[serde(default)]
    pub max_turns: Option<usize>,
    #[serde(default)]
    pub deadline_seconds: Option<u64>,
    #[serde(default)]
    pub network: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentSource {
    Workspace,
    Global,
}

/// How a delegated run executes. Absent means a normal single agent turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrchestration {
    /// Deterministic mutator → worker fan-out → feedback rounds → triage.
    Sweep,
}

impl AgentOrchestration {
    fn parse(raw: &str, agent_name: &str) -> Option<Self> {
        match raw {
            "sweep" => Some(Self::Sweep),
            other => {
                tracing::warn!(
                    "Agent '{}' declares unknown orchestration '{}'; ignoring",
                    agent_name,
                    other
                );
                None
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub source: AgentSource,
    pub body: String,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub model: Option<AgentModelRef>,
    pub worker_model: Option<AgentModelRef>,
    pub orchestration: Option<AgentOrchestration>,
    pub max_turns: usize,
    #[serde(serialize_with = "serialize_optional_duration_seconds")]
    pub deadline: Option<Duration>,
    pub network: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentSummary {
    pub name: String,
    pub description: String,
    pub source: AgentSource,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub network: bool,
    pub orchestration: Option<AgentOrchestration>,
}

impl From<&AgentDefinition> for AgentSummary {
    fn from(agent: &AgentDefinition) -> Self {
        Self {
            name: agent.name.clone(),
            description: agent.description.clone(),
            source: agent.source.clone(),
            tools: agent.tools.clone(),
            skills: agent.skills.clone(),
            network: agent.network,
            orchestration: agent.orchestration,
        }
    }
}

fn serialize_optional_duration_seconds<S>(
    value: &Option<Duration>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(duration) => serializer.serialize_some(&duration.as_secs()),
        None => serializer.serialize_none(),
    }
}

fn normalize_crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn canonicalize_with_warning(path: &Path) -> Option<PathBuf> {
    match fs::canonicalize(path) {
        Ok(path) => Some(path),
        Err(error) => {
            tracing::warn!("Failed to canonicalize {}: {}", path.display(), error);
            None
        }
    }
}

fn parse_agent_file(
    agent_dir: &Path,
    source: AgentSource,
    agents_root: &Path,
) -> Result<Option<AgentDefinition>, String> {
    let agent_md_path = agent_dir.join("AGENT.md");
    if !agent_md_path.exists() {
        return Ok(None);
    }

    let Some(canonical_agents_root) = canonicalize_with_warning(agents_root) else {
        return Ok(None);
    };
    let Some(canonical_agent_dir) = canonicalize_with_warning(agent_dir) else {
        return Ok(None);
    };
    if !canonical_agent_dir.starts_with(&canonical_agents_root) {
        tracing::warn!(
            "Symlink escape detected: agent dir {} is outside agents root {}",
            canonical_agent_dir.display(),
            canonical_agents_root.display()
        );
        return Ok(None);
    }

    let raw = match fs::read_to_string(&agent_md_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            tracing::warn!(
                "Permission denied reading {}: {}",
                agent_md_path.display(),
                error
            );
            return Ok(None);
        }
        Err(error) => {
            tracing::warn!("Failed to read {}: {}", agent_md_path.display(), error);
            return Ok(None);
        }
    };

    let content = normalize_crlf(&raw);
    let content = content.strip_prefix("---\n").unwrap_or(content.as_str());
    let parts: Vec<&str> = content.splitn(2, "\n---\n").collect();
    if parts.len() < 2 {
        tracing::warn!(
            "Missing frontmatter in {}: expected closing `---` delimiter",
            agent_md_path.display()
        );
        return Ok(None);
    }

    let frontmatter_str = parts[0].trim();
    if frontmatter_str.is_empty() {
        tracing::warn!("Empty frontmatter in {}", agent_md_path.display());
        return Ok(None);
    }

    let frontmatter: AgentFrontmatter = match serde_yaml::from_str(frontmatter_str) {
        Ok(frontmatter) => frontmatter,
        Err(error) => {
            tracing::warn!("YAML parse error in {}: {}", agent_md_path.display(), error);
            return Ok(None);
        }
    };

    let folder_name = agent_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if frontmatter.name != folder_name {
        tracing::warn!(
            "Agent name '{}' does not match folder '{}' in {}; skipping",
            frontmatter.name,
            folder_name,
            agent_dir.display()
        );
        return Ok(None);
    }

    let name = frontmatter.name.clone();
    Ok(Some(AgentDefinition {
        name: frontmatter.name,
        description: frontmatter.description,
        source,
        body: parts[1].trim().to_string(),
        tools: frontmatter.tools.unwrap_or_default(),
        skills: frontmatter.skills.unwrap_or_default(),
        model: frontmatter.model,
        worker_model: frontmatter.worker_model,
        orchestration: frontmatter
            .orchestration
            .and_then(|value| AgentOrchestration::parse(&value, &name)),
        max_turns: frontmatter.max_turns.unwrap_or(DEFAULT_MAX_TURNS),
        deadline: frontmatter.deadline_seconds.map(Duration::from_secs),
        network: frontmatter.network.unwrap_or(false),
    }))
}

fn discover_agents_in_dir(
    agents_dir: &Path,
    source: AgentSource,
) -> Result<Vec<AgentDefinition>, String> {
    let entries = match fs::read_dir(agents_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            tracing::warn!(
                "Permission denied reading agents dir: {}",
                agents_dir.display()
            );
            return Ok(Vec::new());
        }
        Err(error) => return Err(format!("Failed to read agents dir: {}", error)),
    };

    let mut agents = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(
                    "Failed to read entry in {}: {}",
                    agents_dir.display(),
                    error
                );
                continue;
            }
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        match parse_agent_file(&path, source.clone(), agents_dir) {
            Ok(Some(agent)) => agents.push(agent),
            Ok(None) => {}
            Err(error) => {
                tracing::warn!("Error parsing agent at {}: {}", path.display(), error);
            }
        }
    }
    Ok(agents)
}

pub fn discover_agents(
    workspace_path: Option<&Path>,
    global_agents_dir: Option<&Path>,
) -> Vec<AgentDefinition> {
    let mut agents = Vec::new();
    let mut seen_names: Vec<String> = Vec::new();

    if let Some(workspace) = workspace_path {
        let workspace_agents_dir = workspace.join(".agents").join("agents");
        if workspace_agents_dir.is_dir() {
            match discover_agents_in_dir(&workspace_agents_dir, AgentSource::Workspace) {
                Ok(workspace_agents) => {
                    for agent in workspace_agents {
                        if seen_names.contains(&agent.name) {
                            tracing::warn!(
                                "Duplicate agent name '{}'; workspace wins over global",
                                agent.name
                            );
                            continue;
                        }
                        seen_names.push(agent.name.clone());
                        agents.push(agent);
                    }
                }
                Err(error) => tracing::warn!("Failed to scan workspace agents: {}", error),
            }
        }
    }

    if let Some(global_dir) = global_agents_dir {
        if global_dir.is_dir() {
            match discover_agents_in_dir(global_dir, AgentSource::Global) {
                Ok(global_agents) => {
                    for agent in global_agents {
                        if seen_names.contains(&agent.name) {
                            continue;
                        }
                        seen_names.push(agent.name.clone());
                        agents.push(agent);
                    }
                }
                Err(error) => tracing::warn!("Failed to scan global agents: {}", error),
            }
        }
    }

    agents
}

pub fn global_agents_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|dir| dir.join(".agents").join("agents"))
}

pub struct AgentLoader {
    cache: RwLock<HashMap<PathBuf, Vec<AgentDefinition>>>,
    global_dir: Option<PathBuf>,
}

impl Default for AgentLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentLoader {
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            global_dir: global_agents_dir(),
        }
    }

    pub fn load(&self, workspace_path: Option<&Path>) -> Vec<AgentDefinition> {
        let canonical_key = workspace_path.and_then(|path| fs::canonicalize(path).ok());
        if let Some(ref key) = canonical_key {
            if let Ok(cache) = self.cache.read() {
                if let Some(cached) = cache.get(key) {
                    return cached.clone();
                }
            }
        }

        let discovered = discover_agents(workspace_path, self.global_dir.as_deref());
        if let Some(ref key) = canonical_key {
            if let Ok(mut cache) = self.cache.write() {
                cache.insert(key.clone(), discovered.clone());
            }
        }
        discovered
    }

    pub fn find(&self, workspace_path: Option<&Path>, name: &str) -> Option<AgentDefinition> {
        self.load(workspace_path)
            .into_iter()
            .find(|agent| agent.name == name)
    }

    pub fn invalidate(&self, workspace_path: &Path) {
        if let Ok(canonical) = fs::canonicalize(workspace_path) {
            if let Ok(mut cache) = self.cache.write() {
                cache.remove(&canonical);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_agent(dir: &Path, folder: &str, frontmatter: &str, body: &str) {
        let agent_dir = dir.join(folder);
        fs::create_dir_all(&agent_dir).unwrap();
        fs::write(
            agent_dir.join("AGENT.md"),
            format!("---\n{}\n---\n\n{}", frontmatter, body),
        )
        .unwrap();
    }

    #[test]
    fn discovers_workspace_agent() {
        let workspace = tempdir().unwrap();
        let agents_dir = workspace.path().join(".agents/agents");
        write_agent(
            &agents_dir,
            "bug-bounty",
            "name: bug-bounty\ndescription: Black-box web bounty researcher\ntools: [read_file, http_request]\nnetwork: true",
            "# Bug Bounty\n\nStay in scope.",
        );

        let agents = discover_agents(Some(workspace.path()), None);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "bug-bounty");
        assert_eq!(agents[0].tools, vec!["read_file", "http_request"]);
        assert!(agents[0].network);
        assert_eq!(agents[0].max_turns, DEFAULT_MAX_TURNS);
        assert!(agents[0].body.contains("Stay in scope"));
    }

    #[test]
    fn discovers_global_agent() {
        let global = tempdir().unwrap();
        write_agent(
            global.path(),
            "global-agent",
            "name: global-agent\ndescription: Global",
            "Global body",
        );

        let agents = discover_agents(None, Some(global.path()));
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].source, AgentSource::Global);
    }

    #[test]
    fn workspace_wins_over_global() {
        let workspace = tempdir().unwrap();
        let global = tempdir().unwrap();
        write_agent(
            &workspace.path().join(".agents/agents"),
            "shared",
            "name: shared\ndescription: Workspace",
            "Workspace body",
        );
        write_agent(
            global.path(),
            "shared",
            "name: shared\ndescription: Global",
            "Global body",
        );

        let agents = discover_agents(Some(workspace.path()), Some(global.path()));
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].source, AgentSource::Workspace);
        assert_eq!(agents[0].description, "Workspace");
    }

    #[test]
    fn rejects_name_folder_mismatch() {
        let workspace = tempdir().unwrap();
        write_agent(
            &workspace.path().join(".agents/agents"),
            "folder",
            "name: different\ndescription: Mismatch",
            "Body",
        );

        assert!(discover_agents(Some(workspace.path()), None).is_empty());
    }

    #[test]
    fn parses_model_and_guards() {
        let workspace = tempdir().unwrap();
        write_agent(
            &workspace.path().join(".agents/agents"),
            "bounty",
            "name: bounty\ndescription: Bounty\nmodel:\n  provider: groq\n  model: llama-3.3-70b-versatile\nworker_model: llama-3.1-8b-instant\nmax_turns: 12\ndeadline_seconds: 90",
            "Body",
        );

        let agents = discover_agents(Some(workspace.path()), None);
        let agent = &agents[0];
        let model = agent.model.as_ref().unwrap();
        assert_eq!(model.provider("openai"), "groq");
        assert_eq!(model.model_name(), "llama-3.3-70b-versatile");
        let worker = agent.worker_model.as_ref().unwrap();
        assert_eq!(worker.provider("groq"), "groq");
        assert_eq!(worker.model_name(), "llama-3.1-8b-instant");
        assert_eq!(agent.max_turns, 12);
        assert_eq!(agent.deadline, Some(Duration::from_secs(90)));
    }

    #[test]
    fn rejects_missing_frontmatter() {
        let workspace = tempdir().unwrap();
        let agent_dir = workspace.path().join(".agents/agents/broken");
        fs::create_dir_all(&agent_dir).unwrap();
        fs::write(agent_dir.join("AGENT.md"), "# No frontmatter").unwrap();

        assert!(discover_agents(Some(workspace.path()), None).is_empty());
    }

    #[test]
    fn loader_finds_agent_by_name() {
        let workspace = tempdir().unwrap();
        write_agent(
            &workspace.path().join(".agents/agents"),
            "bounty",
            "name: bounty\ndescription: Bounty",
            "Body",
        );

        let loader = AgentLoader::new();
        assert!(loader.find(Some(workspace.path()), "bounty").is_some());
        assert!(loader.find(Some(workspace.path()), "missing").is_none());
    }
}
