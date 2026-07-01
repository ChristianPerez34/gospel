use crate::text_utils::truncate_text_bytes;
use rig::completion::message::{AssistantContent, Message, ToolResultContent, UserContent};
use std::collections::HashMap;
use std::sync::Mutex;

const MAX_CONVERSATIONS: usize = 50;
const MAX_MESSAGES_PER_CONVERSATION: usize = 50;
const MAX_HISTORY_BYTES: usize = 64 * 1024;
const MAX_TOOL_RESULT_BYTES: usize = 8 * 1024;

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

    pub fn get_history(&mut self, session_id: &str) -> Vec<Message> {
        if self.conversations.contains_key(session_id) {
            if let Some(pos) = self.access_order.iter().position(|id| id == session_id) {
                self.access_order.remove(pos);
            }
            self.access_order.push(session_id.to_string());
        }
        self.conversations
            .get(session_id)
            .cloned()
            .unwrap_or_default()
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

    pub fn clear(&mut self, session_id: &str) {
        self.conversations.remove(session_id);
        if let Some(pos) = self.access_order.iter().position(|id| id == session_id) {
            self.access_order.remove(pos);
        }
    }
}

fn prune_history_for_storage(mut messages: Vec<Message>) -> Vec<Message> {
    for message in &mut messages {
        truncate_tool_result_text(message);
    }

    trim_history_message_count(&mut messages);
    trim_history_bytes(&mut messages);
    trim_single_message_text_to_history_cap(&mut messages);
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
            let ToolResultContent::Text(text) = result_content else {
                continue;
            };

            let (truncated, did_truncate) = truncate_text_bytes(&text.text, MAX_TOOL_RESULT_BYTES);
            if did_truncate {
                text.text = truncated;
            }
        }
    }
}

fn trim_history_message_count(messages: &mut Vec<Message>) {
    if messages.len() > MAX_MESSAGES_PER_CONVERSATION {
        let excess = messages.len() - MAX_MESSAGES_PER_CONVERSATION;
        messages.drain(..excess);
    }
}

fn trim_history_bytes(messages: &mut Vec<Message>) {
    while messages.len() > 1 && serialized_history_bytes(messages) > MAX_HISTORY_BYTES {
        messages.remove(0);
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
                            if let ToolResultContent::Text(text) = result_content {
                                truncated |= truncate_text_payload(&mut text.text, reduce_by);
                            }
                        }
                    }
                    _ => {}
                }
            }
            truncated
        }
        Message::Assistant { content, .. } => {
            let mut truncated = false;
            for item in content.iter_mut() {
                if let AssistantContent::Text(text) = item {
                    truncated |= truncate_text_payload(&mut text.text, reduce_by);
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

fn serialized_history_bytes(messages: &[Message]) -> usize {
    serde_json::to_vec(messages)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
}

pub struct ConversationState {
    pub store: Mutex<ConversationStore>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::completion::message::{AssistantContent, Text, ToolResult, UserContent};

    fn make_user_message(text: &str) -> Message {
        Message::User {
            content: rig::one_or_many::OneOrMany::one(UserContent::Text(Text {
                text: text.to_string(),
                additional_params: Some(serde_json::json!({})),
            })),
        }
    }

    fn make_tool_result_message(id: &str, text: &str) -> Message {
        Message::User {
            content: rig::one_or_many::OneOrMany::one(UserContent::ToolResult(ToolResult {
                id: id.to_string(),
                call_id: None,
                content: rig::one_or_many::OneOrMany::one(ToolResultContent::Text(Text {
                    text: text.to_string(),
                    additional_params: Some(serde_json::json!({})),
                })),
            })),
        }
    }

    fn make_assistant_message(text: &str) -> Message {
        Message::Assistant {
            id: None,
            content: rig::one_or_many::OneOrMany::one(AssistantContent::Text(Text {
                text: text.to_string(),
                additional_params: Some(serde_json::json!({})),
            })),
        }
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
        store.store_history("s1", vec![make_tool_result_message("tool-1", &oversized)]);

        let retrieved = store.get_history("s1");
        let Message::User { content } = &retrieved[0] else {
            panic!("expected user message");
        };
        let Some(UserContent::ToolResult(tool_result)) = content.iter().next() else {
            panic!("expected tool result content");
        };
        let Some(ToolResultContent::Text(text)) = tool_result.content.iter().next() else {
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
    fn clear_removes_conversation() {
        let mut store = ConversationStore::new();
        store.store_history("s1", vec![make_user_message("hello")]);
        store.clear("s1");
        let retrieved = store.get_history("s1");
        assert!(retrieved.is_empty());
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
}
