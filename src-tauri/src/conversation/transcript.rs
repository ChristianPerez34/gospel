use crate::llm::LlmError;
use crate::trace::observable_tool_arguments;
use rig::completion::message::{AssistantContent, Message, ToolResultContent, UserContent};
use serde_json::json;

/// Shown and persisted when a completed chat turn has no assistant text.
pub(crate) const EMPTY_RESPONSE_FALLBACK: &str = "The agent ended this turn without a text response.";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SuccessfulTurnPersistence {
    pub(crate) display_transcript: String,
    pub(crate) model_history: Option<String>,
    pub(crate) history: Vec<Message>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FailureTurnPersistence {
    pub(crate) display_transcript: String,
    pub(crate) model_history: Option<String>,
    /// The updated Model History, or `None` when it could not be parsed and
    /// the persisted copy was left unchanged.
    pub(crate) history: Option<Vec<Message>>,
}

/// A turn that ended in a provider error or Controlled Stop.
pub(crate) struct FailedTurn<'a> {
    /// The prompt as the user typed it, for the Display Transcript.
    pub(crate) display_prompt: &'a str,
    /// The prompt as sent to the model, for Model History.
    pub(crate) model_prompt: &'a str,
    pub(crate) error: &'a LlmError,
}

fn transcript_entry_role(entry: &serde_json::Value) -> Option<&str> {
    entry.get("role").and_then(|role| role.as_str())
}

fn transcript_entry_flag(entry: &serde_json::Value, flag: &str) -> bool {
    entry.get(flag).and_then(|value| value.as_bool()) == Some(true)
}

/// Set on a failure row whose prompt was appended to Model History. Failure
/// rows written before ADR-0017 lack it: their prompt is display-only.
const PROMPT_IN_MODEL_HISTORY: &str = "prompt_in_model_history";

fn is_terminal_failure_entry(entry: &serde_json::Value) -> bool {
    transcript_entry_flag(entry, "error") || transcript_entry_flag(entry, "controlled_stop")
}

/// How a persisted Display Transcript turn relates to Model History.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptTurnKind {
    /// A completed turn; the rebuild regenerates it.
    Completed,
    /// A failed turn whose prompt is in Model History. The rebuild re-covers
    /// it as a bare prompt, so the persisted turn (with its failure row) wins.
    FailedInModelHistory,
    /// A legacy failed turn that never reached Model History.
    FailedDisplayOnly,
}

fn transcript_turn_kind(turn: &[serde_json::Value]) -> TranscriptTurnKind {
    match turn.iter().find(|entry| is_terminal_failure_entry(entry)) {
        None => TranscriptTurnKind::Completed,
        Some(failure) if transcript_entry_flag(failure, PROMPT_IN_MODEL_HISTORY) => {
            TranscriptTurnKind::FailedInModelHistory
        }
        Some(_) => TranscriptTurnKind::FailedDisplayOnly,
    }
}

/// Each turn's span: a user entry up to (not including) the next one. Entries
/// before the first user entry belong to no turn.
fn user_turn_spans(entries: &[serde_json::Value]) -> Vec<std::ops::Range<usize>> {
    let starts: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (transcript_entry_role(entry) == Some("user")).then_some(index)
        })
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(position, &start)| start..starts.get(position + 1).copied().unwrap_or(entries.len()))
        .collect()
}

/// An assistant entry rebuilt from a tool result whose call was pruned from
/// Model History: empty content and a single unnamed tool block.
fn is_orphan_transcript_block(entry: &serde_json::Value) -> bool {
    if transcript_entry_role(entry) != Some("assistant")
        || !entry
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .is_empty()
    {
        return false;
    }
    if let Some(blocks) = entry.get("blocks").and_then(|b| b.as_array()) {
        if blocks.len() == 1 {
            return blocks[0]
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .is_empty();
        }
    }
    false
}

/// Drop everything before the first user entry — a transcript's initiating
/// user message is always item 0 — and remove orphan tool-result residue.
fn sanitize_transcript_entries(mut entries: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    if let Some(first_user) = entries
        .iter()
        .position(|entry| transcript_entry_role(entry) == Some("user"))
    {
        entries.drain(..first_user);
    }
    entries.retain(|entry| !is_orphan_transcript_block(entry));
    entries
}

/// Merge a freshly rebuilt Display Transcript with the persisted one.
///
/// Model History is pruned on a token budget, so the rebuild only covers a
/// suffix of the conversation while the persisted transcript records
/// everything the user saw. The overlap is positional rather than
/// content-matched: the rebuild's last user entry is the prompt whose turn
/// just completed, so its earlier user entries re-cover exactly the last
/// persisted turns whose prompts are in Model History. Identical repeated
/// prompts therefore cannot merge wrongly. In the overlap, completed turns
/// take the rebuilt entries; failed turns keep their persisted entries (the
/// rebuild has only their bare prompt); legacy display-only failures stay
/// where they were without consuming a rebuilt turn.
pub(crate) fn merge_display_transcripts(existing_json: &str, new_json: &str) -> String {
    let existing: Vec<serde_json::Value> = serde_json::from_str(existing_json).unwrap_or_default();
    let new_entries: Vec<serde_json::Value> = serde_json::from_str(new_json).unwrap_or_default();

    let rebuilt_turns = user_turn_spans(&new_entries);
    if rebuilt_turns.is_empty() {
        // The rebuild produced no user entry; keep history and append any
        // non-orphan assistant entries it did produce.
        let mut merged = sanitize_transcript_entries(existing);
        merged.extend(new_entries.into_iter().filter(|entry| {
            transcript_entry_role(entry) == Some("assistant") && !is_orphan_transcript_block(entry)
        }));
        return serde_json::to_string(&merged).unwrap_or_else(|_| "[]".to_string());
    }
    if existing.is_empty() {
        return serde_json::to_string(&sanitize_transcript_entries(new_entries))
            .unwrap_or_else(|_| "[]".to_string());
    }

    let rebuilt_prior_turns = rebuilt_turns.len() - 1;
    let existing_turns = user_turn_spans(&existing);
    let has_trailing_pending_prompt = existing
        .last()
        .is_some_and(|entry| transcript_entry_role(entry) == Some("user"));
    let completed_existing_turns = if has_trailing_pending_prompt {
        existing_turns.len().saturating_sub(1)
    } else {
        existing_turns.len()
    };
    let kinds: Vec<TranscriptTurnKind> = existing_turns[..completed_existing_turns]
        .iter()
        .map(|span| transcript_turn_kind(&existing[span.clone()]))
        .collect();
    let in_model_history: Vec<usize> = (0..completed_existing_turns)
        .filter(|&turn| kinds[turn] != TranscriptTurnKind::FailedDisplayOnly)
        .collect();

    let overlap = rebuilt_prior_turns.min(in_model_history.len());
    let first_overlapped_turn = if overlap == 0 {
        completed_existing_turns
    } else {
        in_model_history[in_model_history.len() - overlap]
    };
    let cut = existing_turns
        .get(first_overlapped_turn)
        .map_or(existing.len(), |span| span.start);

    let mut merged = sanitize_transcript_entries(existing[..cut].to_vec());
    let rebuilt_turn = |index: usize| {
        new_entries[rebuilt_turns[index].clone()]
            .iter()
            .filter(|entry| !is_orphan_transcript_block(entry))
            .cloned()
            .collect::<Vec<_>>()
    };

    let mut next_rebuilt = rebuilt_prior_turns - overlap;
    for index in 0..next_rebuilt {
        merged.extend(rebuilt_turn(index));
    }
    for (turn, span) in existing_turns[..completed_existing_turns]
        .iter()
        .enumerate()
        .skip(first_overlapped_turn)
    {
        match kinds[turn] {
            TranscriptTurnKind::Completed => {
                let mut rebuilt = rebuilt_turn(next_rebuilt);
                let has_assistant_text = rebuilt.iter().any(|entry| {
                    transcript_entry_role(entry) == Some("assistant")
                        && entry
                            .get("content")
                            .and_then(|c| c.as_str())
                            .is_some_and(|c| !c.trim().is_empty())
                });
                if !has_assistant_text {
                    if let Some(fallback) = existing[span.clone()].iter().rev().find_map(|entry| {
                        if transcript_entry_role(entry) == Some("assistant") {
                            entry
                                .get("content")
                                .and_then(|c| c.as_str())
                                .filter(|c| !c.trim().is_empty())
                        } else {
                            None
                        }
                    }) {
                        add_fallback_response(&mut rebuilt, fallback);
                    }
                }
                merged.extend(rebuilt);
                next_rebuilt += 1;
            }
            TranscriptTurnKind::FailedInModelHistory => {
                merged.extend_from_slice(&existing[span.clone()]);
                next_rebuilt += 1;
            }
            TranscriptTurnKind::FailedDisplayOnly => {
                merged.extend_from_slice(&existing[span.clone()]);
            }
        }
    }
    for index in next_rebuilt..rebuilt_turns.len() {
        merged.extend(rebuilt_turn(index));
    }
    serde_json::to_string(&merged).unwrap_or_else(|_| "[]".to_string())
}

/// `fallback` is shown as the assistant reply when the turn produced no text;
/// it only touches the Display Transcript, never Model History.
pub(crate) fn successful_turn_persistence(
    history: Option<&[Message]>,
    fallback: Option<&str>,
) -> Option<SuccessfulTurnPersistence> {
    let history = history?;
    let mut display = build_display_transcript(history);
    if let Some(fallback) = fallback {
        add_fallback_response(&mut display, fallback);
    }
    let display_transcript = serde_json::to_string(&display).unwrap_or_else(|_| "[]".to_string());
    let model_history = serde_json::to_string(history).ok();

    Some(SuccessfulTurnPersistence {
        display_transcript,
        model_history,
        history: history.to_vec(),
    })
}

fn add_fallback_response(display: &mut Vec<serde_json::Value>, response: &str) {
    let text_block = |id: String| json!({"kind": "text", "id": id, "text": response});
    let last_entry = display.last_mut();
    let Some(entry) = last_entry.filter(|entry| {
        transcript_entry_role(entry) == Some("assistant")
            && entry
                .get("content")
                .and_then(|content| content.as_str())
                .is_none_or(|content| content.trim().is_empty())
    }) else {
        display.push(json!({
            "role": "assistant",
            "content": response,
            "blocks": [text_block("text-0".to_string())],
        }));
        return;
    };

    entry["content"] = json!(response);
    match entry
        .get_mut("blocks")
        .and_then(|blocks| blocks.as_array_mut())
    {
        Some(blocks) => {
            let empty_text = blocks.iter_mut().find(|block| {
                block.get("kind").and_then(|kind| kind.as_str()) == Some("text")
                    && block
                        .get("text")
                        .and_then(|text| text.as_str())
                        .is_none_or(|text| text.trim().is_empty())
            });
            match empty_text {
                Some(block) => block["text"] = json!(response),
                None => {
                    let id = format!("text-{}", blocks.len());
                    blocks.push(text_block(id));
                }
            }
        }
        None => entry["blocks"] = json!([text_block("text-0".to_string())]),
    }
}

pub(crate) fn failure_turn_persistence(
    existing_display_transcript: &str,
    existing_model_history: Option<&str>,
    turn: &FailedTurn<'_>,
) -> FailureTurnPersistence {
    let mut transcript: Vec<serde_json::Value> =
        serde_json::from_str(existing_display_transcript).unwrap_or_default();
    let (message, is_controlled_stop) = match turn.error {
        LlmError::ControlledStop(message) => (message.clone(), true),
        error => (format!("Error: {}", error.to_dto().message), false),
    };

    let history = match existing_model_history {
        Some(raw) => match crate::conversation::parse_model_history(raw) {
            Ok(history) => Some(history),
            Err(error) => {
                tracing::warn!("Failed to append failed prompt to model history: {}", error);
                None
            }
        },
        None => Some(Vec::new()),
    }
    .map(|mut history| {
        history.push(Message::user(turn.model_prompt));
        history
    });
    let appended = history
        .as_ref()
        .and_then(|history| serde_json::to_string(history).ok());
    let prompt_in_model_history = appended.is_some();
    let history = history.filter(|_| prompt_in_model_history);
    let model_history = appended.or_else(|| existing_model_history.map(str::to_string));

    // A retried prompt is already the transcript's last entry.
    if !transcript.last().is_some_and(|entry| {
        transcript_entry_role(entry) == Some("user")
            && entry.get("content").and_then(|content| content.as_str())
                == Some(turn.display_prompt)
    }) {
        transcript.push(json!({"role": "user", "content": turn.display_prompt}));
    }
    transcript.push(json!({
        "role": "assistant",
        "content": message,
        "blocks": [],
        "error": !is_controlled_stop,
        "controlled_stop": is_controlled_stop,
        (PROMPT_IN_MODEL_HISTORY): prompt_in_model_history,
    }));

    FailureTurnPersistence {
        display_transcript: serde_json::to_string(&transcript).unwrap_or_else(|_| "[]".to_string()),
        model_history,
        history,
    }
}
pub(crate) fn build_display_transcript(messages: &[Message]) -> Vec<serde_json::Value> {
    // First pass: build assistant entries with ordered `blocks` (text + tool calls)
    // and collect tool-result text keyed by tool-call id. Tool results in rig's
    // history live on a following `Message::User { content: ToolResult }` entry,
    // so a second pass stitches each result onto its matching `tool` block. Any
    // unmatched tool result is emitted as its own block so nothing is lost.
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut pending_tool_results: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for message in messages {
        match message {
            Message::User { content } => {
                let mut text_parts: Vec<String> = Vec::new();
                for item in content.iter() {
                    match item {
                        UserContent::Text(text) => text_parts.push(text.text.clone()),
                        UserContent::ToolResult(tool_result) => {
                            let result_text: String = tool_result
                                .content
                                .iter()
                                .map(|part| match part {
                                    ToolResultContent::Text(t) => t.text.clone(),
                                    ToolResultContent::Json { value } => value.to_string(),
                                    _ => String::new(),
                                })
                                .collect::<Vec<_>>()
                                .join("");
                            pending_tool_results.insert(tool_result.call.to_string(), result_text);
                        }
                        _ => {}
                    }
                }
                if !text_parts.is_empty() {
                    entries.push(json!({
                        "role": "user",
                        "content": text_parts.join(""),
                    }));
                }
            }
            Message::Assistant(message) => {
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                let mut text_parts: Vec<String> = Vec::new();
                for item in message.content.iter() {
                    match item {
                        AssistantContent::Text(text) => {
                            text_parts.push(text.text.clone());
                            blocks.push(json!({
                                "kind": "text",
                                "id": format!("text-{}", blocks.len()),
                                "text": text.text.clone(),
                            }));
                        }
                        AssistantContent::ToolCall(tool_call) => {
                            blocks.push(json!({
                                "kind": "tool",
                                "id": tool_call.id.to_string(),
                                "name": tool_call.function.name.as_str(),
                                "arguments": observable_tool_arguments(
                                    tool_call.function.name.as_str(),
                                    &tool_call.function.arguments_value(),
                                ),
                                "result": null,
                                "status": "completed",
                            }));
                        }
                        _ => {}
                    }
                }
                if blocks.is_empty() && !text_parts.is_empty() {
                    // Defensive: shouldn't happen given the loop above, but keep
                    // a sane fallback.
                    blocks.push(json!({
                        "kind": "text",
                        "id": "text-0",
                        "text": text_parts.join(""),
                    }));
                }
                if blocks.is_empty() {
                    continue;
                }
                entries.push(json!({
                    "role": "assistant",
                    "content": text_parts.join(""),
                    "blocks": blocks,
                }));
            }
            _ => {}
        }
    }

    // Second pass: attach each collected tool result to its matching `tool`
    // block by id. Unmatched results are appended as their own assistant-level
    // tool block so no tool output is silently dropped.
    if !pending_tool_results.is_empty() {
        for entry in entries.iter_mut() {
            if entry.get("role").and_then(|r| r.as_str()) != Some("assistant") {
                continue;
            }
            if let Some(blocks) = entry.get_mut("blocks").and_then(|b| b.as_array_mut()) {
                for block in blocks.iter_mut() {
                    if block.get("kind").and_then(|k| k.as_str()) != Some("tool") {
                        continue;
                    }
                    if let Some(id) = block.get("id").and_then(|i| i.as_str()) {
                        if let Some(result) = pending_tool_results.remove(id) {
                            block["result"] = serde_json::Value::String(result);
                        }
                    }
                }
            }
        }
        for (id, result) in pending_tool_results.drain() {
            entries.push(json!({
                "role": "assistant",
                "content": "",
                "blocks": [
                    {
                        "kind": "tool",
                        "id": id,
                        "name": "",
                        "arguments": null,
                        "result": result,
                        "status": "completed",
                    }
                ],
            }));
        }
    }

    // A transcript starts at its first user message. Model History pruned for
    // the token budget can begin mid-turn, leaving assistant blocks orphaned
    // in front of the initiating prompt.
    if let Some(first_user) = entries
        .iter()
        .position(|entry| entry.get("role").and_then(|r| r.as_str()) == Some("user"))
    {
        entries.drain(..first_user);
    }

    entries
}
