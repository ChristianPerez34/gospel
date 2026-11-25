use super::interface::ConversationError;
use crate::conversation::parse_model_history;
use rig::completion::message::{AssistantContent, Message, UserContent};

fn parse_history(raw_history: Option<&str>) -> Result<Vec<Message>, ConversationError> {
    parse_model_history(raw_history.unwrap_or("[]"))
        .map_err(|error| ConversationError::InvalidModelHistory(error.to_string()))
}

/// Indices of messages that open a user turn: a User message containing text.
/// Tool-result messages have the user role too, but must not count as new
/// prompts.
fn turn_starts(history: &[Message]) -> Vec<usize> {
    history
        .iter()
        .enumerate()
        .filter_map(|(index, message)| match message {
            Message::User { content }
                if content
                    .iter()
                    .any(|item| matches!(item, UserContent::Text(_))) =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn validate_turn_count(
    raw_history: Option<&str>,
    expected: usize,
    retry_turn: Option<usize>,
) -> Result<(), ConversationError> {
    let actual = turn_starts(&parse_history(raw_history)?).len();
    // Legacy failed turns may not have reached model-history persistence.
    // Otherwise fail closed when pruning or another view changed the history.
    if actual == expected || (retry_turn == Some(actual) && expected == actual + 1) {
        Ok(())
    } else {
        Err(ConversationError::Stale)
    }
}

/// Keep complete user turns, including their tool calls/results.
pub(crate) fn history_before_turn(
    raw_history: Option<&str>,
    turn_index: usize,
) -> Result<Vec<Message>, ConversationError> {
    let history = parse_history(raw_history)?;
    let starts = turn_starts(&history);
    if turn_index > starts.len() {
        return Err(ConversationError::TurnUnavailable);
    }
    let end = starts.get(turn_index).copied().unwrap_or(history.len());
    Ok(history[..end].to_vec())
}

/// Transcript prefix through the `prompt_index`-th user entry (0-based). A
/// retried prompt stays visible while its replacement runs, so a failed or
/// abandoned retry does not silently drop it from the user's record. When the
/// prompt never reached the transcript (its only turn failed before
/// persistence), drop the failed attempt's leftover rows and re-add `prompt`.
pub(crate) fn transcript_through_prompt(
    raw_transcript: &str,
    prompt_index: usize,
    prompt: &str,
) -> Result<String, ConversationError> {
    let mut entries: Vec<serde_json::Value> = serde_json::from_str(raw_transcript)
        .map_err(|error| ConversationError::InvalidDisplayTranscript(error.to_string()))?;
    match entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.get("role").and_then(|role| role.as_str()) == Some("user"))
        .map(|(index, _)| index)
        .nth(prompt_index)
    {
        Some(end) => entries.truncate(end + 1),
        None => {
            while entries.last().is_some_and(|entry| {
                entry.get("error").and_then(|value| value.as_bool()) == Some(true)
                    || entry.get("controlled_stop").and_then(|value| value.as_bool())
                        == Some(true)
            }) {
                entries.pop();
            }
            entries.push(serde_json::json!({"role": "user", "content": prompt}));
        }
    }
    serde_json::to_string(&entries)
        .map_err(|error| ConversationError::UnwriteableDisplayTranscript(error.to_string()))
}

/// Transcript prefix through the assistant response to the `prompt_index`-th
/// user entry (0-based). A fork ends at the selected response, so its
/// transcript keeps everything the user saw before it — including turns
/// pruned from Model History — and nothing after it.
pub(crate) fn transcript_through_response(
    raw_transcript: &str,
    prompt_index: usize,
    response: &str,
) -> Result<String, ConversationError> {
    let mut entries: Vec<serde_json::Value> = serde_json::from_str(raw_transcript)
        .map_err(|error| ConversationError::InvalidDisplayTranscript(error.to_string()))?;
    let turn_start = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.get("role").and_then(|role| role.as_str()) == Some("user"))
        .map(|(index, _)| index)
        .nth(prompt_index)
        .ok_or(ConversationError::TurnUnavailable)?;
    let turn_end = entries[turn_start + 1..]
        .iter()
        .position(|entry| entry.get("role").and_then(|role| role.as_str()) == Some("user"))
        .map(|offset| turn_start + 1 + offset)
        .unwrap_or(entries.len());
    let normalize = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let target = normalize(response);
    let mut accumulated = String::new();
    for index in turn_start + 1..turn_end {
        let entry = &entries[index];
        if entry.get("role").and_then(|role| role.as_str()) != Some("assistant") {
            continue;
        }
        let text = entry.get("content").and_then(|c| c.as_str()).unwrap_or("");
        accumulated.push_str(text);
        if normalize(text) == target || normalize(&accumulated) == target {
            entries.truncate(index + 1);
            return serde_json::to_string(&entries)
                .map_err(|error| ConversationError::UnwriteableDisplayTranscript(error.to_string()));
        }
    }
    Err(ConversationError::ResponseMismatch)
}

pub(crate) fn history_through_response(
    raw_history: Option<&str>,
    turn_index: usize,
    response: &str,
) -> Result<Vec<Message>, ConversationError> {
    let history = parse_history(raw_history)?;
    let starts = turn_starts(&history);
    if turn_index + 1 > starts.len() {
        return Err(ConversationError::TurnUnavailable);
    }
    let start = starts[turn_index];
    let end_bound = starts.get(turn_index + 1).copied().unwrap_or(history.len());
    let mut history = history[..end_bound].to_vec();
    let normalize = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let target = normalize(response);
    let mut accumulated = String::new();
    for index in start..history.len() {
        let Message::Assistant(message) = &history[index] else {
            continue;
        };
        if !message.content.iter().any(|item| {
            matches!(
                item,
                AssistantContent::Text(_) | AssistantContent::ToolCall(_)
            )
        }) {
            continue;
        }
        let text = message
            .content
            .iter()
            .filter_map(|item| match item {
                AssistantContent::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        // Match useChatStream's joinTextBlocks and persisted transcript text.
        accumulated.push_str(&text);
        if normalize(&text) == target || normalize(&accumulated) == target {
            // Retain results of this assistant's tool calls, but no later
            // assistant response. This also supports transcripts loaded from
            // disk, where tool iterations can appear as separate agent rows.
            let end = (index + 1..history.len())
                .find(|&next| matches!(history[next], Message::Assistant(_)))
                .unwrap_or(history.len());
            history.truncate(end);
            return Ok(history);
        }
    }
    Err(ConversationError::ResponseMismatch)
}
