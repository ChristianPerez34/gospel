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
