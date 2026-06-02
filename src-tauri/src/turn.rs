use crate::app_config::AppConfigState;
use crate::conversation::ConversationState;
use crate::keychain;
use crate::llm::{self, LlmError, LlmErrorDto, StreamEvent, WorkspaceToolContext};
use crate::{
    corpus_auto_build_failure_payload, emit_corpus_auto_build_complete, ensure_workspace_corpus,
    validate_active_workspace_path,
};
use std::path::PathBuf;
use tauri::Emitter;

pub struct StreamingTurnRequest {
    pub provider: String,
    pub prompt: String,
    pub model: String,
    pub session_id: Option<String>,
}

pub async fn run_streaming_turn(
    app: &tauri::AppHandle,
    app_config: &AppConfigState,
    conversation_state: &ConversationState,
    request: StreamingTurnRequest,
) -> Result<(), LlmErrorDto> {
    let workspace_path = active_workspace_path(app_config);
    eprintln!(
        "[CORPUS-AUTO] complete_streaming workspace path: {}",
        workspace_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".to_string())
    );

    let workspace_context = prepare_workspace_context(app, workspace_path).await;
    let api_key = resolve_provider_credential(&request.provider)?;
    let chat_history = conversation_history(conversation_state, request.session_id.as_deref());

    let app_clone = app.clone();
    let result = llm::stream_completion(
        &request.provider,
        &request.prompt,
        &request.model,
        &api_key,
        workspace_context,
        chat_history,
        move |event| emit_stream_event(&app_clone, event),
    )
    .await;

    match result {
        Ok(stream_result) => {
            if let (Some(session_id), Some(history)) =
                (request.session_id.as_deref(), stream_result.history)
            {
                let mut store = conversation_state.store.lock().unwrap();
                store.store_history(session_id, history);
            }
            let _ = app.emit("llm-done", stream_result.full_response);
            Ok(())
        }
        Err(error) => {
            let dto = error.to_dto();
            let _ = app.emit("llm-error", dto.clone());
            Err(dto)
        }
    }
}

fn active_workspace_path(app_config: &AppConfigState) -> Option<PathBuf> {
    match &app_config.store {
        Some(store) => store.get_workspace_path().ok().flatten().map(PathBuf::from),
        None => None,
    }
}

async fn prepare_workspace_context(
    app: &tauri::AppHandle,
    workspace_path: Option<PathBuf>,
) -> Option<WorkspaceToolContext> {
    let path = workspace_path?;

    match validate_active_workspace_path(&path) {
        Ok(()) => {
            let ensure_result = ensure_workspace_corpus(app, &path).await;
            let (context, corpus_error) = workspace_context_for_corpus_result(path, ensure_result);
            if let Some(error) = corpus_error {
                tracing::warn!(
                    "[CORPUS-AUTO] continuing without corpus for {}: {}",
                    context.workspace_path.display(),
                    error
                );
                emit_corpus_auto_build_complete(app, corpus_auto_build_failure_payload());
            }
            Some(context)
        }
        Err(error) => {
            tracing::warn!(
                "[CORPUS-AUTO] workspace tools unavailable for {}: {}",
                path.display(),
                error
            );
            emit_corpus_auto_build_complete(app, corpus_auto_build_failure_payload());
            None
        }
    }
}

fn workspace_context_for_corpus_result(
    workspace_path: PathBuf,
    corpus_result: Result<Option<usize>, String>,
) -> (WorkspaceToolContext, Option<String>) {
    match corpus_result {
        Ok(_) => (
            WorkspaceToolContext {
                workspace_path,
                corpus_available: true,
            },
            None,
        ),
        Err(error) => (
            WorkspaceToolContext {
                workspace_path,
                corpus_available: false,
            },
            Some(error),
        ),
    }
}

fn resolve_provider_credential(provider: &str) -> Result<String, LlmErrorDto> {
    credential_for_provider(provider, keychain::retrieve)
}

fn credential_for_provider<R>(provider: &str, retrieve: R) -> Result<String, LlmErrorDto>
where
    R: FnOnce(&str) -> Result<String, keychain::KeychainError>,
{
    if provider == "chatgpt" {
        return Ok(String::new());
    }

    retrieve(provider).map_err(|_| LlmError::ApiKeyMissing.to_dto())
}

fn conversation_history(
    conversation_state: &ConversationState,
    session_id: Option<&str>,
) -> Vec<rig::completion::message::Message> {
    match session_id {
        Some(session_id) => {
            let mut store = conversation_state.store.lock().unwrap();
            store.get_history(session_id)
        }
        None => vec![],
    }
}

fn emit_stream_event(app: &tauri::AppHandle, event: StreamEvent) {
    match event {
        StreamEvent::Text(token) => {
            let _ = app.emit("llm-token", token);
        }
        StreamEvent::ToolCall {
            id,
            name,
            arguments,
        } => {
            let _ = app.emit(
                "llm-tool-call",
                serde_json::json!({ "id": id, "name": name, "arguments": arguments }),
            );
        }
        StreamEvent::ToolResult { id, name, result } => {
            let _ = app.emit(
                "llm-tool-result",
                serde_json::json!({ "id": id, "name": name, "result": result }),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chatgpt_turn_credential_skips_keychain() {
        let mut called = false;

        let credential_result = credential_for_provider("chatgpt", |_| {
            called = true;
            Err(keychain::KeychainError::UnsupportedProvider(
                "chatgpt".to_string(),
            ))
        });
        let credential = match credential_result {
            Ok(credential) => credential,
            Err(error) => panic!("unexpected credential error: {}", error.message),
        };

        assert_eq!(credential, "");
        assert!(!called);
    }

    #[test]
    fn api_key_turn_credential_uses_keychain() {
        let credential_result = credential_for_provider("openai", |provider| {
            assert_eq!(provider, "openai");
            Ok("sk-test".to_string())
        });
        let credential = match credential_result {
            Ok(credential) => credential,
            Err(error) => panic!("unexpected credential error: {}", error.message),
        };

        assert_eq!(credential, "sk-test");
    }

    #[test]
    fn api_key_turn_credential_errors_map_to_missing_key() {
        let error = credential_for_provider("openai", |_| {
            Err(keychain::KeychainError::UnsupportedProvider(
                "openai".to_string(),
            ))
        })
        .unwrap_err();

        assert_eq!(error.code, "API_KEY_MISSING");
    }

    #[test]
    fn failed_corpus_still_enables_live_workspace_tools() {
        let workspace_path = PathBuf::from("/tmp/gospel-workspace");

        let (context, corpus_error) = workspace_context_for_corpus_result(
            workspace_path.clone(),
            Err("corpus build failed".to_string()),
        );

        assert_eq!(context.workspace_path, workspace_path);
        assert!(!context.corpus_available);
        assert_eq!(corpus_error.as_deref(), Some("corpus build failed"));
    }

    #[test]
    fn successful_corpus_marks_corpus_available() {
        let workspace_path = PathBuf::from("/tmp/gospel-workspace");

        let (context, corpus_error) =
            workspace_context_for_corpus_result(workspace_path.clone(), Ok(None));

        assert_eq!(context.workspace_path, workspace_path);
        assert!(context.corpus_available);
        assert!(corpus_error.is_none());
    }
}
