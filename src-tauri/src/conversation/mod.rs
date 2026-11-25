pub(crate) mod interface;
#[cfg(test)]
mod interface_tests;
pub(crate) mod migration;
pub(crate) mod pruning;
pub(crate) mod projection;
pub(crate) mod slicing;
pub(crate) mod transcript;

pub(crate) use interface::{Conversation, ConversationError, ExportFormat, ExportProjection, FailedTurn, ForkTarget, RetryTarget, SkillHarvestEntry, TranscriptPage, TranscriptPageRequest};
pub(crate) use migration::parse_model_history;
pub(crate) use pruning::prune_history_for_storage;
#[cfg(test)]
pub(crate) use pruning::{serialized_history_bytes, MAX_HISTORY_BYTES, MAX_MESSAGES_PER_CONVERSATION, MAX_TOOL_RESULT_BYTES};

use rig::completion::message::Message;
use std::collections::HashMap;
use std::sync::Mutex;

const MAX_CONVERSATIONS: usize = 50;

pub struct ConversationStore {
    conversations: HashMap<String, Vec<Message>>,
    access_order: Vec<String>,
}

impl ConversationStore {
    pub fn new() -> Self {
        Self {
            conversations: HashMap::new(),
            access_order: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn get_history(&mut self, session_id: &str) -> Vec<Message> {
        self.get_history_if_present(session_id).unwrap_or_default()
    }

    pub fn get_history_if_present(&mut self, session_id: &str) -> Option<Vec<Message>> {
        let history = self.conversations.get(session_id)?.clone();
        if let Some(pos) = self.access_order.iter().position(|id| id == session_id) {
            self.access_order.remove(pos);
        }
        self.access_order.push(session_id.to_string());
        Some(history)
    }

    pub fn seed_history(&mut self, session_id: &str, messages: Vec<Message>) {
        if !self.conversations.contains_key(session_id) {
            self.store_history(session_id, messages);
        }
    }

    pub fn store_history(&mut self, session_id: &str, new_messages: Vec<Message>) {
        if !self.conversations.contains_key(session_id)
            && self.conversations.len() >= MAX_CONVERSATIONS
        {
            if let Some(evict_id) = self.access_order.first().cloned() {
                self.conversations.remove(&evict_id);
                self.access_order.remove(0);
            }
        }

        if let Some(pos) = self.access_order.iter().position(|id| id == session_id) {
            self.access_order.remove(pos);
        }
        self.access_order.push(session_id.to_string());

        let new_messages = prune_history_for_storage(new_messages);
        let entry = self
            .conversations
            .entry(session_id.to_string())
            .or_default();
        *entry = new_messages;
    }
}


pub struct ConversationState {
    pub store: Mutex<ConversationStore>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::completion::message::{AssistantContent, AssistantMessage, CallId, ToolCall, ToolFunction, ToolName, ToolResultContent, UserContent};

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
        let mut store = ConversationStore::new();
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
        let mut store = ConversationStore::new();
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
        let mut store = ConversationStore::new();
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
    fn store_replaces_history() {
        let mut store = ConversationStore::new();
        let msgs1 = vec![make_user_message("hello"), make_assistant_message("hi")];
        store.store_history("s1", msgs1.clone());
        let msgs2 = vec![
            make_user_message("hello"),
            make_assistant_message("hi"),
            make_user_message("how are you"),
        ];
        store.store_history("s1", msgs2.clone());
        let retrieved = store.get_history("s1");
        assert_eq!(retrieved.len(), 3);
        assert_eq!(retrieved[2], make_user_message("how are you"));
    }

    #[test]
    fn lru_eviction_removes_oldest() {
        let mut store = ConversationStore::new();
        for i in 0..51 {
            store.store_history(
                &format!("s{}", i),
                vec![make_user_message(&format!("msg {}", i))],
            );
        }
        assert_eq!(store.conversations.len(), 50);
        assert!(!store.conversations.contains_key("s0"));
        assert!(store.conversations.contains_key("s50"));
    }

    #[test]
    fn message_cap_drops_oldest() {
        let mut store = ConversationStore::new();
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
        let mut store = ConversationStore::new();
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
        let mut store = ConversationStore::new();
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
        let mut store = ConversationStore::new();
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

        let mut store = ConversationStore::new();
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
    fn access_order_updates_on_get() {
        let mut store = ConversationStore::new();
        store.store_history("s1", vec![make_user_message("a")]);
        store.store_history("s2", vec![make_user_message("b")]);
        let _ = store.get_history("s1");
        store.store_history("s3", vec![make_user_message("c")]);
        for i in 4..52 {
            store.store_history(
                &format!("s{}", i),
                vec![make_user_message(&format!("m{}", i))],
            );
        }
        assert!(!store.conversations.contains_key("s2"));
        assert!(store.conversations.contains_key("s1"));
    }

    #[test]
    fn export_conversation_round_trips() {
        let mut store = ConversationStore::new();
        let original = vec![
            make_user_message("hello"),
            make_assistant_message("hi there"),
        ];
        store.store_history("export-test", original.clone());

        let json = serde_json::to_string_pretty(&store.get_history("export-test")).unwrap();
        let restored: Vec<Message> = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, original);
    }

    #[test]
    fn export_conversation_not_found() {
        let mut store = ConversationStore::new();
        let history = store.get_history("non-existent-session");
        assert!(history.is_empty());
    }

    #[test]
    fn seeding_history_does_not_replace_a_newer_or_intentionally_empty_entry() {
        let mut store = ConversationStore::new();
        store.seed_history("fresh", vec![make_user_message("persisted")]);
        assert_eq!(
            store.get_history("fresh"),
            vec![make_user_message("persisted")]
        );
        store.store_history("fresh", vec![make_user_message("newer")]);
        store.seed_history("fresh", vec![make_user_message("stale")]);
        assert_eq!(store.get_history("fresh"), vec![make_user_message("newer")]);
        store.store_history("empty", Vec::new());
        store.seed_history("empty", vec![make_user_message("stale")]);
        assert_eq!(store.get_history_if_present("empty"), Some(Vec::new()));
    }

    #[test]
    fn history_lookup_distinguishes_missing_from_stored_empty() {
        let mut store = ConversationStore::new();
        assert_eq!(store.get_history_if_present("missing"), None);
        store.store_history("empty", Vec::new());
        assert_eq!(store.get_history_if_present("empty"), Some(Vec::new()));
    }

    #[test]
    fn pruning_repairs_orphaned_tool_results_and_keeps_user_text() {
        let mut store = ConversationStore::new();
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
        let mut store = ConversationStore::new();
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

        let mut store = ConversationStore::new();
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
