//! Interface-level tests for the Conversation: orchestration across the two
//! histories, asserted against what actually lands in in-memory SQLite.

use super::*;
use crate::llm::{LlmError, StreamCompletionResult};
use crate::session_store::{SessionDetail, SessionStore};
use crate::conversation::transcript::EMPTY_RESPONSE_FALLBACK;
use rig::completion::message::{AssistantContent, UserContent};

fn store_and_conversation() -> (SessionStore, String) {
    let store = SessionStore::in_memory_for_test().unwrap();
    let record = store.create_session("t", "p", "m", None).unwrap();
    (store, record.id)
}

fn completion(text: &str, messages: Vec<Message>) -> StreamCompletionResult {
    StreamCompletionResult {
        full_response: text.to_string(),
        history: Some(messages),
        source_edit_succeeded: true,
        prompt_tokens: 0,
        response_tokens: 0,
        tool_calls: 0,
    }
}

fn exchange(prompt: &str, response: &str) -> Vec<Message> {
    vec![Message::user(prompt), Message::assistant(response)]
}

fn conv<'a>(store: &'a SessionStore, id: &str) -> Conversation<'a> {
    Conversation::for_session(store, id)
}

fn detail(store: &SessionStore, id: &str) -> SessionDetail {
    store.get_session(id).unwrap().unwrap()
}

fn transcript_entries(store: &SessionStore, id: &str) -> Vec<serde_json::Value> {
    serde_json::from_str(&detail(store, id).display_transcript).unwrap()
}

fn history(store: &SessionStore, id: &str) -> Vec<Message> {
    parse_model_history(detail(store, id).model_history.as_deref().unwrap()).unwrap()
}

fn llm_error() -> LlmError {
    LlmError::ProviderError("provider exploded".to_string())
}

#[test]
fn record_success_persists_both_histories_and_activates_the_draft() {
    let (store, id) = store_and_conversation();
    assert_eq!(detail(&store, &id).status, "draft");

    let merged = conv(&store, &id)
        .record_success(&completion("answer", exchange("hi", "answer")))
        .unwrap();
    let merged = merged.expect("merged transcript returned for memory ingest");

    let detail = detail(&store, &id);
    assert_eq!(detail.status, "active", "persist_turn activates drafts");
    assert_eq!(detail.display_transcript, merged);
    let history = history(&store, &id);
    assert_eq!(history.len(), 2);

    let entries = transcript_entries(&store, &id);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["role"], "user");
    assert_eq!(entries[0]["content"], "hi");
    assert_eq!(entries[1]["content"], "answer");
}

#[test]
fn record_success_without_provider_history_writes_nothing() {
    let (store, id) = store_and_conversation();
    let mut no_history = completion("answer", vec![]);
    no_history.history = None;
    let result = conv(&store, &id).record_success(&no_history).unwrap();
    assert_eq!(result, None);
    assert_eq!(detail(&store, &id).model_history, None);
}

#[test]
fn record_success_applies_the_empty_response_fallback_to_display_only() {
    let (store, id) = store_and_conversation();
    conv(&store, &id)
        .record_success(&completion("", exchange("hi", "")))
        .unwrap();
    let entries = transcript_entries(&store, &id);
    assert_eq!(entries[1]["content"], EMPTY_RESPONSE_FALLBACK);
    // The fallback never enters Model History: the persisted history is the
    // verbatim provider history.
    let history = history(&store, &id);
    match &history[1] {
        Message::Assistant(message) => {
            let empty = message.content.iter().any(|c| matches!(c, AssistantContent::Text(t) if t.text.is_empty()));
            assert!(empty, "the assistant entry is the verbatim empty provider response");
        }
        other => panic!("expected assistant message, got {other:?}"),
    }
}

#[test]
fn record_success_merges_across_turns_preserving_failed_rows() {
    let (store, id) = store_and_conversation();
    conv(&store, &id)
        .record_success(&completion("first answer", exchange("first", "first answer")))
        .unwrap();
    conv(&store, &id)
        .record_failure(FailedTurn {
            display_prompt: "second",
            model_prompt: "second",
            error: &llm_error(),
        })
        .unwrap();
    conv(&store, &id)
        .record_success(&completion(
            "third answer",
            vec![
                Message::user("first"),
                Message::assistant("first answer"),
                Message::user("second"),
                Message::user("third"),
                Message::assistant("third answer"),
            ],
        ))
        .unwrap();

    let entries = transcript_entries(&store, &id);
    let user_contents: Vec<&str> = entries
        .iter()
        .filter(|e| e["role"] == "user")
        .map(|e| e["content"].as_str().unwrap())
        .collect();
    assert_eq!(user_contents, vec!["first", "second", "third"]);
    let failure = entries.iter().find(|e| e["error"] == true);
    assert!(failure.is_some(), "failed turn row survives the merge");
}

#[test]
fn record_failure_appends_prompt_only_to_model_history() {
    let (store, id) = store_and_conversation();
    conv(&store, &id)
        .record_success(&completion("answer", exchange("first", "answer")))
        .unwrap();
    conv(&store, &id)
        .record_failure(FailedTurn {
            display_prompt: "shown prompt",
            model_prompt: "effective prompt",
            error: &llm_error(),
        })
        .unwrap();

    let history = history(&store, &id);
    assert_eq!(history.len(), 3, "only the effective prompt is appended");
    match &history[2] {
        Message::User { content } => {
            let text = content.iter().find_map(|c| match c {
                UserContent::Text(t) => Some(t.text.clone()),
                _ => None,
            });
            assert_eq!(text.as_deref(), Some("effective prompt"));
        }
        other => panic!("expected user message, got {other:?}"),
    }

    let entries = transcript_entries(&store, &id);
    let last_user = entries.iter().rfind(|e| e["role"] == "user").unwrap();
    assert_eq!(last_user["content"], "shown prompt");
    let failure = entries.iter().last().unwrap();
    assert_eq!(failure["role"], "assistant");
    assert_eq!(failure["error"], true);
    assert_eq!(failure["prompt_in_model_history"], true);
}

#[test]
fn record_failure_with_malformed_persisted_history_leaves_it_unchanged() {
    let (store, id) = store_and_conversation();
    store
        .persist_turn(&id, "[]", Some("this is not json"))
        .unwrap();
    conv(&store, &id)
        .record_failure(FailedTurn {
            display_prompt: "p",
            model_prompt: "p",
            error: &llm_error(),
        })
        .unwrap();

    let detail = detail(&store, &id);
    assert_eq!(
        detail.model_history.as_deref(),
        Some("this is not json"),
        "malformed history is left byte-for-byte unchanged"
    );
    let entries = transcript_entries(&store, &id);
    let failure = entries
        .iter()
        .find(|e| e["error"] == true)
        .expect("failure row present");
    assert_eq!(failure["prompt_in_model_history"], false, "row is display-only");
}

#[test]
fn record_failure_on_a_missing_row_is_a_noop() {
    let store = SessionStore::in_memory_for_test().unwrap();
    conv(&store, "no-such-session")
        .record_failure(FailedTurn {
            display_prompt: "p",
            model_prompt: "p",
            error: &llm_error(),
        })
        .unwrap();
}

#[test]
fn continuation_reads_migrates_and_prunes_without_writing_back() {
    let (store, id) = store_and_conversation();
    conv(&store, &id)
        .record_success(&completion("answer", exchange("hi", "answer")))
        .unwrap();
    let before = detail(&store, &id).model_history.clone();
    let continuation = conv(&store, &id).continuation();
    assert_eq!(continuation.len(), 2);
    assert_eq!(
        detail(&store, &id).model_history,
        before,
        "reading the continuation never writes"
    );
}

#[test]
fn continuation_is_empty_for_missing_or_malformed_history() {
    let store = SessionStore::in_memory_for_test().unwrap();
    assert!(conv(&store, "missing").continuation().is_empty());
    let record = store.create_session("t", "p", "m", None).unwrap();
    assert!(conv(&store, &record.id).continuation().is_empty());
    store.persist_turn(&record.id, "[]", Some("garbage")).unwrap();
    assert!(conv(&store, &record.id).continuation().is_empty());
}

#[test]
fn retry_truncates_model_history_but_keeps_the_prompt_visible() {
    let (store, id) = store_and_conversation();
    conv(&store, &id)
        .record_success(&completion("a1", exchange("p1", "a1")))
        .unwrap();
    conv(&store, &id)
        .record_success(&completion(
            "a2",
            vec![
                Message::user("p1"),
                Message::assistant("a1"),
                Message::user("p2"),
                Message::assistant("a2"),
            ],
        ))
        .unwrap();

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
    conv(&store, &id)
        .record_success(&completion("a1", exchange("p1", "a1")))
        .unwrap();
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
    conv(&store, &id)
        .record_success(&completion(
            "a2",
            vec![
                Message::user("p1"),
                Message::assistant("a1"),
                Message::user("p2"),
                Message::assistant("a2"),
            ],
        ))
        .unwrap();
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
    conv(&store, &id)
        .record_success(&completion("a1", exchange("p1", "a1")))
        .unwrap();
    let page = conv(&store, &id)
        .transcript_page(TranscriptPageRequest {
            tail: Some(1),
            before: None,
        })
        .unwrap();
    assert_eq!(page.total, 2);
    let entries: Vec<serde_json::Value> = serde_json::from_str(&page.entries_json).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["content"], "a1");
}

#[test]
fn export_projections_redact_or_expose_according_to_format() {
    let (store, id) = store_and_conversation();
    conv(&store, &id)
        .record_success(&completion("a1", exchange("p1", "a1")))
        .unwrap();
    let conversation = conv(&store, &id);
    match conversation.export_projection(ExportFormat::Internal).unwrap() {
        ExportProjection::Internal(internal) => {
            assert!(internal.model_history.is_some(), "internal keeps raw history");
            assert!(internal.display_transcript.is_array());
        }
        other => panic!("expected internal projection, got {other:?}"),
    }
    match conversation.export_projection(ExportFormat::Transcript).unwrap() {
        ExportProjection::Transcript(text) => {
            assert!(text.contains("p1") && text.contains("a1"));
        }
        _ => panic!("expected transcript projection"),
    }
}

#[test]
fn skill_harvest_entries_expose_user_text_and_tool_names() {
    let (store, id) = store_and_conversation();
    conv(&store, &id)
        .record_success(&completion("a1", exchange("/skill do a thing", "a1")))
        .unwrap();
    let entries = conv(&store, &id).skill_harvest_entries().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0],
        SkillHarvestEntry::User {
            content: "/skill do a thing".to_string()
        }
    );
    assert!(matches!(entries[1], SkillHarvestEntry::Assistant { .. }));
}

#[test]
fn operations_on_a_missing_row_error_where_the_old_code_errored() {
    let store = SessionStore::in_memory_for_test().unwrap();
    let conversation = conv(&store, "missing");
    assert!(matches!(
        conversation.transcript_page(TranscriptPageRequest::default()),
        Err(ConversationError::NotFound)
    ));
    assert!(matches!(
        conversation.export_projection(ExportFormat::Debug),
        Err(ConversationError::NotFound)
    ));
    assert!(matches!(
        conversation.skill_harvest_entries(),
        Err(ConversationError::NotFound)
    ));
}
