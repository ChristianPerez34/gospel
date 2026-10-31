use crate::model_history::parse_model_history;
use rig::completion::message::{AssistantContent, Message, UserContent};

pub fn validate_turn_count(
    raw_history: Option<&str>,
    expected: usize,
    retry_turn: Option<usize>,
) -> Result<(), String> {
    let history = parse_model_history(raw_history.unwrap_or("[]"))
        .map_err(|error| format!("Unable to read conversation history: {error}"))?;
    let actual = history.iter().filter(|message| matches!(
        message, Message::User { content } if content.iter().any(|item| matches!(item, UserContent::Text(_)))
    )).count();
    // A failed turn may not have reached model-history persistence. Otherwise
    // fail closed when pruning or another view has changed the saved history.
    if actual == expected || (retry_turn == Some(actual) && expected == actual + 1) {
        Ok(())
    } else {
        Err("The saved conversation has changed. Reopen the task and try again.".into())
    }
}

/// Keep complete user turns, including their tool calls/results. Tool-result
/// messages have the user role too, but must not count as new prompts.
pub fn history_before_turn(
    raw_history: Option<&str>,
    turn_index: usize,
) -> Result<Vec<Message>, String> {
    let history = parse_model_history(raw_history.unwrap_or("[]"))
        .map_err(|error| format!("Unable to read conversation history: {error}"))?;
    let starts: Vec<usize> = history
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
        .collect();
    if turn_index > starts.len() {
        return Err("This response is no longer available in the saved conversation.".into());
    }
    let end = starts.get(turn_index).copied().unwrap_or(history.len());
    Ok(history[..end].to_vec())
}

pub fn history_through_response(
    raw_history: Option<&str>,
    turn_index: usize,
    response: &str,
) -> Result<Vec<Message>, String> {
    let start = history_before_turn(raw_history, turn_index)?.len();
    let mut history = history_before_turn(raw_history, turn_index + 1)?;
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
}
