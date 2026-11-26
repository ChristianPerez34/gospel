use crate::text_utils::truncate_text_bytes;
use rig::completion::message::{AssistantContent, Message, ToolResultContent, UserContent};

pub(crate) const MAX_MESSAGES_PER_CONVERSATION: usize = 50;
pub(crate) const MAX_HISTORY_BYTES: usize = 64 * 1024;
pub(crate) const MAX_TOOL_RESULT_BYTES: usize = 8 * 1024;

pub(crate) fn prune_history_for_storage(mut messages: Vec<Message>) -> Vec<Message> {
    for message in &mut messages {
        truncate_tool_result_text(message);
    }

    trim_history_message_count(&mut messages);
    trim_history_bytes(&mut messages);
    trim_single_message_text_to_history_cap(&mut messages);
    let safe_start = rig::conversation::pairing::safe_start(&messages, 0);
    messages.drain(..safe_start);
    messages
}

fn truncate_tool_result_text(message: &mut Message) {
    let Message::User { content } = message else {
        return;
    };

    for item in content.iter_mut() {
        let UserContent::ToolResult(tool_result) = item else {
            continue;
        };

        for result_content in tool_result.content.iter_mut() {
            match result_content {
                ToolResultContent::Text(text) => {
                    let (truncated, did_truncate) =
                        truncate_text_bytes(&text.text, MAX_TOOL_RESULT_BYTES);
                    if did_truncate {
                        text.text = truncated;
                    }
                }
                ToolResultContent::Json { value } => {
                    // Demote oversized structured output instead of corrupting the JSON.
                    let rendered = value.to_string();
                    let (truncated, did_truncate) =
                        truncate_text_bytes(&rendered, MAX_TOOL_RESULT_BYTES);
                    if did_truncate {
                        *result_content = ToolResultContent::text(truncated);
                    }
                }
                _ => {}
            }
        }
    }
}

fn trim_history_message_count(messages: &mut Vec<Message>) {
    if messages.len() > MAX_MESSAGES_PER_CONVERSATION {
        let excess = messages.len() - MAX_MESSAGES_PER_CONVERSATION;
        let safe_start = rig::conversation::pairing::safe_start(messages, excess);
        messages.drain(..safe_start);
    }
}

fn trim_history_bytes(messages: &mut Vec<Message>) {
    while messages.len() > 1 && serialized_history_bytes(messages) > MAX_HISTORY_BYTES {
        let safe_start = rig::conversation::pairing::safe_start(messages, 1);
        messages.drain(..safe_start);
    }
}

fn trim_single_message_text_to_history_cap(messages: &mut [Message]) {
    if messages.len() != 1 {
        return;
    }

    while serialized_history_bytes(messages) > MAX_HISTORY_BYTES {
        let overage = serialized_history_bytes(messages).saturating_sub(MAX_HISTORY_BYTES);
        if !truncate_message_text_payloads(&mut messages[0], overage + 1024) {
            return;
        }
    }
}

fn truncate_message_text_payloads(message: &mut Message, reduce_by: usize) -> bool {
    match message {
        Message::System { content } => truncate_text_payload(content, reduce_by),
        Message::User { content } => {
            let mut truncated = false;
            for item in content.iter_mut() {
                match item {
                    UserContent::Text(text) => {
                        truncated |= truncate_text_payload(&mut text.text, reduce_by);
                    }
                    UserContent::ToolResult(tool_result) => {
                        for result_content in tool_result.content.iter_mut() {
                            match result_content {
                                ToolResultContent::Text(text) => {
                                    truncated |= truncate_text_payload(&mut text.text, reduce_by);
                                }
                                // Structured output is demoted to text when
                                // it must shrink; a Json block cannot be
                                // truncated in place without corrupting it.
                                ToolResultContent::Json { value } => {
                                    let mut rendered = value.to_string();
                                    if truncate_text_payload(&mut rendered, reduce_by) {
                                        *result_content = ToolResultContent::text(rendered);
                                        truncated = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            truncated
        }
        Message::Assistant(message) => {
            let mut truncated = false;
            for item in message.content.iter_mut() {
                match item {
                    AssistantContent::Text(text) => {
                        truncated |= truncate_text_payload(&mut text.text, reduce_by);
                    }
                    // Provider reasoning text is also subject to the size
                    // cap so a single over-long reasoning block cannot
                    // stall history pruning.
                    AssistantContent::Reasoning(reasoning) => {
                        truncated |= truncate_text_payload(&mut reasoning.text, reduce_by);
                    }
                    _ => {}
                }
            }
            truncated
        }
    }
}

fn truncate_text_payload(text: &mut String, reduce_by: usize) -> bool {
    if text.is_empty() {
        return false;
    }

    let max_bytes = text.len().saturating_sub(reduce_by);
    let (truncated, did_truncate) = truncate_text_bytes(text, max_bytes);
    if !did_truncate || truncated.len() >= text.len() {
        return false;
    }

    *text = truncated;
    true
}

pub(crate) fn serialized_history_bytes(messages: &[Message]) -> usize {
    serde_json::to_vec(messages)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::completion::message::{AssistantContent, AssistantMessage, CallId, ToolCall, ToolFunction, ToolName, ToolResultContent, UserContent};

    /// The cache died with the Conversation deepening; what these tests exercise
    /// is purely the pruning pipeline that ran inside it. This harness is a
    /// trivial passthrough so the cases survive verbatim.
    struct PruneOnlyStore(Vec<Message>);

    impl PruneOnlyStore {
        fn new() -> Self {
            Self(Vec::new())
        }

        fn store_history(&mut self, _session_id: &str, messages: Vec<Message>) {
            self.0 = prune_history_for_storage(messages);
        }

        fn get_history(&mut self, _session_id: &str) -> Vec<Message> {
            self.0.clone()
        }

        fn get_history_if_present(&mut self, _session_id: &str) -> Option<Vec<Message>> {
            Some(self.0.clone())
        }
    }

    fn make_user_message(text: &str) -> Message {
        Message::user(text)
    }

fn make_tool_result_message(id: &str, text: &str) -> Message {
        Message::tool_result(
            CallId::from_wire(id),
            ToolName::new("read_file").unwrap(),
            text,
        )
    }

fn make_tool_call_message(id: &str) -> Message {
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
            ToolCall {
                id: CallId::from_wire(id),
                function: ToolFunction::new(
                    ToolName::new("read_file").unwrap(),
                    serde_json::json!({}),
                ),
                native: None,
            },
        )]))
    }

fn make_assistant_message(text: &str) -> Message {
        Message::assistant(text)
    }

fn make_tool_calls_message(ids: &[&str]) -> Message {
        Message::Assistant(AssistantMessage::new(
            ids.iter()
                .map(|id| {
                    AssistantContent::ToolCall(ToolCall {
                        id: CallId::from_wire(*id),
                        function: ToolFunction::new(
                            ToolName::new("read_file").unwrap(),
                            serde_json::json!({}),
                        ),
                        native: None,
                    })
                })
                .collect(),
        ))
    }

    #[test]
    fn count_pruning_never_splits_a_multi_call_batch() {
        let mut store = PruneOnlyStore::new();
        // 52 messages: the count cap would cut between the two results.
        let mut messages = vec![
            make_tool_calls_message(&["a", "b"]),
            make_tool_result_message("a", "first"),
            make_tool_result_message("b", "second"),
        ];
        for i in 0..24 {
            messages.push(make_user_message(&format!("request {i}")));
            messages.push(make_assistant_message("Done."));
        }
        messages.push(make_user_message("last"));

        store.store_history("s1", messages);
        let history = store.get_history("s1");

        assert!(history.len() <= MAX_MESSAGES_PER_CONVERSATION);
        assert!(rig::conversation::pairing::validate(&history).is_ok());
        assert_eq!(history[0], make_user_message("request 0"));
    }

#[test]
    fn byte_pruning_keeps_tool_calls_paired() {
        let mut store = PruneOnlyStore::new();
        let payload = "x".repeat(MAX_TOOL_RESULT_BYTES - 512);
        let mut messages = Vec::new();
        for i in 0..15 {
            let id = format!("call-{i}");
            messages.push(make_user_message(&format!("request {i}")));
            messages.push(make_tool_calls_message(&[&id]));
            messages.push(make_tool_result_message(&id, &payload));
        }
        assert!(serialized_history_bytes(&messages) > MAX_HISTORY_BYTES);

        store.store_history("s1", messages);
        let history = store.get_history("s1");

        assert!(!history.is_empty());
        assert!(serialized_history_bytes(&history) <= MAX_HISTORY_BYTES);
        assert!(rig::conversation::pairing::validate(&history).is_ok());
    }

#[test]
    fn pruning_drops_history_made_only_of_orphaned_results() {
        let mut store = PruneOnlyStore::new();
        store.store_history(
            "s1",
            vec![
                make_tool_result_message("gone-1", "stale"),
                make_tool_result_message("gone-2", "stale"),
            ],
        );

        assert_eq!(store.get_history_if_present("s1"), Some(Vec::new()));
    }

#[test]
    fn message_cap_drops_oldest() {
        let mut store = PruneOnlyStore::new();
        let mut msgs = vec![];
        for i in 0..60 {
            msgs.push(make_user_message(&format!("msg {}", i)));
        }
        store.store_history("s1", msgs);
        let retrieved = store.get_history("s1");
        assert_eq!(retrieved.len(), 50);
    }

#[test]
    fn large_tool_results_are_truncated_before_storage() {
        let mut store = PruneOnlyStore::new();
        let oversized = "a".repeat(MAX_TOOL_RESULT_BYTES * 2);
        store.store_history(
            "s1",
            vec![
                make_tool_call_message("tool-1"),
                make_tool_result_message("tool-1", &oversized),
            ],
        );

        let retrieved = store.get_history("s1");
        let Message::User { content } = &retrieved[1] else {
            panic!("expected user message");
        };
        let Some(UserContent::ToolResult(tool_result)) = content.iter().next() else {
            panic!("expected tool result content");
        };
        let Some(ToolResultContent::Text(text)) = tool_result.content.first() else {
            panic!("expected tool result text");
        };

        assert!(text.text.len() <= MAX_TOOL_RESULT_BYTES);
        assert!(text.text.contains("[truncated]"));
    }

#[test]
    fn byte_cap_drops_oldest_messages() {
        let mut store = PruneOnlyStore::new();
        let large = "x".repeat(20 * 1024);
        let messages = vec![
            make_user_message(&format!("one-{large}")),
            make_user_message(&format!("two-{large}")),
            make_user_message(&format!("three-{large}")),
            make_user_message(&format!("four-{large}")),
        ];

        store.store_history("s1", messages);
        let retrieved = store.get_history("s1");

        assert!(serialized_history_bytes(&retrieved) <= MAX_HISTORY_BYTES);
        assert!(retrieved.len() < 4);
        assert_eq!(
            retrieved.last(),
            Some(&make_user_message(&format!("four-{large}")))
        );
    }

#[test]
    fn byte_cap_truncates_single_oversized_user_message() {
        let mut store = PruneOnlyStore::new();
        let large = "x".repeat(MAX_HISTORY_BYTES * 2);

        store.store_history("s1", vec![make_user_message(&large)]);
        let retrieved = store.get_history("s1");

        assert_eq!(retrieved.len(), 1);
        assert!(serialized_history_bytes(&retrieved) <= MAX_HISTORY_BYTES);

        let Message::User { content } = &retrieved[0] else {
            panic!("expected user message");
        };
        let Some(UserContent::Text(text)) = content.iter().next() else {
            panic!("expected text content");
        };
        assert!(text.text.contains("[truncated]"));
        assert!(text.text.len() < large.len());
    }

#[test]
    fn large_json_tool_result_is_demoted_and_truncated_before_storage() {
        use rig::completion::message::ToolResult;

        let mut store = PruneOnlyStore::new();
        let large = serde_json::json!({"output": "x".repeat(MAX_HISTORY_BYTES * 2)});
        let result = Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("call_1"),
                name: ToolName::new("read_file").unwrap(),
                content: vec![ToolResultContent::Json { value: large }],
                is_error: false,
            })],
        };

        store.store_history("s1", vec![make_tool_call_message("call_1"), result]);
        let retrieved = store.get_history("s1");

        assert_eq!(retrieved.len(), 2);
        assert!(serialized_history_bytes(&retrieved) <= MAX_HISTORY_BYTES);
        let Message::User { content } = &retrieved[1] else {
            panic!("expected user message");
        };
        let Some(UserContent::ToolResult(result)) = content.first() else {
            panic!("expected tool result");
        };
        let Some(ToolResultContent::Text(text)) = result.content.first() else {
            panic!("expected the trimmed result demoted to text");
        };
        assert!(text.text.contains("[truncated]"));
    }

#[test]
    fn pruning_repairs_orphaned_tool_results_and_keeps_user_text() {
        let mut store = PruneOnlyStore::new();
        store.store_history(
            "s1",
            vec![
                make_tool_result_message("orphan", "stale result"),
                make_user_message("continue with this request"),
                make_assistant_message("Done."),
            ],
        );

        let history = store.get_history("s1");

        assert_eq!(
            history,
            vec![
                make_user_message("continue with this request"),
                make_assistant_message("Done."),
            ]
        );
        assert!(rig::conversation::pairing::validate(&history).is_ok());
    }

#[test]
    fn count_pruning_keeps_tool_calls_paired() {
        let mut store = PruneOnlyStore::new();
        let mut messages = vec![
            make_user_message("old request"),
            make_tool_call_message("old-call"),
            make_tool_result_message("old-call", "done"),
        ];
        for i in 0..25 {
            messages.push(make_user_message(&format!("request {i}")));
            messages.push(make_assistant_message("Done."));
        }

        store.store_history("s1", messages);
        let history = store.get_history("s1");

        assert!(history.len() <= MAX_MESSAGES_PER_CONVERSATION);
        assert!(rig::conversation::pairing::validate(&history).is_ok());
    }

#[test]
    fn oversized_assistant_reasoning_is_truncated_under_history_cap() {
        use rig::completion::message::Reasoning;

        let mut store = PruneOnlyStore::new();
        let large = "z".repeat(MAX_HISTORY_BYTES * 2);
        let message = Message::from(AssistantContent::Reasoning(Reasoning::new(large)));

        store.store_history("s1", vec![message]);

        let retrieved = store.get_history("s1");
        assert_eq!(retrieved.len(), 1);
        let Message::Assistant(message) = &retrieved[0] else {
            panic!("expected assistant message");
        };
        let Some(AssistantContent::Reasoning(reasoning)) = message.content.first() else {
            panic!("expected reasoning content");
        };
        assert!(reasoning.text.contains("[truncated]"));
    }
}
