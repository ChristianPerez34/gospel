use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::providers::{anthropic, gemini, groq, mistral, openai};
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

pub struct LlmService;

impl LlmService {
    pub async fn completion(
        provider: &str,
        prompt: &str,
        model: &str,
        api_key: &str,
    ) -> Result<String, LlmError> {
        if api_key.trim().is_empty() {
            return Err(LlmError::ApiKeyMissing);
        }

        let response = match provider {
            "openai" => {
                let client = openai::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent.prompt(prompt).await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "anthropic" => {
                let client = anthropic::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent.prompt(prompt).await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "gemini" => {
                let client = gemini::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent.prompt(prompt).await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "groq" => {
                let client = groq::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent.prompt(prompt).await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            "mistral" => {
                let client = mistral::Client::new(api_key)
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?;
                let agent = client.agent(model).build();
                agent.prompt(prompt).await
                    .map_err(|e| LlmError::ProviderError(e.to_string()))?
            }
            _ => return Err(LlmError::UnsupportedProvider(provider.to_string())),
        };
        Ok(response)
    }
}

pub async fn stream_completion<F>(
    provider: &str,
    prompt: &str,
    model: &str,
    api_key: &str,
    mut on_token: F,
) -> Result<String, LlmError>
where
    F: FnMut(String),
{
    if api_key.trim().is_empty() {
        return Err(LlmError::ApiKeyMissing);
    }

    let mut full_response = String::new();

    macro_rules! stream_from_client {
        ($client:expr, $model:expr) => {{
            let agent = $client.agent($model).build();
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
        }};
    }

    match provider {
        "openai" => {
            let client = openai::Client::new(api_key)
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "anthropic" => {
            let client = anthropic::Client::new(api_key)
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "gemini" => {
            let client = gemini::Client::new(api_key)
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "groq" => {
            let client = groq::Client::new(api_key)
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        "mistral" => {
            let client = mistral::Client::new(api_key)
                .map_err(|e| LlmError::ProviderError(e.to_string()))?;
            stream_from_client!(client, model);
        }
        _ => return Err(LlmError::UnsupportedProvider(provider.to_string())),
    }

    Ok(full_response)
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
        let mut token_count = 0;
        let error = stream_completion("openai", "hello", "gpt-4o-mini", "", |_| {
            token_count += 1;
        })
        .await
        .unwrap_err();

        assert!(matches!(error, LlmError::ApiKeyMissing));
        assert_eq!(token_count, 0);
    }
}
