//! Loading of persisted Model History.
//!
//! Sessions saved before the rig 0.44 upgrade store rig 0.39 `Message` JSON,
//! which the current types cannot read. `parse_model_history` reads the
//! current shape first and falls back to rewriting the legacy shape. Items
//! that still cannot be read after rewriting are dropped, so one bad entry
//! does not cost the session its whole history.

use rig::completion::message::Message;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Used when a legacy tool result has no matching tool call to name it.
const UNKNOWN_TOOL_NAME: &str = "unknown_tool";

pub(crate) fn parse_model_history(json: &str) -> Result<Vec<Message>, serde_json::Error> {
    let value: Value = serde_json::from_str(json)?;
    match serde_json::from_value::<Vec<Message>>(value.clone()) {
        Ok(messages) => Ok(messages),
        Err(current_error) => {
            let Value::Array(messages) = value else {
                return Err(current_error);
            };
            salvage_migrated(migrate_legacy_messages(messages), current_error)
        }
    }
}

/// Deserializes rewritten messages one at a time, keeping every message that
/// reads and dropping the rest. Returns the current-format error when nothing
/// survives, so the caller still sees why hydration failed.
fn salvage_migrated(
    migrated: Vec<Value>,
    current_error: serde_json::Error,
) -> Result<Vec<Message>, serde_json::Error> {
    let total = migrated.len();
    let mut messages = Vec::with_capacity(total);
    let mut first_error = None;
    for message in migrated {
        match serde_json::from_value::<Message>(message) {
            Ok(message) => messages.push(message),
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    match first_error {
        Some(error) if messages.is_empty() => {
            tracing::warn!(error = %error, "no model history message survived migration");
            Err(current_error)
        }
        Some(error) => {
            tracing::warn!(
                error = %error,
                dropped = total - messages.len(),
                "dropped unparseable model history message(s) during migration"
            );
            Ok(messages)
        }
        None => Ok(messages),
    }
}

fn migrate_legacy_messages(messages: Vec<Value>) -> Vec<Value> {
    let mut tool_names: HashMap<String, String> = HashMap::new();
    let mut migrated = Vec::with_capacity(messages.len());
    for message in messages {
        let Value::Object(mut message) = message else {
            migrated.push(message);
            continue;
        };
        match message.get("role").and_then(Value::as_str) {
            Some("assistant") => {
                let content: Vec<Value> = into_items(message.remove("content"))
                    .into_iter()
                    .filter_map(|item| migrate_assistant_item(item, &mut tool_names))
                    .collect();
                // Providers reject empty assistant turns; a legacy turn that
                // only held reasoning has nothing left to replay.
                if content.is_empty() {
                    continue;
                }
                migrated.push(json!({"role": "assistant", "content": content}));
            }
            Some("user") => {
                let content: Vec<Value> = into_items(message.remove("content"))
                    .into_iter()
                    .map(|item| migrate_user_item(item, &tool_names))
                    .collect();
                migrated.push(json!({"role": "user", "content": content}));
            }
            _ => migrated.push(Value::Object(message)),
        }
    }
    migrated
}

/// rig 0.39 `OneOrMany` serialized as an array but also accepted one object.
fn into_items(content: Option<Value>) -> Vec<Value> {
    match content {
        Some(Value::Array(items)) => items,
        Some(Value::Null) | None => Vec::new(),
        Some(item) => vec![item],
    }
}

/// rig 0.39 assistant content was untagged, so the variant is inferred from
/// its fields.
fn migrate_assistant_item(item: Value, tool_names: &mut HashMap<String, String>) -> Option<Value> {
    let Value::Object(item) = item else {
        return None;
    };
    if item.contains_key("type") {
        return Some(Value::Object(item));
    }
    if let Some(function) = item.get("function").and_then(Value::as_object) {
        let call_id = legacy_call_key(&item)?;
        let name = function.get("name").and_then(Value::as_str)?.to_string();
        let arguments = function.get("arguments").cloned().unwrap_or(Value::Null);
        tool_names.insert(call_id.clone(), name.clone());
        return Some(json!({
            "type": "toolcall",
            "id": {"provider": call_id},
            "function": {"name": name, "arguments": arguments},
        }));
    }
    if let Some(text) = item.get("text").and_then(Value::as_str) {
        return Some(json!({"type": "text", "text": text}));
    }
    // Legacy reasoning carried provider signatures and encrypted payloads the
    // current types cannot hold; replaying it unsigned can be rejected, and
    // providers never require prior-turn reasoning, so it is dropped.
    if item.contains_key("content") {
        return None;
    }
    if item.contains_key("data") {
        let mut image = item;
        image.insert("type".to_string(), json!("image"));
        return Some(Value::Object(image));
    }
    None
}

fn migrate_user_item(item: Value, tool_names: &HashMap<String, String>) -> Value {
    let Value::Object(mut item) = item else {
        return item;
    };
    match item.get("type").and_then(Value::as_str) {
        Some("toolresult") if !item.contains_key("call") => {
            let Some(call_id) = legacy_call_key(&item) else {
                return Value::Object(item);
            };
            let name = tool_names
                .get(&call_id)
                .cloned()
                .unwrap_or_else(|| UNKNOWN_TOOL_NAME.to_string());
            let content: Vec<Value> = into_items(item.remove("content"))
                .into_iter()
                .map(strip_text_extras)
                .collect();
            json!({
                "type": "toolresult",
                "call": {"provider": call_id},
                "name": name,
                "content": content,
            })
        }
        Some("text") => strip_text_extras(Value::Object(item)),
        _ => Value::Object(item),
    }
}

/// rig 0.39 flattened `additional_params` into text blocks.
fn strip_text_extras(item: Value) -> Value {
    match item.get("type").and_then(Value::as_str) {
        Some("text") => match item.get("text") {
            Some(text) => json!({"type": "text", "text": text}),
            None => item,
        },
        _ => item,
    }
}

/// Some providers (OpenAI Responses) correlate results through `call_id`
/// rather than the item `id`; rig 0.44 keeps only the correlating id.
fn legacy_call_key(item: &Map<String, Value>) -> Option<String> {
    ["call_id", "id"]
        .iter()
        .filter_map(|key| item.get(*key).and_then(Value::as_str))
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::completion::message::{AssistantContent, ToolResultContent, UserContent};

    fn assistant_content(message: &Message) -> &[AssistantContent] {
        let Message::Assistant(message) = message else {
            panic!("expected assistant message: {message:?}");
        };
        &message.content
    }

    #[test]
    fn current_history_parses_unchanged() {
        let original = vec![Message::user("hi"), Message::assistant("hello back")];
        let json = serde_json::to_string(&original).unwrap();

        assert_eq!(parse_model_history(&json).unwrap(), original);
    }

    #[test]
    fn legacy_text_history_migrates() {
        let legacy = r#"[
            {"role":"user","content":[{"type":"text","text":"hi"}]},
            {"role":"assistant","id":null,"content":[{"text":"hello back"}]}
        ]"#;

        let messages = parse_model_history(legacy).unwrap();

        assert_eq!(
            messages,
            vec![Message::user("hi"), Message::assistant("hello back")]
        );
    }

    #[test]
    fn legacy_tool_call_and_result_keep_correlation_and_name() {
        let legacy = r#"[
            {"role":"user","content":[{"type":"text","text":"read a"}]},
            {"role":"assistant","id":"msg_1","content":[
                {"text":"Looking"},
                {"id":"fc_1","call_id":"call_1","function":{"name":"read_file","arguments":{"path":"a"}},"signature":null,"additional_params":null}
            ]},
            {"role":"user","content":[
                {"type":"toolresult","id":"fc_1","call_id":"call_1","content":[{"type":"text","text":"ok"}]}
            ]},
            {"role":"assistant","id":null,"content":[
                {"id":"toolu_2","call_id":null,"function":{"name":"search_code","arguments":{}},"signature":null,"additional_params":null}
            ]},
            {"role":"user","content":{"type":"toolresult","id":"toolu_2","content":{"type":"text","text":"none"}}}
        ]"#;

        let messages = parse_model_history(legacy).unwrap();

        assert_eq!(messages.len(), 5);
        let [AssistantContent::Text(text), AssistantContent::ToolCall(call)] =
            assistant_content(&messages[1])
        else {
            panic!("expected text then tool call: {:?}", messages[1]);
        };
        assert_eq!(text.text, "Looking");
        assert_eq!(call.id.to_string(), "call_1");
        assert_eq!(call.function.name.as_str(), "read_file");
        assert_eq!(call.function.arguments_value(), json!({"path": "a"}));

        let Message::User { content } = &messages[2] else {
            panic!("expected user message");
        };
        let [UserContent::ToolResult(result)] = content.as_slice() else {
            panic!("expected tool result: {content:?}");
        };
        assert_eq!(result.call, call.id);
        assert_eq!(result.name.as_str(), "read_file");
        assert_eq!(result.content, vec![ToolResultContent::text("ok")]);

        let Message::User { content } = &messages[4] else {
            panic!("expected user message");
        };
        let [UserContent::ToolResult(result)] = content.as_slice() else {
            panic!("expected tool result: {content:?}");
        };
        assert_eq!(result.call.to_string(), "toolu_2");
        assert_eq!(result.name.as_str(), "search_code");
    }

    #[test]
    fn legacy_reasoning_is_dropped_and_reasoning_only_turns_removed() {
        let legacy = r#"[
            {"role":"user","content":[{"type":"text","text":"think"}]},
            {"role":"assistant","id":null,"content":[
                {"id":"rs_1","content":[{"type":"encrypted","content":"cipher"}]}
            ]},
            {"role":"assistant","id":null,"content":[
                {"id":null,"content":[{"type":"text","content":{"text":"plan","signature":"sig"}}]},
                {"text":"answer"}
            ]}
        ]"#;

        let messages = parse_model_history(legacy).unwrap();

        assert_eq!(
            messages,
            vec![Message::user("think"), Message::assistant("answer")]
        );
    }

    #[test]
    fn unmatched_legacy_tool_result_gets_placeholder_name() {
        let legacy = r#"[
            {"role":"user","content":[{"type":"toolresult","id":"orphan","content":[{"type":"text","text":"x"}]}]}
        ]"#;

        let messages = parse_model_history(legacy).unwrap();

        let Message::User { content } = &messages[0] else {
            panic!("expected user message");
        };
        let [UserContent::ToolResult(result)] = content.as_slice() else {
            panic!("expected tool result: {content:?}");
        };
        assert_eq!(result.name.as_str(), UNKNOWN_TOOL_NAME);
    }

    #[test]
    fn malformed_history_is_an_error() {
        assert!(parse_model_history("not json").is_err());
        assert!(parse_model_history(r#"{"role":"user"}"#).is_err());
    }

    #[test]
    fn unmigratable_items_are_dropped_not_the_whole_history() {
        // A legacy tool result with no id or call_id cannot be rewritten;
        // the messages around it must still load.
        let legacy = r#"[
            {"role":"user","content":[{"type":"text","text":"hi"}]},
            {"role":"user","content":[{"type":"toolresult","content":[{"type":"text","text":"x"}]}]},
            {"role":"assistant","id":null,"content":[{"text":"hello back"}]}
        ]"#;

        let messages = parse_model_history(legacy).unwrap();

        assert_eq!(
            messages,
            vec![Message::user("hi"), Message::assistant("hello back")]
        );
    }

    #[test]
    fn history_with_nothing_migratable_is_an_error() {
        let legacy = r#"[
            {"role":"user","content":[{"type":"toolresult","content":[{"type":"text","text":"x"}]}]}
        ]"#;

        assert!(parse_model_history(legacy).is_err());
    }
}
