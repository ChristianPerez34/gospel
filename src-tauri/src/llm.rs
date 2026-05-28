use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::completion::message::Message;
use rig::completion::Prompt;
use rig::providers::{anthropic, chatgpt, gemini, groq, mistral, openai};
use rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingChat, StreamingPrompt};
use serde::Serialize;
use std::path::PathBuf;
use thiserror::Error;

use crate::corpus::tools::{
    create_corpus_neighbors_tool, create_corpus_query_tool, create_corpus_summary_tool,
    CORPUS_SYSTEM_PROMPT,
};

#[derive(Error, Debug)]
pub enum LlmError {
    #[error("API key not configured for provider")]
    ApiKeyMissing,
    #[error("provider error: {0}")]
    ProviderError(String),
    #[error("model {0} is not available")]
    ModelUnavailable(String),
    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),
}

#[derive(Serialize, Clone)]
pub struct LlmErrorDto {
    pub code: String,
    pub message: String,
}

impl LlmError {
    pub fn to_dto(&self) -> LlmErrorDto {
        match self {
            LlmError::ApiKeyMissing => LlmErrorDto {
                code: "API_KEY_MISSING".to_string(),
                message: "API key not configured. Open Settings to add one.".to_string(),
            },
            LlmError::ProviderError(msg) => LlmErrorDto {
                code: "PROVIDER_ERROR".to_string(),
                message: format!("Completion failed: {}", msg),
            },
            LlmError::ModelUnavailable(model) => LlmErrorDto {
                code: "MODEL_UNAVAILABLE".to_string(),
                message: format!("Model {} is not available", model),
            },
            LlmError::UnsupportedProvider(provider) => LlmErrorDto {
                code: "UNSUPPORTED_PROVIDER".to_string(),
                message: format!("Provider {} is not supported", provider),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StreamEvent {
    Text(String),
    ToolCall { name: String, arguments: serde_json::Value },
    ToolResult { name: String, result: String },
}

pub struct LlmService;

fn validate_api_key(provider: &str, api_key: &str) -> Result<(), LlmError> {
    if provider != "chatgpt" && api_key.trim().is_empty() {
        return Err(LlmError::ApiKeyMissing);
    }

    Ok(())
}

impl LlmService {
    pub async fn completion(
        provider: &str,
        prompt: &str,
        model: &str,
        api_key: &str,
    ) -> Result<String, LlmError> {
        validate_api_key(provider, api_key)?;

        let response = match provider {
            "openai" => {
                let client = openai::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent
                    .prompt(prompt)
                    .await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "chatgpt" => {
                let client = chatgpt::Client::builder()
                    .oauth()
                    .build()
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent
                    .prompt(prompt)
                    .await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "anthropic" => {
                let client = anthropic::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent
                    .prompt(prompt)
                    .await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "gemini" => {
                let client = gemini::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent
                    .prompt(prompt)
                    .await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "groq" => {
                let client = groq::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent
                    .prompt(prompt)
                    .await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "mistral" => {
                let client = mistral::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent
                    .prompt(prompt)
                    .await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            _ => return Err(LlmError::UnsupportedProvider(provider.to_string())),
        };
        Ok(response)
    }
}

#[derive(Debug)]
pub struct StreamCompletionResult {
    pub full_response: String,
    pub history: Option<Vec<Message>>,
}

pub async fn stream_completion<F>(
    provider: &str,
    prompt: &str,
    model: &str,
    api_key: &str,
    workspace_path: Option<PathBuf>,
    chat_history: Vec<Message>,
    mut on_event: F,
) -> Result<StreamCompletionResult, LlmError>
where
    F: FnMut(StreamEvent),
{
    validate_api_key(provider, api_key)?;

    let mut full_response = String::new();
    let mut captured_history: Option<Vec<Message>> = None;

    macro_rules! stream_from_client {
        ($client:expr, $model:expr) => {{
            let summary_tool = create_corpus_summary_tool(workspace_path.clone());
            let query_tool = create_corpus_query_tool(workspace_path.clone());
            let neighbors_tool = create_corpus_neighbors_tool(workspace_path.clone());

            let agent = $client
                .agent($model)
                .preamble(CORPUS_SYSTEM_PROMPT)
                .tool(summary_tool)
                .tool(query_tool)
                .tool(neighbors_tool)
                .default_max_turns(5)
                .build();

            let request = agent.stream_chat(prompt, chat_history).multi_turn(5);
            let mut stream = request.await;

            let mut tool_name_by_id: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();

            while let Some(item) = stream.next().await {
                match item {
                    Ok(item) => match item {
                        MultiTurnStreamItem::StreamAssistantItem(
                            StreamedAssistantContent::Text(text),
                        ) => {
                            full_response.push_str(&text.text);
                            on_event(StreamEvent::Text(text.text.clone()));
                        }
                        MultiTurnStreamItem::StreamAssistantItem(
                            StreamedAssistantContent::ToolCall {
                                tool_call,
                                internal_call_id,
                            },
                        ) => {
                            tool_name_by_id.insert(
                                internal_call_id.clone(),
                                tool_call.function.name.clone(),
                            );
                            on_event(StreamEvent::ToolCall {
                                name: tool_call.function.name.clone(),
                                arguments: tool_call.function.arguments.clone(),
                            });
                        }
                        MultiTurnStreamItem::StreamUserItem(
                            StreamedUserContent::ToolResult {
                                tool_result,
                                internal_call_id,
                            },
                        ) => {
                            let result_summary = tool_result
                                .content
                                .iter()
                                .filter_map(|c| match c {
                                    rig::completion::message::ToolResultContent::Text(t) => {
                                        Some(t.text.clone())
                                    }
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            let tool_name = tool_name_by_id
                                .get(&internal_call_id)
                                .cloned()
                                .unwrap_or_else(|| tool_result.id.clone());
                            on_event(StreamEvent::ToolResult {
                                name: tool_name,
                                result: result_summary,
                            });
                        }
                        MultiTurnStreamItem::FinalResponse(final_response) => {
                            full_response = final_response.response().to_owned();
                            captured_history = final_response.history().map(|h| h.to_vec());
                            break;
                        }
                        _ => {}
                    },
                    Err(error) => {
                        return Err(LlmError::ProviderError(error.to_string()));
                    }
                }
            }
        }};
    }

    match provider {
        "openai" => {
            let client =
                openai::Client::new(api_key).map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "chatgpt" => {
            let client = chatgpt::Client::builder()
                .oauth()
                .build()
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "anthropic" => {
            let client = anthropic::Client::new(api_key)
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "gemini" => {
            let client =
                gemini::Client::new(api_key).map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "groq" => {
            let client =
                groq::Client::new(api_key).map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "mistral" => {
            let client = mistral::Client::new(api_key)
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        _ => return Err(LlmError::UnsupportedProvider(provider.to_string())),
    }

    Ok(StreamCompletionResult {
        full_response,
        history: captured_history,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn completion_rejects_blank_api_key() {
        let error = LlmService::completion("openai", "hello", "gpt-4o-mini", "  ")
            .await
            .unwrap_err();

        assert!(matches!(error, LlmError::ApiKeyMissing));
    }

    #[tokio::test]
    async fn stream_completion_rejects_blank_api_key() {
        let mut events = vec![];
        let error = stream_completion(
            "openai",
            "hello",
            "gpt-4o-mini",
            "",
            None,
            vec![],
            |event| events.push(event),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, LlmError::ApiKeyMissing));
        assert!(events.is_empty());
    }

    #[test]
    fn validate_api_key_allows_blank_key_for_chatgpt() {
        let result = validate_api_key("chatgpt", "   ");

        assert!(result.is_ok());
    }

    #[test]
    fn validate_api_key_rejects_blank_key_for_non_chatgpt() {
        let result = validate_api_key("openai", "");

        assert!(matches!(result, Err(LlmError::ApiKeyMissing)));
    }
}