//! Live activity relay from sub-agents back to the turn that delegated them.
//!
//! Rig does not pass the tool call id into `Tool::call`, so a sub-agent tool
//! cannot tell which parent call it is serving. The runtime records each
//! sub-agent tool call when it gates the call (before the tool starts), and
//! the tool claims that id by name and arguments. Steps are then sent on the
//! parent run's own event channel, which keeps them ordered between the
//! parent's `ToolCall` and `ToolResult`.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::mpsc::{UnboundedSender, WeakUnboundedSender};

use crate::llm::StreamEvent;

/// Tools whose execution runs a nested agent and reports live steps.
pub(crate) const SUBAGENT_TOOLS: &[&str] = &[
    "delegate_exploration",
    "run_review",
    "run_multi_review",
    "run_security_review",
];

const STEP_RESULT_BYTES_CAP: usize = 2 * 1024;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubagentKind {
    Exploration,
    Review,
    Verification,
}

/// One unit of live sub-agent activity. Tool ids are only unique within the
/// parent call they belong to.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SubagentStep {
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ToolResult {
        id: String,
        name: String,
        result: String,
    },
    /// Streamed report text.
    Text { text: String },
    /// A provider retry discarded streamed text; `text` is what remains.
    TextReset { text: String },
    /// A pipeline stage label, such as a review chunk starting.
    Phase { label: String },
}

impl SubagentStep {
    /// Maps a nested agent's stream event to a visible step. Reasoning and
    /// loop diagnostics stay private to the sub-agent.
    pub(crate) fn from_stream_event(event: StreamEvent) -> Option<Self> {
        match event {
            StreamEvent::ToolCall {
                id,
                name,
                arguments,
            } => Some(Self::ToolCall {
                id,
                name,
                arguments,
            }),
            StreamEvent::ToolResult { id, name, result } => Some(Self::ToolResult {
                id,
                name,
                result: capped_result(&result),
            }),
            StreamEvent::Text(text) => Some(Self::Text { text }),
            StreamEvent::RetryReset { text, .. } => Some(Self::TextReset { text }),
            _ => None,
        }
    }
}

pub(crate) fn capped_result(result: &str) -> String {
    crate::text_utils::truncate_text_bytes(result, STEP_RESULT_BYTES_CAP).0
}

#[derive(Debug)]
struct PendingCall {
    id: String,
    name: String,
    arguments: serde_json::Value,
}

#[derive(Debug, Default)]
pub struct SubagentRelay {
    // Weak so the relay never keeps a finished run's channel open.
    sender: Mutex<Option<WeakUnboundedSender<StreamEvent>>>,
    pending: Mutex<Vec<PendingCall>>,
}

impl SubagentRelay {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub(crate) fn attach(&self, sender: &UnboundedSender<StreamEvent>) {
        *self.sender.lock().unwrap() = Some(sender.downgrade());
    }

    pub(crate) fn record_call(&self, id: &str, name: &str, arguments: &serde_json::Value) {
        if !SUBAGENT_TOOLS.contains(&name) {
            return;
        }
        self.pending.lock().unwrap().push(PendingCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.clone(),
        });
    }

    /// Drops a call that settled without being claimed, such as one whose
    /// arguments failed to parse.
    pub(crate) fn forget(&self, id: &str) {
        self.pending.lock().unwrap().retain(|call| call.id != id);
    }

    /// Claims the oldest recorded call to `name` whose arguments contain
    /// every non-null field of `arguments`, falling back to the oldest call
    /// to `name`.
    fn claim(&self, name: &str, arguments: &serde_json::Value) -> Option<String> {
        let mut pending = self.pending.lock().unwrap();
        let position = pending
            .iter()
            .position(|call| call.name == name && arguments_match(&call.arguments, arguments))
            .or_else(|| pending.iter().position(|call| call.name == name))?;
        Some(pending.remove(position).id)
    }

    fn send(&self, event: StreamEvent) {
        let sender = self
            .sender
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|weak| weak.upgrade());
        if let Some(sender) = sender {
            let _ = sender.send(event);
        }
    }
}

fn arguments_match(recorded: &serde_json::Value, claimed: &serde_json::Value) -> bool {
    let Some(claimed) = claimed.as_object() else {
        return true;
    };
    claimed
        .iter()
        .filter(|(_, value)| !value.is_null())
        .all(|(key, value)| recorded.get(key) == Some(value))
}

/// Sends one parent call's sub-agent steps. Without a relay or a recorded
/// call, steps are dropped.
#[derive(Clone)]
pub struct SubagentEmitter {
    target: Option<(Arc<SubagentRelay>, String)>,
    kind: SubagentKind,
}

impl SubagentEmitter {
    pub fn claim(
        relay: Option<&Arc<SubagentRelay>>,
        tool_name: &str,
        arguments: &serde_json::Value,
        kind: SubagentKind,
    ) -> Self {
        let target = relay.and_then(|relay| {
            relay
                .claim(tool_name, arguments)
                .map(|parent_id| (relay.clone(), parent_id))
        });
        Self { target, kind }
    }

    pub fn emit(&self, step: SubagentStep) {
        if let Some((relay, parent_id)) = &self.target {
            relay.send(StreamEvent::Subagent {
                parent_id: parent_id.clone(),
                agent: self.kind,
                step,
            });
        }
    }
}

impl std::fmt::Debug for SubagentEmitter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubagentEmitter")
            .field("parent_id", &self.target.as_ref().map(|(_, id)| id))
            .field("kind", &self.kind)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::sync::mpsc::unbounded_channel;

    fn parent_ids(events: &mut tokio::sync::mpsc::UnboundedReceiver<StreamEvent>) -> Vec<String> {
        let mut ids = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let StreamEvent::Subagent { parent_id, .. } = event {
                ids.push(parent_id);
            }
        }
        ids
    }

    #[test]
    fn claims_the_call_with_matching_arguments() {
        let relay = SubagentRelay::new();
        let (sender, mut events) = unbounded_channel();
        relay.attach(&sender);
        relay.record_call("a", "delegate_exploration", &json!({"task": "one"}));
        relay.record_call("b", "delegate_exploration", &json!({"task": "two"}));

        let second = SubagentEmitter::claim(
            Some(&relay),
            "delegate_exploration",
            &json!({"task": "two", "context": null}),
            SubagentKind::Exploration,
        );
        let first = SubagentEmitter::claim(
            Some(&relay),
            "delegate_exploration",
            &json!({"task": "something else"}),
            SubagentKind::Exploration,
        );
        second.emit(SubagentStep::Text { text: "x".into() });
        first.emit(SubagentStep::Text { text: "y".into() });

        assert_eq!(parent_ids(&mut events), vec!["b", "a"]);
    }

    #[test]
    fn ignores_ordinary_tools_and_forgotten_calls() {
        let relay = SubagentRelay::new();
        let (sender, mut events) = unbounded_channel();
        relay.attach(&sender);
        relay.record_call("r", "read_file", &json!({}));
        relay.record_call("e", "run_review", &json!({"mode": "local"}));
        relay.forget("e");

        let emitter = SubagentEmitter::claim(
            Some(&relay),
            "run_review",
            &json!({"mode": "local"}),
            SubagentKind::Review,
        );
        emitter.emit(SubagentStep::Phase { label: "x".into() });
        assert!(parent_ids(&mut events).is_empty());
    }

    #[test]
    fn relay_does_not_keep_a_finished_run_open() {
        let relay = SubagentRelay::new();
        let (sender, mut events) = unbounded_channel::<StreamEvent>();
        relay.attach(&sender);
        drop(sender);
        assert!(events.try_recv().is_err());
        assert!(events.is_closed());
    }

    #[test]
    fn caps_tool_results() {
        let step = SubagentStep::from_stream_event(StreamEvent::ToolResult {
            id: "1".into(),
            name: "read_file".into(),
            result: "x".repeat(10_000),
        });
        let Some(SubagentStep::ToolResult { result, .. }) = step else {
            panic!("expected tool result");
        };
        assert!(result.len() <= STEP_RESULT_BYTES_CAP);
    }
}
