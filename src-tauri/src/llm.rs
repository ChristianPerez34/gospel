use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::providers::openai;
use rig::streaming::{StreamedAssistantContent, StreamingPrompt};
use serde::Serialize;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LlmError {
    #[error("API key not configured for provider")]
    ApiKeyMissing,
    #[error("provider error: {0}")]
    ProviderError(String),
    #[error("model {0} is not available")]
    ModelUnavailable(String),
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
                message: "OpenAI API key not configured. Open Settings to add one.".to_string(),
            },
            LlmError::ProviderError(msg) => LlmErrorDto {
                code: "PROVIDER_ERROR".to_string(),
                message: format!("Completion failed: {}", msg),
            },
            LlmError::ModelUnavailable(model) => LlmErrorDto {
                code: "MODEL_UNAVAILABLE".to_string(),
                message: format!("Model {} is not available", model),
            },
        }
    }
}

pub struct LlmService;

impl LlmService {
    pub async fn completion(prompt: &str, model: &str, api_key: &str) -> Result<String, LlmError> {
        let client =
            openai::Client::new(api_key).map_err(|e| LlmError::ProviderError(e.to_string()))?;
        let agent = client.agent(model).build();
        let response = match agent.prompt(prompt).await {
            Ok(response) => response,
            Err(error) => return Err(LlmError::ProviderError(error.to_string())),
        };
        Ok(response)
    }
}

pub async fn stream_completion<F>(
    prompt: &str,
    model: &str,
    api_key: &str,
    mut on_token: F,
) -> Result<String, LlmError>
where
    F: FnMut(String),
{
    let mut full_response = String::new();

    let client =
        openai::Client::new(api_key).map_err(|e| LlmError::ProviderError(e.to_string()))?;
    let agent = client.agent(model).build();
    let mut stream = agent.stream_prompt(prompt).await;

    while let Some(item) = stream.next().await {
        match item {
            Ok(item) => match item {
                MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(text)) => {
                    full_response.push_str(&text.text);
                    on_token(text.text.clone());
                }
                MultiTurnStreamItem::FinalResponse(final_response) => {
                    full_response = final_response.response().to_owned();
                    break;
                }
                _ => {}
            },
            Err(error) => {
                return Err(LlmError::ProviderError(error.to_string()));
            }
        }
    }

    Ok(full_response)
}
