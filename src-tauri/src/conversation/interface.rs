//! The Conversation: the single owner of a Session's Display Transcript and
//! Model History. Every write and every interpreting read of the two history
//! columns crosses this interface. Verbatim row copying (archive, restore,
//! import) and Session metadata stay in the Session Store; workspace
//! authorization, in-flight-turn guards, cancellation, and Session Turn Events
//! stay outside.

use super::migration::parse_model_history;
use super::projection;
use super::pruning::prune_history_for_storage;
use super::slicing;
use super::transcript::{self, merge_display_transcripts, successful_turn_persistence, FailedTurn, EMPTY_RESPONSE_FALLBACK};
use crate::llm::StreamCompletionResult;
use crate::session_store::{SessionDetail, SessionStore, SessionStoreError};
use rig::completion::message::Message;

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

/// One transcript entry in the exact form skill harvesting consumes: user
/// text as displayed, or the ordered non-empty tool names of one assistant
/// entry. Entries of other roles are dropped; malformed transcripts yield an
/// empty feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SkillHarvestEntry {
    User { content: String },
    Assistant { tool_names: Vec<String> },
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

    /// Load the provider-native continuation for a new Session Turn: read the
    /// row, run legacy migration, then prune — without writing either back.
    /// Deliberately total: a missing row, missing history, unreadable row, or
    /// malformed history yields an empty continuation after logging, matching
    /// the previous cache-miss behavior. Consecutive user messages survive.
    pub(crate) fn continuation(&self) -> Vec<Message> {
        let detail = match self.detail() {
            Ok(Some(detail)) => detail,
            Ok(None) => return Vec::new(),
            Err(error) => {
                tracing::warn!(
                    "Failed to read session {} for continuation: {}",
                    self.session_id,
                    error
                );
                return Vec::new();
            }
        };
        let Some(raw) = detail.model_history.as_deref() else {
            return Vec::new();
        };
        match parse_model_history(raw) {
            Ok(messages) => prune_history_for_storage(messages),
            Err(error) => {
                tracing::warn!(
                    "Failed to parse model history for session {}: {}",
                    self.session_id,
                    error
                );
                Vec::new()
            }
        }
    }

    /// Persist a successfully completed Session Turn. `completion` must be the
    /// exact result returned by the agent runtime; its history is the full
    /// continuation plus this Turn's exchange. The empty-response fallback is
    /// a Display Transcript rule and never enters Model History.
    ///
    /// Returns the merged Display Transcript for Memory ingest, or `None`
    /// when nothing was written (no persistable history, or the row is gone).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn record_success(
        &self,
        completion: &StreamCompletionResult,
    ) -> Result<Option<String>, ConversationError> {
        self.record_turn_success(None, completion)
    }

    pub(crate) fn record_turn_success(
        &self,
        display_prompt: Option<&str>,
        completion: &StreamCompletionResult,
    ) -> Result<Option<String>, ConversationError> {
        let fallback = completion
            .full_response
            .trim()
            .is_empty()
            .then_some(EMPTY_RESPONSE_FALLBACK);
        if let Some(blocks) = completion.acp_turn_blocks.as_deref() {
            let existing = self
                .detail()?
                .map(|detail| detail.display_transcript)
                .unwrap_or_else(|| "[]".to_string());
            let display_transcript = transcript::acp_turn_display_transcript(
                &existing,
                display_prompt.unwrap_or(""),
                &completion.full_response,
                blocks,
                fallback,
            );
            self.store.persist_turn_with_acp(
                &self.session_id,
                &display_transcript,
                None,
                completion.acp_session_id.as_deref(),
            )?;
            return Ok(Some(display_transcript));
        }
        let Some(persistence) =
            successful_turn_persistence(completion.history.as_deref(), fallback)
        else {
            return Ok(None);
        };
        let existing = self
            .detail()?
            .map(|detail| detail.display_transcript)
            .unwrap_or_else(|| "[]".to_string());
        let display_transcript =
            merge_display_transcripts(&existing, &persistence.display_transcript);
        self.store.persist_turn(
            &self.session_id,
            &display_transcript,
            persistence.model_history.as_deref(),
        )?;
        Ok(Some(display_transcript))
    }

    /// Persist a provider/runtime failure or a Controlled Stop. Only the
    /// effective prompt is appended to Model History, as one plain user
    /// message; partial output and tool activity from the failed run are never
    /// written. A missing row is a no-op; malformed persisted history is left
    /// unchanged and the failure row becomes display-only.
    pub(crate) fn record_failure(&self, turn: FailedTurn<'_>) -> Result<(), ConversationError> {
        let Some(detail) = self.detail()? else {
            return Ok(());
        };
        let is_acp = crate::providers::is_acp_provider(&detail.provider)
            && detail.model_history.is_none();
        let persistence = if is_acp {
            transcript::failure_turn_persistence_without_model_history(
                &detail.display_transcript,
                &turn,
            )
        } else {
            transcript::failure_turn_persistence(
                &detail.display_transcript,
                detail.model_history.as_deref(),
                &turn,
            )
        };
        self.store.persist_turn(
            &self.session_id,
            &persistence.display_transcript,
            persistence.model_history.as_deref(),
        )?;
        Ok(())
    }

    /// Prepare a Retry: validate the optimistic turn count, truncate Model
    /// History immediately before the selected prompt, and truncate the
    /// Display Transcript through it — the retried prompt deliberately stays
    /// visible while its replacement runs.
    pub(crate) fn retry(&self, target: RetryTarget<'_>) -> Result<SessionDetail, ConversationError> {
        let detail = self.detail()?.ok_or(ConversationError::NotFound)?;
        let raw_history = detail.model_history.as_deref();
        if crate::providers::is_acp_provider(&detail.provider) && raw_history.is_none() {
            slicing::validate_display_turn_count(
                &detail.display_transcript,
                target.expected_user_turns,
                target.failed_response.then_some(target.prompt_index),
            )?;
            let display_transcript = slicing::transcript_through_prompt(
                &detail.display_transcript,
                target.prompt_index,
                target.prompt,
            )?;
            self.store.persist_turn_with_acp(
                &self.session_id,
                &display_transcript,
                None,
                None,
            )?;
            return self.detail()?.ok_or(ConversationError::NotFound);
        }
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
        if crate::providers::is_acp_provider(&detail.provider) && raw_history.is_none() {
            slicing::validate_display_turn_count(
                &detail.display_transcript,
                target.expected_user_turns,
                None,
            )?;
            let display_transcript = slicing::transcript_through_response(
                &detail.display_transcript,
                target.prompt_index,
                target.response_text,
            )?;
            return Ok(self.store.fork_session_with_acp(
                &self.session_id,
                &display_transcript,
                None,
                None,
            )?);
        }
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

    pub(crate) fn acp_seed_context(&self) -> Result<Option<String>, ConversationError> {
        let detail = self.detail()?.ok_or(ConversationError::NotFound)?;
        Ok(projection::acp_seed_context(&detail.display_transcript))
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

    /// Read the Display Transcript as the typed feed skill harvesting
    /// consumes, keeping transcript JSON shape out of skill_opt. Assistant
    /// entries are present even with no tool names so ordering between user
    /// turns is preserved.
    pub(crate) fn skill_harvest_entries(
        &self,
    ) -> Result<Vec<SkillHarvestEntry>, ConversationError> {
        let detail = self.detail()?.ok_or(ConversationError::NotFound)?;
        let entries = projection::parse_display_transcript(&detail.display_transcript);
        let mut feed = Vec::new();
        for entry in &entries {
            match entry.get("role").and_then(|role| role.as_str()) {
                Some("user") => feed.push(SkillHarvestEntry::User {
                    content: entry
                        .get("content")
                        .and_then(|content| content.as_str())
                        .unwrap_or("")
                        .to_string(),
                }),
                Some("assistant") => {
                    let mut tool_names = Vec::new();
                    if let Some(blocks) = entry.get("blocks").and_then(|b| b.as_array()) {
                        for block in blocks {
                            if block.get("kind").and_then(|kind| kind.as_str()) != Some("tool") {
                                continue;
                            }
                            if let Some(name) = block.get("name").and_then(|name| name.as_str()) {
                                if !name.is_empty() {
                                    tool_names.push(name.to_string());
                                }
                            }
                        }
                    }
                    feed.push(SkillHarvestEntry::Assistant { tool_names });
                }
                _ => {}
            }
        }
        Ok(feed)
    }
}
