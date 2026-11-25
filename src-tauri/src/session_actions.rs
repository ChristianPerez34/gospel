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

#[cfg(test)]
mod tests {
    use crate::conversation::slicing::{
        history_before_turn, history_through_response, transcript_through_prompt,
        transcript_through_response, validate_turn_count,
    };
    use rig::completion::message::Message;
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

    #[test]
    fn transcript_through_response_truncates_at_matching_reply() {
        let transcript = json!([
            {"role":"user","content":"old"},
            {"role":"assistant","content":"older answer","blocks":[]},
            {"role":"user","content":"first"},
            {"role":"assistant","content":"Before tools.","blocks":[]},
            {"role":"assistant","content":"After tools.","blocks":[]},
            {"role":"user","content":"second"},
            {"role":"assistant","content":"later","blocks":[]}
        ])
        .to_string();
        // Forking at turn 1 keeps earlier turns even when they are gone from
        // Model History, and drops later turns.
        let kept: Vec<serde_json::Value> = serde_json::from_str(
            &transcript_through_response(&transcript, 1, "Before tools.After tools.").unwrap(),
        )
        .unwrap();
        assert_eq!(kept.len(), 5);
        assert_eq!(kept[4]["content"], "After tools.");
        let kept: Vec<serde_json::Value> = serde_json::from_str(
            &transcript_through_response(&transcript, 1, "Before tools.").unwrap(),
        )
        .unwrap();
        assert_eq!(kept.len(), 4);
        assert!(transcript_through_response(&transcript, 1, "later").is_err());
        assert!(transcript_through_response(&transcript, 2, "later").is_ok());
        assert!(transcript_through_response("not json", 0, "x").is_err());
    }
}
