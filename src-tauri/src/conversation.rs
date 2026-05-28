use rig::completion::message::Message;
use std::collections::HashMap;
use std::sync::Mutex;

const MAX_CONVERSATIONS: usize = 50;
const MAX_MESSAGES_PER_CONVERSATION: usize = 50;

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
        if !self.conversations.contains_key(session_id) {
            if self.conversations.len() >= MAX_CONVERSATIONS {
                if let Some(evict_id) = self.access_order.first().cloned() {
                    self.conversations.remove(&evict_id);
                    self.access_order.remove(0);
                }
            }
        }

        if let Some(pos) = self.access_order.iter().position(|id| id == session_id) {
            self.access_order.remove(pos);
        }
        self.access_order.push(session_id.to_string());

        let mut entry = self
            .conversations
            .entry(session_id.to_string())
            .or_default();
        *entry = new_messages;
        if entry.len() > MAX_MESSAGES_PER_CONVERSATION {
            let excess = entry.len() - MAX_MESSAGES_PER_CONVERSATION;
            entry.drain(..excess);
        }
    }

    pub fn clear(&mut self, session_id: &str) {
        self.conversations.remove(session_id);
        if let Some(pos) = self.access_order.iter().position(|id| id == session_id) {
            self.access_order.remove(pos);
        }
    }
}

pub struct ConversationState {
    pub store: Mutex<ConversationStore>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::completion::message::{AssistantContent, Text, UserContent};

    fn make_user_message(text: &str) -> Message {
        Message::User {
            content: rig::one_or_many::OneOrMany::one(UserContent::Text(Text {
                text: text.to_string(),
            })),
        }
    }

    fn make_assistant_message(text: &str) -> Message {
        Message::Assistant {
            id: None,
            content: rig::one_or_many::OneOrMany::one(AssistantContent::Text(Text {
                text: text.to_string(),
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
}
