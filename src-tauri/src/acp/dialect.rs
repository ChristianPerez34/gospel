//! Factory Droid dialect types for ACP payloads.
//!
//! Droid 1.21.x speaks an older session-config dialect than the current ACP
//! schema (plain-string `currentValue`, `"type": "select"` config options) and
//! exposes Factory extensions through `_meta`. The typed SDK types cover the
//! stable core protocol; these types cover the payloads where Droid's dialect
//! diverges, parsed from raw JSON sent through `UntypedMessage` requests.

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialectConfigChoice {
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialectConfigGroup {
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub options: Vec<DialectConfigChoice>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialectConfigOption {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default, rename = "type")]
    pub option_type: String,
    #[serde(default)]
    pub current_value: Value,
    #[serde(default)]
    pub options: Vec<DialectConfigChoice>,
    #[serde(default)]
    pub groups: Vec<DialectConfigGroup>,
}

impl DialectConfigOption {
    pub fn all_choices(&self) -> Vec<&DialectConfigChoice> {
        self.options
            .iter()
            .chain(self.groups.iter().flat_map(|g| g.options.iter()))
            .collect()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialectModel {
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "_meta")]
    pub meta: Option<Value>,
}

impl DialectModel {
    /// Per-model reasoning efforts advertised through Factory's `_meta`
    /// extension: `_meta.factory.supportedReasoningEfforts`.
    pub fn supported_reasoning_efforts(&self) -> Vec<String> {
        self.meta
            .as_ref()
            .and_then(|meta| meta.get("factory"))
            .and_then(|factory| factory.get("supportedReasoningEfforts"))
            .and_then(Value::as_array)
            .map(|efforts| {
                efforts
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialectModelsMetadata {
    #[serde(default)]
    pub current_model_id: Option<String>,
    #[serde(default)]
    pub available_models: Vec<DialectModel>,
}

/// Parsed result of `session/new`, `session/load`, or `session/fork` in
/// Droid's dialect. `session/load` returns no session id (the requested id is
/// reused); callers fill it in.
#[derive(Debug, Clone, Default)]
pub struct DialectSessionSetup {
    pub session_id: Option<String>,
    pub config_options: Vec<DialectConfigOption>,
    pub models: Option<DialectModelsMetadata>,
}

impl DialectSessionSetup {
    pub fn parse(value: &Value) -> Self {
        let config_options = value
            .get("configOptions")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|option| serde_json::from_value::<DialectConfigOption>(option.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();
        let models = value
            .get("models")
            .filter(|models| !models.is_null())
            .and_then(|models| serde_json::from_value::<DialectModelsMetadata>(models.clone()).ok());
        let session_id = value
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string);
        Self {
            session_id,
            config_options,
            models,
        }
    }

    pub fn config_option(&self, id_or_category: &str) -> Option<&DialectConfigOption> {
        self.config_options
            .iter()
            .find(|opt| opt.category.as_deref() == Some(id_or_category) || opt.id == id_or_category)
    }
}

/// Builds the params for `session/new`, `session/load`, or `session/fork` in
/// the shape Droid expects.
pub fn session_scope_params(session_id: Option<&str>, cwd: &str) -> Value {
    let mut params = serde_json::json!({
        "cwd": cwd,
        "mcpServers": Vec::<Value>::new(),
    });
    if let Some(id) = session_id {
        params["sessionId"] = Value::String(id.to_string());
    }
    params
}

/// Builds the params for `session/set_config_option` in Droid's dialect: the
/// value travels as a plain string/boolean rather than the tagged union the
/// current schema serializes.
pub fn set_config_option_params(session_id: &str, config_id: &str, value: &str) -> Value {
    serde_json::json!({
        "sessionId": session_id,
        "configId": config_id,
        "value": value,
    })
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialectAuthStatus {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub method_id: Option<String>,
}

impl DialectAuthStatus {
    pub fn is_authenticated(&self) -> bool {
        self.state.eq_ignore_ascii_case("authenticated")
    }
}

/// Auth prompt extracted from a JSON-RPC payload or stderr diagnostic during `authenticate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractedAuthChallenge {
    DeviceCode {
        verification_url: String,
        user_code: String,
    },
    Browser {
        authorize_url: String,
    },
}

pub fn extract_auth_challenge_from_value(value: &Value) -> Option<ExtractedAuthChallenge> {
    let url = find_string_field(
        value,
        &[
            "verificationUriComplete",
            "verification_uri_complete",
            "verificationUri",
            "verification_uri",
            "authorizeUrl",
            "authorize_url",
            "verificationUrl",
            "verification_url",
            "authUrl",
            "auth_url",
            "url",
        ],
    )?;
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return None;
    }
    if let Some(code) = find_string_field(value, &["userCode", "user_code", "pairingCode", "code"])
    {
        if !code.is_empty() && code.len() <= 32 {
            return Some(ExtractedAuthChallenge::DeviceCode {
                verification_url: url,
                user_code: code,
            });
        }
    }
    Some(ExtractedAuthChallenge::Browser { authorize_url: url })
}

pub fn extract_auth_challenge_from_text(line: &str) -> Option<ExtractedAuthChallenge> {
    let url = line
        .split_whitespace()
        .map(|tok| tok.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | '.' | ')' | '(' | '<' | '>')))
        .find(|tok| tok.starts_with("https://") || tok.starts_with("http://"))?
        .to_string();
    let user_code = line
        .split_whitespace()
        .map(|tok| tok.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | '.' | ')' | '(' | ':')))
        .find(|tok| is_device_code_token(tok))
        .map(str::to_string);
    match user_code {
        Some(code) => Some(ExtractedAuthChallenge::DeviceCode {
            verification_url: url,
            user_code: code,
        }),
        None => Some(ExtractedAuthChallenge::Browser { authorize_url: url }),
    }
}

fn is_device_code_token(token: &str) -> bool {
    let parts: Vec<&str> = token.split('-').collect();
    parts.len() == 2
        && parts.iter().all(|p| {
            (3..=6).contains(&p.len())
                && p.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
}

fn find_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(s) = map.get(*key).and_then(Value::as_str) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            for child in map.values() {
                if let Some(found) = find_string_field(child, keys) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_string_field(item, keys)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_droid_session_new_response() {
        let value = json!({
            "sessionId": "sess-1",
            "configOptions": [
                {"id": "model", "category": "model", "type": "select", "currentValue": "auto",
                 "options": [{"value": "auto", "name": "Auto"}, {"value": "claude-opus-5-5", "name": "Opus"}]},
                {"id": "autonomy_level", "category": "mode", "type": "select", "currentValue": "auto-medium",
                 "options": [{"value": "normal", "name": "Auto (Off)"}]}
            ],
            "models": {"currentModelId": "auto", "availableModels": [
                {"modelId": "auto", "name": "Auto", "_meta": {"factory": {"supportedReasoningEfforts": ["none"]}}}
            ]}
        });
        let setup = DialectSessionSetup::parse(&value);
        assert_eq!(setup.session_id.as_deref(), Some("sess-1"));
        assert_eq!(setup.config_options.len(), 2);
        let model_opt = setup.config_option("model").unwrap();
        assert_eq!(model_opt.current_value.as_str(), Some("auto"));
        assert_eq!(model_opt.all_choices().len(), 2);
        let models = setup.models.unwrap();
        assert_eq!(models.available_models.len(), 1);
        assert_eq!(models.available_models[0].supported_reasoning_efforts(), vec!["none"]);
    }

    #[test]
    fn parses_session_load_response_without_session_id() {
        let value = json!({"configOptions": [], "modes": null});
        let setup = DialectSessionSetup::parse(&value);
        assert!(setup.session_id.is_none());
    }

    #[test]
    fn builds_droid_dialect_config_value() {
        let params = set_config_option_params("s1", "model", "auto");
        assert_eq!(params["value"], json!("auto"));
    }

    #[test]
    fn extracts_device_code_challenge() {
        let challenge = extract_auth_challenge_from_value(&json!({
            "verificationUri": "https://app.factory.ai/device", "userCode": "ABCD-1234"
        }));
        assert_eq!(
            challenge,
            Some(ExtractedAuthChallenge::DeviceCode {
                verification_url: "https://app.factory.ai/device".into(),
                user_code: "ABCD-1234".into(),
            })
        );
    }
}
