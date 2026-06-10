use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
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
];

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
            let trimmed = text.trim_start();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                if let Ok(mut nested) = serde_json::from_str::<serde_json::Value>(text) {
                    redact_sensitive_value(&mut nested);
                    if let Ok(redacted) = serde_json::to_string(&nested) {
                        *text = redacted;
                        return;
                    }
                }
            }

            redact_sensitive(text);
        }
        _ => {}
    }
}

fn redact_sensitive(json_str: &mut String) {
    for pattern in SENSITIVE_KEYS {
        let search = format!("\"{}\":\"", pattern);
        let mut search_from = 0;
        while let Some(relative_start) = json_str[search_from..].find(&search) {
            let start = search_from + relative_start;
            let value_start = start + search.len();
            if let Some(end) = json_str[value_start..].find('"') {
                let redacted = format!("\"{}\":\"[REDACTED]\"", pattern);
                json_str.replace_range(start..value_start + end, &redacted);
                search_from = start + redacted.len();
            } else {
                break;
            }
        }
    }
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
}
