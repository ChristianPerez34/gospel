use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

pub const SCOPE_FILE_RELATIVE_PATH: &str = ".gospel/bounty/scope.yaml";
const DEFAULT_MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
const DEFAULT_RATE_LIMIT_PER_SECOND: f64 = 1.0;

#[derive(Error, Debug)]
pub enum ScopeError {
    #[error("no bounty scope file at {path}")]
    Missing { path: PathBuf },
    #[error("failed to read bounty scope file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse bounty scope file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("bounty scope is not authorized; set `authorized: true` after confirming program authorization")]
    Unauthorized,
    #[error("bounty scope has no in-scope rules")]
    EmptyInScope,
    #[error("invalid bounty scope rule: {0}")]
    InvalidRule(String),
    #[error("auth profile '{profile}' uses unsupported secret reference '{secret_ref}'")]
    UnsupportedSecretRef { profile: String, secret_ref: String },
    #[error("auth profile '{profile}' secret environment variable '{variable}' is not set")]
    MissingSecret { profile: String, variable: String },
    #[error(
        "auth profile '{profile}' keychain secret '{name}' is not set (store it with the store_bounty_secret command)"
    )]
    MissingKeychainSecret { profile: String, name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeFile {
    pub program: String,
    #[serde(default)]
    pub authorized: bool,
    #[serde(default)]
    pub authorization_note: Option<String>,
    #[serde(default)]
    pub in_scope: Vec<ScopeRule>,
    #[serde(default)]
    pub out_of_scope: Vec<ScopeRule>,
    #[serde(default)]
    pub allow_mutations: bool,
    #[serde(default)]
    pub allow_http: bool,
    #[serde(default)]
    pub allow_private_hosts: bool,
    #[serde(default)]
    pub allowed_ports: Option<Vec<u16>>,
    #[serde(default)]
    pub rate_limit: Option<RateLimit>,
    #[serde(default)]
    pub auth_profiles: HashMap<String, AuthProfile>,
    #[serde(default)]
    pub max_response_bytes: Option<usize>,
    #[serde(default)]
    pub follow_redirects: bool,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeRule {
    pub host: String,
    #[serde(default)]
    pub paths: Option<Vec<String>>,
    #[serde(default)]
    pub methods: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    #[serde(default = "default_requests_per_second")]
    pub requests_per_second: f64,
    #[serde(default)]
    pub max_concurrent: Option<usize>,
}

fn default_requests_per_second() -> f64 {
    DEFAULT_RATE_LIMIT_PER_SECOND
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProfile {
    #[serde(rename = "type")]
    pub kind: String,
    pub secret_ref: String,
    #[serde(default)]
    pub header: Option<String>,
}

impl AuthProfile {
    pub fn resolve_secret(&self, profile_name: &str) -> Result<String, ScopeError> {
        if let Some(variable) = self.secret_ref.strip_prefix("env:") {
            return std::env::var(variable).map_err(|_| ScopeError::MissingSecret {
                profile: profile_name.to_string(),
                variable: variable.to_string(),
            });
        }
        if let Some(name) = self.secret_ref.strip_prefix("keychain:") {
            return crate::keychain::retrieve_bounty_secret(name).map_err(|_| {
                ScopeError::MissingKeychainSecret {
                    profile: profile_name.to_string(),
                    name: name.to_string(),
                }
            });
        }
        Err(ScopeError::UnsupportedSecretRef {
            profile: profile_name.to_string(),
            secret_ref: self.secret_ref.clone(),
        })
    }

    pub fn header_pair(&self, profile_name: &str) -> Result<(String, String), ScopeError> {
        let secret = self.resolve_secret(profile_name)?;
        match self.kind.as_str() {
            "bearer" => Ok(("Authorization".to_string(), format!("Bearer {secret}"))),
            "header" => {
                let header = self.header.clone().ok_or_else(|| {
                    ScopeError::InvalidRule(format!(
                        "auth profile '{profile_name}' of type 'header' requires `header`"
                    ))
                })?;
                Ok((header, secret))
            }
            "cookie" => Ok(("Cookie".to_string(), secret)),
            other => Err(ScopeError::InvalidRule(format!(
                "auth profile '{profile_name}' has unsupported type '{other}'"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
enum HostPattern {
    Exact(String),
    SubdomainsOf(String),
}

impl HostPattern {
    fn parse(raw: &str) -> Result<Self, ScopeError> {
        let host = raw.trim().to_ascii_lowercase();
        if host.is_empty() {
            return Err(ScopeError::InvalidRule("empty host".to_string()));
        }
        if host == "*" {
            return Err(ScopeError::InvalidRule(
                "wildcard-only host rules are not allowed; list explicit hosts".to_string(),
            ));
        }
        if let Some(suffix) = host.strip_prefix("*.") {
            if suffix.is_empty() || suffix.contains('*') {
                return Err(ScopeError::InvalidRule(format!(
                    "invalid wildcard host '{raw}'"
                )));
            }
            return Ok(Self::SubdomainsOf(suffix.to_string()));
        }
        if host.contains('*') {
            return Err(ScopeError::InvalidRule(format!(
                "invalid wildcard host '{raw}'; only a leading `*.` is supported"
            )));
        }
        Ok(Self::Exact(host))
    }

    fn matches(&self, host: &str) -> bool {
        match self {
            Self::Exact(expected) => host.eq_ignore_ascii_case(expected),
            Self::SubdomainsOf(suffix) => {
                let host = host.to_ascii_lowercase();
                host.len() > suffix.len() + 1
                    && host.ends_with(suffix)
                    && host.as_bytes()[host.len() - suffix.len() - 1] == b'.'
            }
        }
    }
}

#[derive(Debug, Clone)]
struct CompiledRule {
    host: HostPattern,
    paths: Vec<GlobMatcher>,
    methods: Option<Vec<String>>,
    raw: ScopeRule,
}

impl CompiledRule {
    fn compile(rule: &ScopeRule) -> Result<Self, ScopeError> {
        let host = HostPattern::parse(&rule.host)?;
        let paths = match rule.paths.as_ref() {
            Some(patterns) if !patterns.is_empty() => patterns
                .iter()
                .map(|pattern| {
                    Glob::new(pattern)
                        .map(|glob| glob.compile_matcher())
                        .map_err(|error| {
                            ScopeError::InvalidRule(format!(
                                "invalid path glob '{pattern}': {error}"
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => vec![Glob::new("**")
                .expect("static glob is valid")
                .compile_matcher()],
        };
        let methods = rule.methods.as_ref().map(|methods| {
            methods
                .iter()
                .map(|method| method.trim().to_ascii_uppercase())
                .filter(|method| !method.is_empty())
                .collect::<Vec<_>>()
        });
        Ok(Self {
            host,
            paths,
            methods,
            raw: rule.clone(),
        })
    }

    fn matches_host_and_path(&self, host: &str, path: &str) -> bool {
        self.host.matches(host) && self.paths.iter().any(|matcher| matcher.is_match(path))
    }

    fn allows_method(&self, method: &str, allow_mutations: bool) -> bool {
        match self.methods.as_ref() {
            Some(methods) => methods.iter().any(|allowed| allowed == method),
            None => is_read_only_method(method) || allow_mutations,
        }
    }
}

fn is_read_only_method(method: &str) -> bool {
    matches!(method, "GET" | "HEAD" | "OPTIONS")
}

fn is_mutating_method(method: &str) -> bool {
    matches!(method, "POST" | "PUT" | "PATCH" | "DELETE")
}

fn is_private_host(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();
    if lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".local")
        || lower.ends_with(".internal")
        || lower == "metadata.google.internal"
    {
        return true;
    }
    match lower.parse::<IpAddr>() {
        Ok(ip) => is_private_ip(ip),
        Err(_) => false,
    }
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_unspecified()
                || ip.is_documentation()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
        }
    }
}

#[derive(Debug, Clone)]
pub struct BountyScope {
    pub program: String,
    /// Parsed scope config kept for reporting; the tool does not read it yet.
    #[allow(dead_code)]
    pub authorization_note: Option<String>,
    pub allow_mutations: bool,
    pub allow_http: bool,
    pub allow_private_hosts: bool,
    pub allowed_ports: Vec<u16>,
    pub rate_limit: RateLimit,
    pub auth_profiles: HashMap<String, AuthProfile>,
    pub max_response_bytes: usize,
    /// When true, redirects are followed hop-by-hop; every hop re-passes
    /// `decide`, so a redirect can never land outside scope.
    pub follow_redirects: bool,
    pub timeout: Duration,
    in_scope: Vec<CompiledRule>,
    out_of_scope: Vec<CompiledRule>,
}

impl BountyScope {
    pub fn load(workspace_path: &Path) -> Result<Self, ScopeError> {
        let yaml_path = workspace_path.join(SCOPE_FILE_RELATIVE_PATH);
        let path = if yaml_path.is_file() {
            yaml_path
        } else {
            let yml_path = workspace_path.join(".gospel/bounty/scope.yml");
            if yml_path.is_file() {
                yml_path
            } else {
                return Err(ScopeError::Missing { path: yaml_path });
            }
        };
        let raw = std::fs::read_to_string(&path).map_err(|source| ScopeError::Io {
            path: path.clone(),
            source,
        })?;
        let file: ScopeFile =
            serde_yaml::from_str(&raw).map_err(|source| ScopeError::Parse {
                path: path.clone(),
                source,
            })?;
        Self::from_file(file)
    }

    fn from_file(file: ScopeFile) -> Result<Self, ScopeError> {
        if !file.authorized {
            return Err(ScopeError::Unauthorized);
        }
        if file.in_scope.is_empty() {
            return Err(ScopeError::EmptyInScope);
        }
        let in_scope = file
            .in_scope
            .iter()
            .map(CompiledRule::compile)
            .collect::<Result<Vec<_>, _>>()?;
        let out_of_scope = file
            .out_of_scope
            .iter()
            .map(CompiledRule::compile)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            program: file.program,
            authorization_note: file.authorization_note,
            allow_mutations: file.allow_mutations,
            allow_http: file.allow_http,
            allow_private_hosts: file.allow_private_hosts,
            allowed_ports: file.allowed_ports.unwrap_or_else(|| vec![80, 443]),
            rate_limit: file.rate_limit.unwrap_or(RateLimit {
                requests_per_second: DEFAULT_RATE_LIMIT_PER_SECOND,
                max_concurrent: Some(1),
            }),
            auth_profiles: file.auth_profiles,
            max_response_bytes: file.max_response_bytes.unwrap_or(DEFAULT_MAX_RESPONSE_BYTES),
            follow_redirects: file.follow_redirects,
            timeout: Duration::from_secs(
                file.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS),
            ),
            in_scope,
            out_of_scope,
        })
    }

    /// Human-readable scope summary for prompts and sweep gating.
    pub fn describe(&self) -> String {
        let render = |rules: &[CompiledRule]| -> String {
            if rules.is_empty() {
                return "  (none)".to_string();
            }
            rules
                .iter()
                .map(|rule| {
                    let mut line = format!("  - host {}", rule.raw.host);
                    if let Some(paths) = &rule.raw.paths {
                        line.push_str(&format!(" paths {:?}", paths));
                    }
                    if let Some(methods) = &rule.raw.methods {
                        line.push_str(&format!(" methods {:?}", methods));
                    }
                    line
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut text = format!(
            "program: {}\nin-scope:\n{}\nout-of-scope:\n{}\n",
            self.program,
            render(&self.in_scope),
            render(&self.out_of_scope)
        );
        text.push_str(&format!(
            "allowed_ports: {:?}, allow_mutations: {}, allow_http: {}, allow_private_hosts: {}, follow_redirects: {}",
            self.allowed_ports,
            self.allow_mutations,
            self.allow_http,
            self.allow_private_hosts,
            self.follow_redirects
        ));
        if !self.auth_profiles.is_empty() {
            let mut profiles: Vec<&str> =
                self.auth_profiles.keys().map(String::as_str).collect();
            profiles.sort_unstable();
            text.push_str(&format!("\nauth_profiles: {}", profiles.join(", ")));
        }
        text
    }

    pub fn decide(&self, method: &str, url: &reqwest::Url) -> Result<(), String> {
        let method = method.trim().to_ascii_uppercase();
        if !is_read_only_method(&method) && !is_mutating_method(&method) {
            return Err(format!("method '{method}' is not supported"));
        }
        if is_mutating_method(&method) && !self.allow_mutations {
            return Err(format!(
                "method '{method}' is mutating and `allow_mutations` is false in the scope file"
            ));
        }

        let scheme = url.scheme();
        if scheme != "https" && !(scheme == "http" && self.allow_http) {
            return Err(format!(
                "scheme '{scheme}' is not allowed; use https{}",
                if self.allow_http {
                    ""
                } else {
                    " or enable `allow_http`"
                }
            ));
        }

        let host = url
            .host_str()
            .ok_or_else(|| "request URL has no host".to_string())?
            .to_ascii_lowercase();
        if is_private_host(&host) && !self.allow_private_hosts {
            return Err(format!(
                "host '{host}' is private or local and `allow_private_hosts` is false"
            ));
        }

        let port = url
            .port_or_known_default()
            .ok_or_else(|| "request URL has no port".to_string())?;
        if !self.allowed_ports.contains(&port) {
            return Err(format!(
                "port {port} is not allowed; allowed ports: {:?}",
                self.allowed_ports
            ));
        }

        let path = if url.path().is_empty() {
            "/"
        } else {
            url.path()
        };

        for rule in &self.out_of_scope {
            if rule.matches_host_and_path(&host, path) {
                return Err(format!(
                    "request matches an out-of-scope rule for host '{host}' path '{path}'"
                ));
            }
        }

        let mut host_matched = false;
        for rule in &self.in_scope {
            if rule.matches_host_and_path(&host, path) {
                host_matched = true;
                if rule.allows_method(&method, self.allow_mutations) {
                    return Ok(());
                }
            }
        }

        if host_matched {
            Err(format!(
                "method '{method}' is not allowed for host '{host}' path '{path}'"
            ))
        } else {
            Err(format!(
                "host '{host}' path '{path}' is not in scope for program '{}'",
                self.program
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn parse_scope(yaml: &str) -> BountyScope {
        let workspace = tempdir().unwrap();
        let path = workspace.path().join(SCOPE_FILE_RELATIVE_PATH);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, yaml).unwrap();
        BountyScope::load(workspace.path()).unwrap()
    }

    fn url(raw: &str) -> reqwest::Url {
        reqwest::Url::parse(raw).unwrap()
    }

    const BASE: &str = r#"
program: acme
authorized: true
in_scope:
  - host: example.com
    paths: ["/api/*"]
  - host: "*.example.com"
out_of_scope:
  - host: example.com
    paths: ["/api/admin/*"]
"#;

    #[test]
    fn allows_exact_host_and_path() {
        let scope = parse_scope(BASE);
        assert!(scope.decide("GET", &url("https://example.com/api/users")).is_ok());
    }

    #[test]
    fn allows_wildcard_subdomain() {
        let scope = parse_scope(BASE);
        assert!(scope.decide("GET", &url("https://app.example.com/dashboard")).is_ok());
    }

    #[test]
    fn wildcard_does_not_match_apex() {
        let scope = parse_scope(
            "program: acme\nauthorized: true\nin_scope:\n  - host: '*.example.com'",
        );
        assert!(scope.decide("GET", &url("https://example.com/")).is_err());
    }

    #[test]
    fn denies_out_of_scope_path() {
        let scope = parse_scope(BASE);
        assert!(scope.decide("GET", &url("https://example.com/api/admin/users")).is_err());
    }

    #[test]
    fn denies_host_outside_scope() {
        let scope = parse_scope(BASE);
        assert!(scope.decide("GET", &url("https://other.com/")).is_err());
    }

    #[test]
    fn denies_mutations_by_default() {
        let scope = parse_scope(BASE);
        assert!(scope.decide("POST", &url("https://example.com/api/users")).is_err());
    }

    #[test]
    fn allows_mutations_when_enabled() {
        let scope = parse_scope(
            "program: acme\nauthorized: true\nallow_mutations: true\nin_scope:\n  - host: example.com",
        );
        assert!(scope.decide("POST", &url("https://example.com/api/users")).is_ok());
    }

    #[test]
    fn explicit_method_allowlist_wins() {
        let scope = parse_scope(
            "program: acme\nauthorized: true\nin_scope:\n  - host: example.com\n    methods: [GET, POST]",
        );
        assert!(scope.decide("POST", &url("https://example.com/")).is_err());
        let scope = parse_scope(
            "program: acme\nauthorized: true\nallow_mutations: true\nin_scope:\n  - host: example.com\n    methods: [POST]",
        );
        assert!(scope.decide("POST", &url("https://example.com/")).is_ok());
        assert!(scope.decide("GET", &url("https://example.com/")).is_err());
    }

    #[test]
    fn denies_http_unless_enabled() {
        let scope = parse_scope(BASE);
        assert!(scope.decide("GET", &url("http://example.com/api/users")).is_err());
        let scope = parse_scope(
            "program: acme\nauthorized: true\nallow_http: true\nin_scope:\n  - host: example.com",
        );
        assert!(scope.decide("GET", &url("http://example.com/")).is_ok());
    }

    #[test]
    fn denies_private_hosts_unless_enabled() {
        let scope = parse_scope(
            "program: acme\nauthorized: true\nin_scope:\n  - host: localhost\n  - host: 127.0.0.1",
        );
        assert!(scope.decide("GET", &url("https://localhost/")).is_err());
        assert!(scope.decide("GET", &url("https://127.0.0.1/")).is_err());

        let scope = parse_scope(
            "program: acme\nauthorized: true\nallow_private_hosts: true\nin_scope:\n  - host: localhost",
        );
        assert!(scope.decide("GET", &url("https://localhost/")).is_ok());
    }

    #[test]
    fn denies_unlisted_port() {
        let scope = parse_scope(BASE);
        assert!(scope.decide("GET", &url("https://example.com:8443/api/users")).is_err());
    }

    #[test]
    fn requires_authorization() {
        let workspace = tempdir().unwrap();
        let path = workspace.path().join(SCOPE_FILE_RELATIVE_PATH);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "program: acme\nin_scope:\n  - host: example.com").unwrap();
        assert!(matches!(
            BountyScope::load(workspace.path()),
            Err(ScopeError::Unauthorized)
        ));
    }

    #[test]
    fn requires_in_scope_rules() {
        let workspace = tempdir().unwrap();
        let path = workspace.path().join(SCOPE_FILE_RELATIVE_PATH);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "program: acme\nauthorized: true").unwrap();
        assert!(matches!(
            BountyScope::load(workspace.path()),
            Err(ScopeError::EmptyInScope)
        ));
    }

    #[test]
    fn rejects_wildcard_only_host() {
        let workspace = tempdir().unwrap();
        let path = workspace.path().join(SCOPE_FILE_RELATIVE_PATH);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "program: acme\nauthorized: true\nin_scope:\n  - host: '*'",
        )
        .unwrap();
        assert!(matches!(
            BountyScope::load(workspace.path()),
            Err(ScopeError::InvalidRule(_))
        ));
    }

    #[test]
    fn resolves_env_auth_profile() {
        let variable = format!("GOSPEL_TEST_TOKEN_{}", std::process::id());
        std::env::set_var(&variable, "secret-token");
        let scope = parse_scope(&format!(
            "program: acme\nauthorized: true\nin_scope:\n  - host: example.com\nauth_profiles:\n  user_a:\n    type: bearer\n    secret_ref: env:{variable}"
        ));
        let profile = scope.auth_profiles.get("user_a").unwrap();
        assert_eq!(
            profile.header_pair("user_a").unwrap(),
            ("Authorization".to_string(), "Bearer secret-token".to_string())
        );
    }

    #[test]
    fn resolves_keychain_auth_profile() {
        let config_home = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", config_home.path());
        crate::keychain::store_bounty_secret("acme_session", "cookie-value").unwrap();

        let profile = AuthProfile {
            kind: "cookie".to_string(),
            secret_ref: "keychain:acme_session".to_string(),
            header: None,
        };
        assert_eq!(profile.resolve_secret("session").unwrap(), "cookie-value");
        let _ = crate::keychain::delete_bounty_secret("acme_session");

        let missing = AuthProfile {
            kind: "bearer".to_string(),
            secret_ref: "keychain:not_there".to_string(),
            header: None,
        };
        assert!(matches!(
            missing.resolve_secret("other"),
            Err(ScopeError::MissingKeychainSecret { .. })
        ));

        let literal = AuthProfile {
            kind: "bearer".to_string(),
            secret_ref: "sk-pasted-into-file".to_string(),
            header: None,
        };
        assert!(matches!(
            literal.resolve_secret("other"),
            Err(ScopeError::UnsupportedSecretRef { .. })
        ));
    }
}
