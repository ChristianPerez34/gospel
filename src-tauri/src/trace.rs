use regex::Regex;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_TRACE_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB per file
const MAX_GLOBAL_TRACE_SIZE: u64 = 250 * 1024 * 1024; // 250 MB global cap
const TRACE_RETENTION_DAYS: u64 = 30;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceEvent {
    TurnStart {
        session_id: String,
        role: String,
        provider: String,
        model: String,
        timestamp: u64,
    },
    ToolCall {
        session_id: String,
        role: String,
        tool_name: String,
        arguments_redacted: String,
        timestamp: u64,
    },
    ToolResult {
        session_id: String,
        role: String,
        tool_name: String,
        result_summary: String,
        timestamp: u64,
    },
    Warning {
        session_id: String,
        role: String,
        message: String,
        timestamp: u64,
    },
    Stopped {
        session_id: String,
        role: String,
        reason: String,
        count: usize,
        timestamp: u64,
    },
    Done {
        session_id: String,
        role: String,
        response_length: usize,
        prompt_tokens: usize,
        response_tokens: usize,
        tool_calls: usize,
        timestamp: u64,
    },
    Error {
        session_id: String,
        role: String,
        error_code: String,
        error_message: String,
        timestamp: u64,
    },
}

pub struct TraceLogger {
    trace_dir: PathBuf,
    current_file: Mutex<Option<String>>,
}

impl Default for TraceLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceLogger {
    pub fn new() -> Self {
        let trace_dir = app_data_dir().join("traces");
        let _ = fs::create_dir_all(&trace_dir);
        Self {
            trace_dir,
            current_file: Mutex::new(None),
        }
    }

    pub fn write_event(&self, event: &TraceEvent) {
        if let Ok(mut value) = serde_json::to_value(event) {
            redact_sensitive_value(&mut value);
            if let Ok(line) = serde_json::to_string(&value) {
                let mut guard = self.current_file.lock().unwrap();
                let file_name = guard.as_ref().cloned().unwrap_or_else(|| {
                    let name = format!("trace-{}.jsonl", current_timestamp());
                    *guard = Some(name.clone());
                    name
                });

                let file_path = self.trace_dir.join(&file_name);

                // Check file size and rotate if needed
                if let Ok(metadata) = fs::metadata(&file_path) {
                    if metadata.len() > MAX_TRACE_FILE_SIZE {
                        let new_name = format!("trace-{}.jsonl", current_timestamp());
                        *guard = Some(new_name.clone());
                        if let Ok(mut file) = OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(self.trace_dir.join(&new_name))
                        {
                            let _ = writeln!(file, "{}", line);
                        }
                        return;
                    }
                }

                if let Ok(mut file) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&file_path)
                {
                    let _ = writeln!(file, "{}", line);
                }
            }
        }
    }

    pub fn cleanup_old_traces(&self) {
        let now = current_timestamp();
        let cutoff = now - (TRACE_RETENTION_DAYS * 24 * 60 * 60);

        if let Ok(entries) = fs::read_dir(&self.trace_dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        let ts = modified
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        if ts < cutoff {
                            let _ = fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
    }

    pub fn enforce_global_cap(&self) {
        let mut total_size: u64 = 0;
        let mut files: Vec<(PathBuf, u64)> = Vec::new();

        if let Ok(entries) = fs::read_dir(&self.trace_dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    let size = metadata.len();
                    total_size += size;
                    files.push((entry.path(), size));
                }
            }
        }

        if total_size > MAX_GLOBAL_TRACE_SIZE {
            // Sort by modification time (oldest first) and delete until under cap
            files.sort_by_key(|(path, _)| {
                fs::metadata(path)
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH)
            });

            while total_size > MAX_GLOBAL_TRACE_SIZE * 80 / 100 && !files.is_empty() {
                let (path, size) = files.remove(0);
                if fs::remove_file(&path).is_ok() {
                    total_size -= size;
                }
            }
        }
    }

    pub fn trace_dir(&self) -> &PathBuf {
        &self.trace_dir
    }
}

pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn redacted_json_string(value: &serde_json::Value) -> String {
    let mut redacted = value.clone();
    redact_sensitive_value(&mut redacted);
    serde_json::to_string(&redacted).unwrap_or_default()
}

pub fn redacted_text(value: &str) -> String {
    let mut redacted = value.to_string();
    redact_sensitive(&mut redacted);
    redacted
}

const SENSITIVE_KEYS: &[&str] = &[
    "api_key",
    "apiKey",
    "api-key",
    "access_token",
    "accessToken",
    "refresh_token",
    "refreshToken",
    "password",
    "secret",
    "token",
    "old_text",
    "new_text",
    "content",
    "diff_preview",
    "stdout",
    "stderr",
];

/// Token-prefix patterns used by major providers. Add new providers here —
/// this table is the deduplication seam for free-form secret scanning.
/// Patterns use bounded character classes with minimum lengths to avoid
/// over-redacting legitimate short text.
const SECRET_TOKEN_PATTERNS: &[&str] = &[
    r"sk-[A-Za-z0-9_-]{20,}", // OpenAI
    r"sk-ant-[A-Za-z0-9_-]{20,}", // Anthropic
    r"ghp_[A-Za-z0-9]{36,}", // GitHub personal access token
    r"gho_[A-Za-z0-9]{36,}", // GitHub OAuth token
    r"ghs_[A-Za-z0-9]{36,}", // GitHub server-to-server token
];

static KEY_VALUE_RE: LazyLock<Regex> = LazyLock::new(|| {
    let alternation = SENSITIVE_KEYS
        .iter()
        .map(|k| regex::escape(k))
        .collect::<Vec<_>>()
        .join("|");
    Regex::new(&format!(
        r#""({alternation})"\s*:\s*"((?:[^"\\]|\\.)*)""#
    ))
    .expect("key-value regex is valid")
});

static BEARER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(Bearer )(\S{20,})").expect("bearer regex is valid"));

static QUERY_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    let alternation = SENSITIVE_KEYS
        .iter()
        .map(|k| regex::escape(k))
        .collect::<Vec<_>>()
        .join("|");
    Regex::new(&format!(r"([?&])({alternation})=([^&\s]+)"))
        .expect("query token regex is valid")
});

static TOKEN_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    let combined = SECRET_TOKEN_PATTERNS.join("|");
    Regex::new(&combined).expect("token prefix regex is valid")
});

fn redact_sensitive_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if SENSITIVE_KEYS.contains(&key.as_str()) {
                    *child = serde_json::Value::String("[REDACTED]".to_string());
                } else {
                    redact_sensitive_value(child);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                redact_sensitive_value(child);
            }
        }
        serde_json::Value::String(text) => {
            // If the string parses as JSON, recurse and re-serialize. This
            // catches pretty-printed JSON (`"api_key": "…"`) regardless of
            // the leading character, and the substring scan below will catch
            // any JSON embedded in free-form error strings.
            if let Ok(mut nested) = serde_json::from_str::<serde_json::Value>(text) {
                redact_sensitive_value(&mut nested);
                if let Ok(redacted) = serde_json::to_string(&nested) {
                    *text = redacted;
                    return;
                }
            }

            redact_sensitive(text);
        }
        _ => {}
    }
}

fn redact_sensitive(json_str: &mut String) {
    // Whitespace-tolerant JSON key/value scan: matches `"key"\s*:\s*"value"`
    // for every key in SENSITIVE_KEYS, preserving the key and surrounding
    // structure while replacing only the value with the redaction placeholder.
    let mut last_end = 0;
    let mut rebuilt = String::with_capacity(json_str.len());
    let captures: Vec<(usize, usize, String)> = KEY_VALUE_RE
        .captures_iter(json_str)
        .map(|cap| {
            let m = cap.get(0).expect("capture has match");
            let key = cap.get(1).expect("capture has key group").as_str().to_string();
            (m.start(), m.end(), format!("\"{key}\":\"[REDACTED]\""))
        })
        .collect();
    for (start, end, replacement) in captures {
        rebuilt.push_str(&json_str[last_end..start]);
        rebuilt.push_str(&replacement);
        last_end = end;
    }
    rebuilt.push_str(&json_str[last_end..]);
    *json_str = rebuilt;

    // Free-form secret-token scan for HTTP error strings.
    redact_freeform_secrets(json_str);
}

fn redact_freeform_secrets(s: &mut String) {
    // Bearer <token> — keep the `Bearer ` label, redact the token.
    let mut last_end = 0;
    let mut rebuilt = String::with_capacity(s.len());
    let bearer_caps: Vec<(usize, usize, String)> = BEARER_RE
        .captures_iter(s)
        .map(|cap| {
            let m = cap.get(0).expect("bearer capture has match");
            let label = cap.get(1).expect("bearer label group").as_str();
            (m.start(), m.end(), format!("{label}[REDACTED]"))
        })
        .collect();
    for (start, end, replacement) in bearer_caps {
        rebuilt.push_str(&s[last_end..start]);
        rebuilt.push_str(&replacement);
        last_end = end;
    }
    rebuilt.push_str(&s[last_end..]);
    *s = rebuilt;

    // query-string-style `?key=…` / `&key=…` — preserve the leading delimiter.
    let mut last_end = 0;
    let mut rebuilt = String::with_capacity(s.len());
    let q_caps: Vec<(usize, usize, String)> = QUERY_TOKEN_RE
        .captures_iter(s)
        .map(|cap| {
            let m = cap.get(0).expect("query capture has match");
            let key = cap.get(2).expect("query key group").as_str().to_string();
            // group 1 is the delimiter (? or &); preserve it in the output.
            let delim = cap.get(1).expect("query delimiter group").as_str();
            (m.start(), m.end(), format!("{delim}{key}=[REDACTED]"))
        })
        .collect();
    for (start, end, replacement) in q_caps {
        rebuilt.push_str(&s[last_end..start]);
        rebuilt.push_str(&replacement);
        last_end = end;
    }
    rebuilt.push_str(&s[last_end..]);
    *s = rebuilt;

    // Provider token-prefix patterns (sk-…, ghp_…, gho_…, ghs_…, sk-ant-…).
    let mut last_end = 0;
    let mut rebuilt = String::with_capacity(s.len());
    let t_caps: Vec<(usize, usize)> = TOKEN_PREFIX_RE
        .find_iter(s)
        .map(|m| (m.start(), m.end()))
        .collect();
    for (start, end) in t_caps {
        rebuilt.push_str(&s[last_end..start]);
        rebuilt.push_str("[REDACTED]");
        last_end = end;
    }
    rebuilt.push_str(&s[last_end..]);
    *s = rebuilt;
}

pub struct TraceState {
    pub logger: Option<TraceLogger>,
}

impl Default for TraceState {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceState {
    pub fn new() -> Self {
        Self { logger: None }
    }

    pub fn init(&mut self) {
        let logger = TraceLogger::new();
        logger.cleanup_old_traces();
        logger.enforce_global_cap();
        self.logger = Some(logger);
    }

    pub fn write_event(&self, event: &TraceEvent) {
        if let Some(ref logger) = self.logger {
            logger.write_event(event);
        }
    }
}

fn app_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("gospel")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_api_key_from_json() {
        let mut s = r#"{"api_key":"sk-1234567890","other":"value"}"#.to_string();
        redact_sensitive(&mut s);
        assert!(s.contains("[REDACTED]"));
        assert!(!s.contains("sk-1234567890"));
    }

    #[test]
    fn redacts_oauth_token() {
        let mut s = r#"{"access_token":"token123"}"#.to_string();
        redact_sensitive(&mut s);
        assert!(s.contains("[REDACTED]"));
        assert!(!s.contains("token123"));
    }

    #[test]
    fn redacts_nested_json_strings() {
        let mut value = serde_json::json!({
            "type": "tool_call",
            "arguments_redacted": "{\"api_key\":\"sk-nested\",\"query\":\"ok\"}"
        });

        redact_sensitive_value(&mut value);
        let serialized = serde_json::to_string(&value).unwrap();

        assert!(serialized.contains("[REDACTED]"));
        assert!(!serialized.contains("sk-nested"));
    }

    #[test]
    fn redacted_json_string_preserves_safe_arguments() {
        let arguments = serde_json::json!({
            "path": "src/lib.rs",
            "api_key": "sk-secret"
        });

        let redacted = redacted_json_string(&arguments);
        let value: serde_json::Value = serde_json::from_str(&redacted).unwrap();

        assert_eq!(value["path"], "src/lib.rs");
        assert_eq!(value["api_key"], "[REDACTED]");
        assert!(!redacted.contains("sk-secret"));
    }

    #[test]
    fn redacted_json_string_redacts_source_edit_snippets() {
        let arguments = serde_json::json!({
            "path": "src/lib.rs",
            "old_text": "raw old",
            "new_text": "raw new"
        });

        let redacted = redacted_json_string(&arguments);
        let value: serde_json::Value = serde_json::from_str(&redacted).unwrap();

        assert_eq!(value["path"], "src/lib.rs");
        assert_eq!(value["old_text"], "[REDACTED]");
        assert_eq!(value["new_text"], "[REDACTED]");
        assert!(!redacted.contains("raw old"));
        assert!(!redacted.contains("raw new"));
    }

    #[test]
    fn redacts_pretty_printed_json_with_whitespace() {
        let mut s = "{\"api_key\": \"sk-REDACTEDFAKE123456789\", \"other\": \"value\"}"
            .to_string();
        redact_sensitive(&mut s);
        assert!(s.contains("[REDACTED]"));
        assert!(!s.contains("sk-REDACTEDFAKE123456789"));
        assert!(s.contains("\"other\""));
    }

    #[test]
    fn redacts_pretty_printed_json_in_freeform_string() {
        let mut s = "Got error: { \"api_key\": \"sk-REDACTEDFAKE123456789\" }".to_string();
        redact_sensitive(&mut s);
        assert!(s.contains("[REDACTED]"));
        assert!(!s.contains("sk-REDACTEDFAKE123456789"));
    }

    #[test]
    fn redacts_bearer_token_in_error_message() {
        let mut s =
            "HTTP 401: Authorization: Bearer ghp_REDACTEDFAKE0123456789ABCDEFGHIJKLMN".to_string();
        redact_sensitive(&mut s);
        assert!(s.contains("Bearer "));
        assert!(s.contains("[REDACTED]"));
        assert!(!s.contains("ghp_REDACTEDFAKE0123456789ABCDEFGHIJKLMN"));
    }

    #[test]
    fn redacts_openai_key_in_query_string() {
        let mut s = "GET /v1/completions?key=sk-REDACTEDFAKE123456789XYZ failed".to_string();
        redact_sensitive(&mut s);
        assert!(s.contains("[REDACTED]"));
        assert!(!s.contains("sk-REDACTEDFAKE123456789XYZ"));
    }

    #[test]
    fn redacts_github_oauth_token() {
        let mut s = "gho_REDACTEDFAKE0123456789ABCDEFGHIJKLMN".to_string();
        redact_sensitive(&mut s);
        assert!(s.contains("[REDACTED]"));
        assert!(!s.contains("gho_REDACTEDFAKE0123456789ABCDEFGHIJKLMN"));
    }

    #[test]
    fn does_not_redact_short_non_secret_strings() {
        let mut s = "Bearer abc".to_string();
        redact_sensitive(&mut s);
        assert_eq!(s, "Bearer abc");
    }
}
