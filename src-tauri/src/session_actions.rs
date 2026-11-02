use crate::model_history::parse_model_history;
use rig::completion::message::{AssistantContent, Message, UserContent};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageActionRequest {
    pub session_id: String,
    pub turn_count: usize,
    pub fork: bool,
    pub response_text: String,
    pub expected_user_turns: usize,
    pub failed_response: bool,
    pub retry_prompt: String,
}

fn parse_history(raw_history: Option<&str>) -> Result<Vec<Message>, String> {
    parse_model_history(raw_history.unwrap_or("[]"))
        .map_err(|error| format!("Unable to read conversation history: {error}"))
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

pub fn validate_turn_count(
    raw_history: Option<&str>,
    expected: usize,
    retry_turn: Option<usize>,
) -> Result<(), String> {
    let actual = turn_starts(&parse_history(raw_history)?).len();
    // A failed turn may not have reached model-history persistence. Otherwise
    // fail closed when pruning or another view has changed the saved history.
    if actual == expected || (retry_turn == Some(actual) && expected == actual + 1) {
        Ok(())
    } else {
        Err("The saved conversation has changed. Reopen the task and try again.".into())
    }
}

/// Keep complete user turns, including their tool calls/results.
pub fn history_before_turn(
    raw_history: Option<&str>,
    turn_index: usize,
) -> Result<Vec<Message>, String> {
    let history = parse_history(raw_history)?;
    let starts = turn_starts(&history);
    if turn_index > starts.len() {
        return Err("This response is no longer available in the saved conversation.".into());
    }
    let end = starts.get(turn_index).copied().unwrap_or(history.len());
    Ok(history[..end].to_vec())
}

/// Transcript prefix through the `prompt_index`-th user entry (0-based). A
/// retried prompt stays visible while its replacement runs, so a failed or
/// abandoned retry does not silently drop it from the user's record. When the
/// prompt never reached the transcript (its only turn failed before
/// persistence), drop the failed attempt's leftover rows and re-add `prompt`.
pub fn transcript_through_prompt(
    raw_transcript: &str,
    prompt_index: usize,
    prompt: &str,
) -> Result<String, String> {
    let mut entries: Vec<serde_json::Value> = serde_json::from_str(raw_transcript)
        .map_err(|error| format!("Unable to read conversation transcript: {error}"))?;
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
        .map_err(|error| format!("Unable to save conversation transcript: {error}"))
}

pub fn history_through_response(
    raw_history: Option<&str>,
    turn_index: usize,
    response: &str,
) -> Result<Vec<Message>, String> {
    let history = parse_history(raw_history)?;
    let starts = turn_starts(&history);
    if turn_index + 1 > starts.len() {
        return Err("This response is no longer available in the saved conversation.".into());
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
    Err(
        "This response does not match the saved conversation. Reopen the task and try again."
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prefixes_preserve_tool_pairs_and_exclude_later_turns() {
        let history = json!([
            {"role":"user","content":[{"type":"text","text":"first"}]},
            {"role":"assistant","content":[{"type":"toolcall","id":{"provider":"call-1"},"function":{"name":"read","arguments":{}}}]},
            {"role":"user","content":[{"type":"toolresult","call":{"provider":"call-1"},"name":"read","content":[{"type":"text","text":"result"}]}]},
            {"role":"assistant","content":[{"type":"text","text":"answer"}]},
            {"role":"user","content":[{"type":"text","text":"second"}]},
            {"role":"assistant","content":[{"type":"text","text":"later"}]}
        ]).to_string();
        assert!(history_before_turn(Some(&history), 0).unwrap().is_empty());
        assert_eq!(history_before_turn(Some(&history), 1).unwrap().len(), 4);
        assert_eq!(history_before_turn(Some(&history), 2).unwrap().len(), 6);
        assert!(history_before_turn(Some(&history), 3).is_err());
        let tool_prefix = history_through_response(Some(&history), 0, "").unwrap();
        assert_eq!(tool_prefix.len(), 3);
        assert_eq!(
            history_through_response(Some(&history), 0, "answer")
                .unwrap()
                .len(),
            4
        );
        assert!(history_through_response(Some(&history), 0, "later").is_err());
        assert!(validate_turn_count(Some(&history), 2, None).is_ok());
        assert!(validate_turn_count(Some(&history), 3, None).is_err());
        assert!(validate_turn_count(Some(&history), 3, Some(2)).is_ok());
        assert!(validate_turn_count(Some(&history), 4, Some(3)).is_err());
    }

    #[test]
    fn forks_match_both_live_aggregated_text_and_reloaded_individual_responses() {
        let history = serde_json::to_string(&vec![
            Message::user("prompt"),
            Message::assistant("Before tools."),
            Message::assistant("After tools."),
            Message::user("later prompt"),
            Message::assistant("Later answer."),
        ])
        .unwrap();
        assert_eq!(
            history_through_response(Some(&history), 0, "Before tools.After tools.")
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            history_through_response(Some(&history), 0, "Before tools.")
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            history_through_response(Some(&history), 0, "After tools.")
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn transcript_through_prompt_keeps_the_retried_prompt_visible() {
        let transcript = json!([
            {"role":"user","content":"first"},
            {"role":"assistant","content":"answer","blocks":[]},
            {"role":"user","content":"second"},
            {"role":"assistant","content":"later","blocks":[]}
        ])
        .to_string();
        let kept: Vec<serde_json::Value> = serde_json::from_str(
            &transcript_through_prompt(&transcript, 1, "second").unwrap(),
        )
        .unwrap();
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[2]["content"], "second");
        // A failed turn leaves prompt + error row in the transcript; retrying
        // it still resolves the same prompt index.
        let failed = json!([
            {"role":"user","content":"first"},
            {"role":"assistant","content":"answer","blocks":[]},
            {"role":"user","content":"second"},
            {"role":"assistant","content":"Error: down","blocks":[],"error":true}
        ])
        .to_string();
        let kept: Vec<serde_json::Value> = serde_json::from_str(
            &transcript_through_prompt(&failed, 1, "second").unwrap(),
        )
        .unwrap();
        assert_eq!(kept.len(), 3);
        // The retried prompt may never have persisted: its only turn failed
        // before writing. Drop the failed attempt's error row and re-add it.
        let missing = json!([
            {"role":"user","content":"first"},
            {"role":"assistant","content":"answer","blocks":[]},
            {"role":"assistant","content":"Error: down","blocks":[],"error":true}
        ])
        .to_string();
        let kept: Vec<serde_json::Value> = serde_json::from_str(
            &transcript_through_prompt(&missing, 1, "second").unwrap(),
        )
        .unwrap();
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[2]["role"], "user");
        assert_eq!(kept[2]["content"], "second");
        // A missing prompt with no trailing failure rows appends at the end.
        let kept: Vec<serde_json::Value> = serde_json::from_str(
            &transcript_through_prompt(&transcript, 2, "third").unwrap(),
        )
        .unwrap();
        assert_eq!(kept.len(), 5);
        assert_eq!(kept[4]["content"], "third");
        assert!(transcript_through_prompt("not json", 0, "x").is_err());
    }
}
