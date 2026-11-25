//! The Conversation: the single owner of a Session's Display Transcript and
//! Model History. Every write and every interpreting read of the two history
//! columns crosses this interface. Verbatim row copying (archive, restore,
//! import) and Session metadata stay in the Session Store; workspace
//! authorization, in-flight-turn guards, cancellation, and Session Turn Events
//! stay outside.

use super::projection;
use super::slicing;
use crate::session_store::{SessionDetail, SessionStore, SessionStoreError};

/// One Session's Conversation, borrowed over the Session Store.
///
/// Construction is total and performs no I/O. Every operation re-reads the
/// persisted row it needs, so a Conversation never becomes a stale copy.
/// Writes ride on `SessionStore::persist_turn`, whose SQL already moves a
/// Draft Session to `active`; no caller performs a separate activation write.
pub(crate) struct Conversation<'store> {
    store: &'store SessionStore,
    session_id: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConversationError {
    #[error(transparent)]
    Store(#[from] SessionStoreError),
    #[error("Session not found")]
    NotFound,
    #[error("The saved conversation has changed. Reopen the task and try again.")]
    Stale,
    #[error("This response is no longer available in the saved conversation.")]
    TurnUnavailable,
    #[error("This response does not match the saved conversation. Reopen the task and try again.")]
    ResponseMismatch,
    #[error("Unable to read conversation history: {0}")]
    InvalidModelHistory(String),
    #[error("Unable to read conversation transcript: {0}")]
    InvalidDisplayTranscript(String),
    #[error("Unable to save conversation transcript: {0}")]
    UnwriteableDisplayTranscript(String),
    #[error("Unable to save conversation history")]
    UnwriteableModelHistory,
}

/// Selection for a Retry. `prompt_index` is the zero-based position of the
/// user prompt to rerun, counting only user-turn starts (tool-result messages
/// never count). `expected_user_turns` is the caller's optimistic freshness
/// check; `failed_response` permits the legacy exception for pre-ADR-0017
/// failure rows whose prompt never reached Model History.
pub(crate) struct RetryTarget<'a> {
    pub prompt_index: usize,
    pub expected_user_turns: usize,
    pub failed_response: bool,
    pub prompt: &'a str,
}

/// Selection for a Fork. `prompt_index` is the zero-based user-turn position
/// whose assistant response ends the fork. `response_text` is matched with
/// whitespace normalization against visible assistant text within that turn.
pub(crate) struct ForkTarget<'a> {
    pub prompt_index: usize,
    pub expected_user_turns: usize,
    pub response_text: &'a str,
}

/// Export formats understood by the Conversation. Also the Tauri wire type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ExportFormat {
    Transcript,
    Debug,
    Internal,
}

impl<'store> Conversation<'store> {
    pub(crate) fn for_session(store: &'store SessionStore, session_id: &str) -> Self {
        Self {
            store,
            session_id: session_id.to_string(),
        }
    }

    fn detail(&self) -> Result<Option<SessionDetail>, ConversationError> {
        Ok(self.store.get_session(&self.session_id)?)
    }

    /// Prepare a Retry: validate the optimistic turn count, truncate Model
    /// History immediately before the selected prompt, and truncate the
    /// Display Transcript through it — the retried prompt deliberately stays
    /// visible while its replacement runs.
    pub(crate) fn retry(&self, target: RetryTarget<'_>) -> Result<SessionDetail, ConversationError> {
        let detail = self.detail()?.ok_or(ConversationError::NotFound)?;
        let raw_history = detail.model_history.as_deref();
        slicing::validate_turn_count(
            raw_history,
            target.expected_user_turns,
            target.failed_response.then_some(target.prompt_index),
        )?;
        let history = slicing::history_before_turn(raw_history, target.prompt_index)?;
        let model_history =
            serde_json::to_string(&history).map_err(|_| ConversationError::UnwriteableModelHistory)?;
        let display_transcript = slicing::transcript_through_prompt(
            &detail.display_transcript,
            target.prompt_index,
            target.prompt,
        )?;
        self.store.persist_turn(
            &self.session_id,
            &display_transcript,
            Some(&model_history),
        )?;
        self.detail()?.ok_or(ConversationError::NotFound)
    }

    /// Create a Fork ending at the selected assistant response. The source
    /// Conversation is unchanged; the copy retains complete tool-call/result
    /// pairs through that response, plus everything the user saw — including
    /// turns already pruned from Model History. Metadata and agent binding are
    /// copied by the SessionStore SQL.
    pub(crate) fn fork(&self, target: ForkTarget<'_>) -> Result<SessionDetail, ConversationError> {
        let detail = self.detail()?.ok_or(ConversationError::NotFound)?;
        let raw_history = detail.model_history.as_deref();
        slicing::validate_turn_count(raw_history, target.expected_user_turns, None)?;
        let history = slicing::history_through_response(
            raw_history,
            target.prompt_index,
            target.response_text,
        )?;
        let model_history =
            serde_json::to_string(&history).map_err(|_| ConversationError::UnwriteableModelHistory)?;
        let display_transcript = slicing::transcript_through_response(
            &detail.display_transcript,
            target.prompt_index,
            target.response_text,
        )?;
        Ok(self.store.fork_session(
            &self.session_id,
            &display_transcript,
            &model_history,
        )?)
    }

    /// Read one window of the Display Transcript. `before` is exclusive and
    /// clamped; absent `tail` means the full prefix. Malformed transcript JSON
    /// pages as empty. Never touches Model History.
    pub(crate) fn transcript_page(
        &self,
        tail: Option<u32>,
        before: Option<u32>,
    ) -> Result<(String, usize, usize), ConversationError> {
        let detail = self.detail()?.ok_or(ConversationError::NotFound)?;
        Ok(projection::slice_display_transcript(
            &detail.display_transcript,
            tail,
            before,
        ))
    }

    pub(crate) fn export_transcript(&self) -> Result<String, ConversationError> {
        let detail = self.detail()?.ok_or(ConversationError::NotFound)?;
        Ok(projection::project_transcript(&detail.display_transcript))
    }

    pub(crate) fn export_debug(&self) -> Result<String, ConversationError> {
        let detail = self.detail()?.ok_or(ConversationError::NotFound)?;
        Ok(projection::project_debug(&detail.display_transcript))
    }

    pub(crate) fn internal_history(
        &self,
    ) -> Result<(serde_json::Value, Option<serde_json::Value>), ConversationError> {
        let detail = self.detail()?.ok_or(ConversationError::NotFound)?;
        Ok((
            serde_json::from_str(&detail.display_transcript).unwrap_or(serde_json::json!([])),
            detail
                .model_history
                .as_ref()
                .and_then(|history| serde_json::from_str(history).ok()),
        ))
    }
}
