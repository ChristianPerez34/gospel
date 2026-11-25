//! Interface-level tests for the Conversation: retry, fork, transcript paging,
//! and export projections asserted against in-memory SQLite.

use super::*;
use crate::session_store::{SessionDetail, SessionStore};
use rig::completion::message::Message;

fn store_and_conversation() -> (SessionStore, String) {
    let store = SessionStore::in_memory_for_test().unwrap();
    let record = store.create_session("t", "p", "m", None).unwrap();
    (store, record.id)
}

fn conv<'a>(store: &'a SessionStore, id: &str) -> Conversation<'a> {
    Conversation::for_session(store, id)
}

fn detail(store: &SessionStore, id: &str) -> SessionDetail {
    store.get_session(id).unwrap().unwrap()
}

fn seed_two_turns(store: &SessionStore, id: &str) {
    let transcript = serde_json::json!([
        {"role": "user", "content": "p1"},
        {"role": "assistant", "content": "a1", "blocks": []},
        {"role": "user", "content": "p2"},
        {"role": "assistant", "content": "a2", "blocks": []}
    ])
    .to_string();
    let history = serde_json::to_string(&vec![
        Message::user("p1"),
        Message::assistant("a1"),
        Message::user("p2"),
        Message::assistant("a2"),
    ])
    .unwrap();
    store
        .persist_turn(id, &transcript, Some(&history))
        .unwrap();
}

#[test]
fn retry_truncates_model_history_but_keeps_the_prompt_visible() {
    let (store, id) = store_and_conversation();
    seed_two_turns(&store, &id);

    let detail = conv(&store, &id)
        .retry(RetryTarget {
            prompt_index: 1,
            expected_user_turns: 2,
            failed_response: false,
            prompt: "p2",
        })
        .unwrap();

    let history = parse_model_history(detail.model_history.as_deref().unwrap()).unwrap();
    assert_eq!(history.len(), 2, "history truncated before the retried prompt");
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&detail.display_transcript).unwrap();
    let user_count = entries.iter().filter(|e| e["role"] == "user").count();
    assert_eq!(user_count, 2, "the retried prompt stays visible");
    assert!(
        !entries.iter().any(|e| e["content"] == "a2"),
        "the replaced answer is removed from the transcript"
    );
}

#[test]
fn retry_rejects_a_stale_turn_count() {
    let (store, id) = store_and_conversation();
    seed_two_turns(&store, &id);
    let error = conv(&store, &id)
        .retry(RetryTarget {
            prompt_index: 0,
            expected_user_turns: 5,
            failed_response: false,
            prompt: "p1",
        })
        .unwrap_err();
    assert!(matches!(error, ConversationError::Stale));
}

#[test]
fn fork_copies_through_the_selected_response_and_leaves_the_source_untouched() {
    let (store, id) = store_and_conversation();
    seed_two_turns(&store, &id);
    let source_before = detail(&store, &id);

    let fork = conv(&store, &id)
        .fork(ForkTarget {
            prompt_index: 0,
            expected_user_turns: 2,
            response_text: "a1",
        })
        .unwrap();

    assert_ne!(fork.id, id);
    let fork_history = parse_model_history(fork.model_history.as_deref().unwrap()).unwrap();
    assert_eq!(fork_history.len(), 2, "fork ends at the selected response");
    let fork_entries: Vec<serde_json::Value> =
        serde_json::from_str(&fork.display_transcript).unwrap();
    assert!(!fork_entries.iter().any(|e| e["content"] == "p2"));

    let source_after = detail(&store, &id);
    assert_eq!(source_before.display_transcript, source_after.display_transcript);
    assert_eq!(source_before.model_history, source_after.model_history);
}

#[test]
fn transcript_page_windows_entries() {
    let (store, id) = store_and_conversation();
    seed_two_turns(&store, &id);
    let (entries_json, _start, total) = conv(&store, &id).transcript_page(Some(1), None).unwrap();
    assert_eq!(total, 4);
    let entries: Vec<serde_json::Value> = serde_json::from_str(&entries_json).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["content"], "a2");
}

#[test]
fn export_projections_redact_or_expose_according_to_format() {
    let (store, id) = store_and_conversation();
    seed_two_turns(&store, &id);
    let conversation = conv(&store, &id);
    let (display_transcript, model_history) = conversation.internal_history().unwrap();
    assert!(model_history.is_some(), "internal keeps raw history");
    assert!(display_transcript.is_array());
    let text = conversation.export_transcript().unwrap();
    assert!(text.contains("p1") && text.contains("a1"));
}

#[test]
fn retry_supports_legacy_failed_turn_and_avoids_double_prefixed_errors() {
    let (store, id) = store_and_conversation();
    let legacy_transcript = serde_json::json!([
        {"role": "user", "content": "p1"},
        {"role": "assistant", "content": "a1", "blocks": []},
        {"role": "user", "content": "p2"},
        {"role": "assistant", "content": "Error: down", "blocks": [], "error": true}
    ])
    .to_string();
    let model_history =
        serde_json::to_string(&vec![Message::user("p1"), Message::assistant("a1")]).unwrap();
    store
        .persist_turn(&id, &legacy_transcript, Some(&model_history))
        .unwrap();

    let retried = conv(&store, &id)
        .retry(RetryTarget {
            prompt_index: 1,
            expected_user_turns: 2,
            failed_response: true,
            prompt: "p2",
        })
        .unwrap();
    assert_eq!(
        parse_model_history(retried.model_history.as_deref().unwrap())
            .unwrap()
            .len(),
        2
    );
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&retried.display_transcript).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[2]["content"], "p2");

    store.persist_turn(&id, "[]", Some("not json")).unwrap();
    let err = conv(&store, &id)
        .retry(RetryTarget {
            prompt_index: 0,
            expected_user_turns: 1,
            failed_response: false,
            prompt: "p1",
        })
        .unwrap_err();
    assert!(matches!(err, ConversationError::InvalidModelHistory(_)));
    assert_eq!(
        err.to_string()
            .matches("Unable to read conversation history: ")
            .count(),
        1
    );

    store
        .persist_turn(&id, "not json", Some(&model_history))
        .unwrap();
    let err = conv(&store, &id)
        .retry(RetryTarget {
            prompt_index: 0,
            expected_user_turns: 1,
            failed_response: false,
            prompt: "p1",
        })
        .unwrap_err();
    assert!(matches!(err, ConversationError::InvalidDisplayTranscript(_)));
    assert_eq!(
        err.to_string()
            .matches("Unable to read conversation transcript: ")
            .count(),
        1
    );

    let valid_transcript = serde_json::json!([
        {"role": "user", "content": "p1"},
        {"role": "assistant", "content": "a1", "blocks": []}
    ])
    .to_string();
    store
        .persist_turn(&id, &valid_transcript, Some(&model_history))
        .unwrap();
    let err = conv(&store, &id)
        .fork(ForkTarget {
            prompt_index: 1,
            expected_user_turns: 1,
            response_text: "a1",
        })
        .unwrap_err();
    assert!(matches!(err, ConversationError::TurnUnavailable));
    let err = conv(&store, &id)
        .fork(ForkTarget {
            prompt_index: 0,
            expected_user_turns: 1,
            response_text: "wrong text",
        })
        .unwrap_err();
    assert!(matches!(err, ConversationError::ResponseMismatch));
}

#[test]
fn operations_on_a_missing_row_error_where_the_old_code_errored() {
    let store = SessionStore::in_memory_for_test().unwrap();
    let conversation = conv(&store, "missing");
    assert!(matches!(
        conversation.transcript_page(None, None),
        Err(ConversationError::NotFound)
    ));
    assert!(matches!(
        conversation.export_debug(),
        Err(ConversationError::NotFound)
    ));
}
