use super::http_tool::{BountyHttpArgs, BountyHttpTool};
use super::scope::BountyScope;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use thiserror::Error;

const MAX_LINKS: usize = 100;
const MAX_FORMS: usize = 30;
const MAX_SCRIPTS: usize = 50;
const MAX_TEXT_CHARS: usize = 80;

#[derive(Error, Debug)]
pub enum BountySurfaceError {
    #[error("surface extraction failed: {0}")]
    Failed(String),
}

/// Browser-lite recon: fetches a page through the scoped `BountyHttpTool` (so
/// scope, checkpoint freeze, rate limit, redirects, and evidence all apply),
/// then extracts the attack surface — links, forms, scripts — with each
/// discovered endpoint marked `in_scope` so the agent knows what is testable.
/// No JS execution; SPAs that render client-side will show as sparse.
#[derive(Debug)]
pub struct BountySurfaceTool {
    http: BountyHttpTool,
    evidence_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct BountySurfaceArgs {
    /// Absolute in-scope URL to fetch (GET only).
    pub url: String,
    /// Optional named auth profile from the scope file.
    pub auth_profile: Option<String>,
    /// Optional request headers object (string → string).
    pub headers: Option<std::collections::HashMap<String, String>>,
    /// Optional per-request timeout in milliseconds.
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct SurfaceLink {
    pub url: String,
    pub in_scope: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SurfaceForm {
    pub method: String,
    pub action: Option<String>,
    pub action_in_scope: Option<bool>,
    pub inputs: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BountySurfaceOutput {
    pub success: bool,
    pub denied: bool,
    pub url: String,
    pub evidence_id: Option<String>,
    pub title: Option<String>,
    pub links: Vec<SurfaceLink>,
    pub forms: Vec<SurfaceForm>,
    pub scripts: Vec<String>,
    pub summary: String,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct EvidenceBody {
    response_body: String,
}

impl BountySurfaceTool {
    pub fn new(
        workspace_path: &std::path::Path,
        run_id: &str,
    ) -> Result<Self, super::http_tool::BountyHttpError> {
        let http = BountyHttpTool::new(workspace_path, run_id)?;
        let evidence_dir = super::http_tool::run_dir(workspace_path, run_id).join("evidence");
        Ok(Self { http, evidence_dir })
    }

    fn read_evidence_body(&self, evidence_id: &str) -> Option<String> {
        let path = self.evidence_dir.join(format!("{evidence_id}.json"));
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str::<EvidenceBody>(&raw)
            .ok()
            .map(|body| body.response_body)
    }
}

fn trim(text: &str) -> Option<String> {
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        None
    } else {
        Some(
            crate::text_utils::truncate_text_bytes(&text, MAX_TEXT_CHARS)
                .0,
        )
    }
}

fn extract_surface(html: &str, base: &Url, scope: Option<&BountyScope>) -> ParsedSurface {
    let document = scraper::Html::parse_document(html);
    let in_scope = |url: &Url| -> bool {
        scope
            .map(|scope| scope.decide("GET", url).is_ok())
            .unwrap_or(false)
    };

    let mut links = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let link_sel = scraper::Selector::parse("a[href]").unwrap();
    for el in document.select(&link_sel) {
        if links.len() >= MAX_LINKS {
            break;
        }
        let Some(href) = el.value().attr("href") else {
            continue;
        };
        let Ok(url) = base.join(href) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https") || !seen.insert(url.as_str().to_string()) {
            continue;
        }
        links.push(SurfaceLink {
            in_scope: in_scope(&url),
            url: url.as_str().to_string(),
            text: trim(&el.text().collect::<String>()),
        });
    }

    let mut forms = Vec::new();
    let form_sel = scraper::Selector::parse("form").unwrap();
    let input_sel = scraper::Selector::parse("input, textarea, select").unwrap();
    for form in document.select(&form_sel) {
        if forms.len() >= MAX_FORMS {
            break;
        }
        let method = form
            .value()
            .attr("method")
            .unwrap_or("GET")
            .to_ascii_uppercase();
        let action = form
            .value()
            .attr("action")
            .and_then(|action| base.join(action).ok())
            .map(|url| url.as_str().to_string());
        let mut inputs = Vec::new();
        for input in form.select(&input_sel) {
            let name = input.value().attr("name").unwrap_or("");
            let kind = input.value().attr("type").unwrap_or("text");
            inputs.push(if name.is_empty() {
                kind.to_string()
            } else {
                format!("{name}({kind})")
            });
        }
        forms.push(SurfaceForm {
            method,
            action_in_scope: action
                .as_ref()
                .and_then(|a| Url::parse(a).ok().map(|u| in_scope(&u))),
            action,
            inputs,
        });
    }

    let mut scripts = Vec::new();
    let script_sel = scraper::Selector::parse("script[src]").unwrap();
    for el in document.select(&script_sel) {
        if scripts.len() >= MAX_SCRIPTS {
            break;
        }
        if let Some(src) = el.value().attr("src") {
            if let Ok(url) = base.join(src) {
                scripts.push(url.as_str().to_string());
            }
        }
    }

    let title = scraper::Selector::parse("title")
        .ok()
        .and_then(|sel| document.select(&sel).next())
        .and_then(|el| trim(&el.text().collect::<String>()));

    ParsedSurface {
        title,
        links,
        forms,
        scripts,
    }
}

struct ParsedSurface {
    title: Option<String>,
    links: Vec<SurfaceLink>,
    forms: Vec<SurfaceForm>,
    scripts: Vec<String>,
}

impl Tool for BountySurfaceTool {
    const NAME: &'static str = "bounty_surface";

    type Error = BountySurfaceError;
    type Args = BountySurfaceArgs;
    type Output = BountySurfaceOutput;

    fn description(&self) -> String {
        "Fetch one in-scope page through the scoped HTTP layer and extract its attack surface: title, links, forms (method/action/inputs), and script URLs. Each link and form action is marked in_scope so you know what is testable. GET only; no JavaScript execution.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Absolute in-scope URL to fetch and analyze."
                },
                "auth_profile": {
                    "type": "string",
                    "description": "Optional named auth profile from the scope file."
                },
                "headers": {
                    "type": "object",
                    "description": "Optional request headers."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Optional per-request timeout in milliseconds."
                }
            },
            "required": ["url"]
        })
    }

    fn map_error(&self, error: Self::Error) -> ToolExecutionError {
        ToolExecutionError::other(error.to_string())
    }

    async fn call(
        &self,
        context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let http_args = BountyHttpArgs {
            method: "GET".to_string(),
            url: args.url.clone(),
            headers: args.headers,
            body: None,
            auth_profile: args.auth_profile,
            content_type: None,
            timeout_ms: args.timeout_ms,
        };
        let http = self.http.call(context, http_args).await.map_err(|e| {
            BountySurfaceError::Failed(e.to_string())
        })?;
        if !http.success {
            return Ok(BountySurfaceOutput {
                success: false,
                denied: http.denied,
                url: http.url,
                evidence_id: http.evidence_id,
                title: None,
                links: Vec::new(),
                forms: Vec::new(),
                scripts: Vec::new(),
                summary: http.summary,
                error: http.error,
            });
        }
        let Some(body) = http
            .evidence_id
            .as_deref()
            .and_then(|id| self.read_evidence_body(id))
        else {
            return Ok(BountySurfaceOutput {
                success: true,
                denied: false,
                url: http.url,
                evidence_id: http.evidence_id,
                title: None,
                links: Vec::new(),
                forms: Vec::new(),
                scripts: Vec::new(),
                summary: format!("{} — no evidence body to parse", http.summary),
                error: None,
            });
        };
        let base = Url::parse(&http.url).unwrap_or_else(|_| Url::parse(&args.url).unwrap());
        let scope = self.http.scope.as_deref();
        let parsed = extract_surface(&body, &base, scope);
        let in_scope_links = parsed.links.iter().filter(|l| l.in_scope).count();
        Ok(BountySurfaceOutput {
            success: true,
            denied: false,
            summary: format!(
                "{} — {} links ({} in-scope), {} forms, {} scripts",
                http.summary,
                parsed.links.len(),
                in_scope_links,
                parsed.forms.len(),
                parsed.scripts.len()
            ),
            url: http.url,
            evidence_id: http.evidence_id,
            title: parsed.title,
            links: parsed.links,
            forms: parsed.forms,
            scripts: parsed.scripts,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bounty::scope::BountyScope;

    fn test_scope() -> BountyScope {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".gospel/bounty/scope.yaml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "program: t\nauthorized: true\nin_scope:\n  - host: example.com\n",
        )
        .unwrap();
        // Leak the tempdir — tests are process-scoped.
        let scope = BountyScope::load(dir.path()).unwrap();
        std::mem::forget(dir);
        scope
    }

    #[test]
    fn extracts_links_forms_scripts_with_scope_marks() {
        let html = r##"<html><head><title>Login</title>
            <script src="/js/app.js"></script><script src="https://cdn.example.com/x.js"></script></head>
            <body><a href="/app">app</a><a href="https://other.evil/phish">ext</a>
            <a href="#frag">frag</a>
            <form method="post" action="/login">
                <input name="user" type="text"><input name="pass" type="password">
                <input type="hidden" name="csrf" value="x">
            </form></body></html>"##;
        let base = Url::parse("https://example.com/start").unwrap();
        let scope = test_scope();
        let surface = extract_surface(html, &base, Some(&scope));

        assert_eq!(surface.title.as_deref(), Some("Login"));
        assert_eq!(surface.links.len(), 3);
        let app = &surface.links[0];
        assert_eq!(app.url, "https://example.com/app");
        assert!(app.in_scope);
        assert!(!surface.links[1].in_scope); // external host
        assert_eq!(surface.scripts.len(), 2);
        let form = &surface.forms[0];
        assert_eq!(form.method, "POST");
        assert_eq!(form.action.as_deref(), Some("https://example.com/login"));
        assert_eq!(form.action_in_scope, Some(true));
        assert!(form.inputs.iter().any(|i| i.starts_with("user")));
    }

    #[test]
    fn no_scope_marks_everything_out() {
        let html = r#"<a href="/x">x</a>"#;
        let base = Url::parse("https://example.com/").unwrap();
        let surface = extract_surface(html, &base, None);
        assert!(!surface.links[0].in_scope);
    }
}
