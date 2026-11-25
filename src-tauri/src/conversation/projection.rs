use crate::text_utils;
use crate::trace;

/// Slices a Display Transcript JSON array into a `tail`-sized window ending at
/// `before` (exclusive; defaults to the end). Returns `(page_json, start, total)`.
pub(crate) fn slice_display_transcript(
    raw: &str,
    tail: Option<u32>,
    before: Option<u32>,
) -> (String, usize, usize) {
    let entries = parse_display_transcript(raw);
    let total = entries.len();
    let end = before.map(|b| (b as usize).min(total)).unwrap_or(total);
    let start = tail
        .map(|t| end.saturating_sub(t as usize))
        .unwrap_or(0);
    let page =
        serde_json::to_string(&entries[start..end]).unwrap_or_else(|_| "[]".to_string());
    (page, start, total)
}

const DEBUG_EXPORT_METADATA_MAX_BYTES: usize = 200;

pub(crate) fn project_debug_metadata(value: &str) -> String {
    let redacted = trace::redacted_text(value);
    text_utils::truncate_text_bytes(&redacted, DEBUG_EXPORT_METADATA_MAX_BYTES).0
}

/// Parse the persisted Display Transcript JSON into an array of entries. A
/// malformed or legacy payload yields an empty array rather than the raw
/// string, so exports never fall back to returning unprojected input.
pub(crate) fn parse_display_transcript(raw: &str) -> Vec<serde_json::Value> {
    serde_json::from_str::<Vec<serde_json::Value>>(raw).unwrap_or_default()
}

fn project_shared_export_fields(
    entry: &serde_json::Map<String, serde_json::Value>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let role = entry.get("role").and_then(|value| value.as_str())?;
    if role != "user" && role != "assistant" {
        return None;
    }

    let mut projected = serde_json::Map::new();
    projected.insert(
        "role".to_string(),
        serde_json::Value::String(role.to_string()),
    );
    if let Some(content) = entry.get("content").and_then(|value| value.as_str()) {
        projected.insert(
            "content".to_string(),
            serde_json::Value::String(content.to_string()),
        );
    }
    if let Some(error) = entry.get("error").and_then(|value| value.as_bool()) {
        projected.insert("error".to_string(), serde_json::Value::Bool(error));
    }
    if let Some(controlled_stop) = entry
        .get("controlled_stop")
        .and_then(|value| value.as_bool())
    {
        projected.insert(
            "controlled_stop".to_string(),
            serde_json::Value::Bool(controlled_stop),
        );
    }
    Some(projected)
}

/// Safe-to-share Transcript projection. Preserves only user/assistant text and
/// existing user-visible error/controlled-stop status metadata. Removes tool
/// blocks, tool IDs, tool names, arguments, and raw tool results.
pub(crate) fn project_transcript(raw: &str) -> String {
    let entries = parse_display_transcript(raw);
    let projected: Vec<serde_json::Value> = entries
        .iter()
        .filter_map(|entry| {
            let obj = entry.as_object()?;
            project_shared_export_fields(obj).map(serde_json::Value::Object)
        })
        .collect();
    serde_json::to_string(&projected).unwrap_or_else(|_| "[]".to_string())
}

/// Bounded diagnostic Debug projection. Preserves the ordered action shape for
/// diagnostics but never exports raw tool arguments or result bodies. Tool
/// identity (name, id), status, and a bounded result-length marker are kept.
pub(crate) fn project_debug(raw: &str) -> String {
    let entries = parse_display_transcript(raw);
    let projected: Vec<serde_json::Value> = entries
        .iter()
        .filter_map(|entry| {
            let obj = entry.as_object()?;
            let mut out = project_shared_export_fields(obj)?;
            if let Some(blocks) = obj.get("blocks").and_then(|b| b.as_array()) {
                let debug_blocks: Vec<serde_json::Value> = blocks
                    .iter()
                    .filter_map(|block| {
                        let b_obj = block.as_object()?;
                        let mut dbg = serde_json::Map::new();
                        if let Some(kind) = b_obj.get("kind").and_then(|v| v.as_str()) {
                            dbg.insert(
                                "kind".to_string(),
                                serde_json::Value::String(project_debug_metadata(kind)),
                            );
                        }
                        if let Some(id) = b_obj.get("id").and_then(|v| v.as_str()) {
                            dbg.insert(
                                "id".to_string(),
                                serde_json::Value::String(project_debug_metadata(id)),
                            );
                        }
                        if let Some(name) = b_obj.get("name").and_then(|v| v.as_str()) {
                            dbg.insert(
                                "name".to_string(),
                                serde_json::Value::String(project_debug_metadata(name)),
                            );
                        }
                        if let Some(status) = b_obj.get("status").and_then(|v| v.as_str()) {
                            dbg.insert(
                                "status".to_string(),
                                serde_json::Value::String(project_debug_metadata(status)),
                            );
                        }
                        if let Some(result) = b_obj.get("result").and_then(|v| v.as_str()) {
                            let byte_len = result.len();
                            dbg.insert(
                                "result".to_string(),
                                serde_json::Value::String(format!("[redacted: {byte_len} bytes]")),
                            );
                        }
                        if dbg.is_empty() {
                            None
                        } else {
                            Some(serde_json::Value::Object(dbg))
                        }
                    })
                    .collect();
                out.insert("blocks".to_string(), serde_json::Value::Array(debug_blocks));
            }
            Some(serde_json::Value::Object(out))
        })
        .collect();
    serde_json::to_string(&projected).unwrap_or_else(|_| "[]".to_string())
}
