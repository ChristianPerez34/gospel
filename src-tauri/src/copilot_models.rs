use rig::http_client::HttpClientExt;
use rig::providers::copilot::{model_listing::Models, CopilotConfig};
use rig::wire::{Mode, Wire};

use crate::models::{ModelInfo, ModelRegistry};

pub(crate) async fn fetch_models(
    config: &CopilotConfig,
    http: &impl HttpClientExt,
) -> Result<Vec<ModelInfo>, String> {
    // Rig's normalized model list discards Copilot's tool-calling capabilities.
    let request = Models {
        provider: config.clone(),
    }
    .encode(None, Mode::Unary)
    .map_err(|error| format!("failed to build GitHub Copilot model request: {error}"))?
    .request
    .map(|_| Vec::<u8>::new());
    let response = http
        .send::<_, Vec<u8>>(request)
        .await
        .map_err(|error| format!("failed to list GitHub Copilot models: {error}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = response
            .into_body()
            .await
            .ok()
            .map(|body| String::from_utf8_lossy(&body).trim().to_string())
            .filter(|body| !body.is_empty())
            .map(|body| format!(": {body}"))
            .unwrap_or_default();
        return Err(format!(
            "failed to list GitHub Copilot models: HTTP {status}{detail}"
        ));
    }
    let body = response
        .into_body()
        .await
        .map_err(|error| format!("failed to read GitHub Copilot models: {error}"))?;
    let body: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| format!("failed to parse GitHub Copilot models: {error}"))?;
    let entries = body["data"]
        .as_array()
        .ok_or("GitHub Copilot model response is missing the data array")?;
    let mut seen = std::collections::HashSet::new();
    let models: Vec<ModelInfo> = entries
        .iter()
        .filter(|entry| {
            entry
                .pointer("/capabilities/type")
                .and_then(|value| value.as_str())
                == Some("chat")
                && entry
                    .pointer("/capabilities/supports/tool_calls")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
                && entry
                    .pointer("/policy/state")
                    .and_then(serde_json::Value::as_str)
                    != Some("disabled")
        })
        .filter_map(|entry| {
            let id = entry["id"].as_str()?.trim();
            if id.is_empty() || !seen.insert(id.to_owned()) {
                return None;
            }
            let endpoints = entry
                .get("supported_endpoints")
                .and_then(serde_json::Value::as_array)
                .map(|endpoints| {
                    endpoints
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                });
            if endpoints.as_ref().is_some_and(|endpoints| {
                !endpoints.iter().any(|endpoint| {
                    endpoint.starts_with('/')
                        && (endpoint.ends_with("/responses")
                            || endpoint.ends_with("/chat/completions"))
                })
            }) {
                return None;
            }
            Some((id.to_owned(), endpoints))
        })
        .map(|(id, endpoints)| {
            let mut model = ModelRegistry::model_info("github_copilot", &id);
            model.copilot_supported_endpoints = endpoints;
            model
        })
        .collect();

    tracing::info!(
        "Fetched {} models from GitHub Copilot, filtered to {} tool-capable models",
        entries.len(),
        models.len()
    );
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::core::test_utils::RecordingHttpClient;
    use rig::http_client::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn discovers_tool_models_and_keeps_their_supported_endpoints() {
        let http = RecordingHttpClient::new(
            serde_json::to_vec(&json!({"data": [
                {"id": "future-responses-model", "capabilities": {
                    "type": "chat", "supports": {"tool_calls": true}
                }, "supported_endpoints": ["/responses"]},
                {"id": "future-chat-model", "capabilities": {
                    "type": "chat", "supports": {"tool_calls": true}
                }, "supported_endpoints": ["/chat/completions"]},
                {"id": "future-responses-model", "capabilities": {
                    "type": "chat", "supports": {"tool_calls": true}
                }, "supported_endpoints": ["/responses"]},
                {"id": "messages-only", "capabilities": {
                    "type": "chat", "supports": {"tool_calls": true}
                }, "supported_endpoints": ["/v1/messages"]},
                {"id": "policy-disabled", "policy": {"state": "disabled"},
                 "capabilities": {"type": "chat", "supports": {"tool_calls": true}}},
                {"id": "embedding", "capabilities": {
                    "type": "embeddings", "supports": {"tool_calls": true}
                }}
            ]}))
            .unwrap(),
        );
        let config = CopilotConfig::new("test-token")
            .with_base_url("https://api.business.githubcopilot.com");

        let models = fetch_models(&config, &http).await.unwrap();

        assert_eq!(
            models
                .iter()
                .map(|model| model.model.as_str())
                .collect::<Vec<_>>(),
            ["future-responses-model", "future-chat-model"]
        );
        assert_eq!(
            models[0].copilot_supported_endpoints.as_deref(),
            Some(["/responses".to_owned()].as_slice())
        );
        let requests = http.requests();
        assert_eq!(
            requests[0].uri,
            "https://api.business.githubcopilot.com/models"
        );
        assert_eq!(requests[0].headers["authorization"], "Bearer test-token");
    }

    #[tokio::test]
    async fn discovers_models_by_capability_instead_of_model_id() {
        let http = RecordingHttpClient::new(
            serde_json::to_vec(&json!({"data": [
                {"id": "new-tool-capable-model", "capabilities": {
                    "type": "chat", "supports": {"tool_calls": true}
                }},
                {"id": "gemini-future", "capabilities": {
                    "type": "chat", "supports": {"tool_calls": true}
                }},
                {"id": "  trimmed-model  ", "capabilities": {
                    "type": "chat", "supports": {"tool_calls": true}
                }},
                {"id": "new-tool-capable-model", "capabilities": {
                    "type": "chat", "supports": {"tool_calls": true}
                }},
                {"id": "  trimmed-model  ", "capabilities": {
                    "type": "chat", "supports": {"tool_calls": true}
                }},
                {"id": "gpt-4o", "capabilities": {
                    "type": "chat", "supports": {"tool_calls": false}
                }},
                {"id": "embedding", "capabilities": {
                    "type": "embeddings", "supports": {"tool_calls": true}
                }},
                {"id": "missing-support", "capabilities": {"type": "chat"}},
                {"id": "missing-capabilities"},
                {"id": "", "capabilities": {
                    "type": "chat", "supports": {"tool_calls": true}
                }},
                {"capabilities": {"type": "chat", "supports": {"tool_calls": true}}}
            ]}))
            .unwrap(),
        );
        let config = CopilotConfig::new("test-token")
            .with_base_url("https://api.business.githubcopilot.com");
        let models = fetch_models(&config, &http).await.unwrap();
        assert_eq!(
            models.iter().map(|m| m.model.as_str()).collect::<Vec<_>>(),
            ["new-tool-capable-model", "gemini-future", "trimmed-model"]
        );
        assert!(models.iter().all(|m| m.provider == "github_copilot"));
        let requests = http.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].uri,
            "https://api.business.githubcopilot.com/models"
        );
        assert_eq!(requests[0].headers["authorization"], "Bearer test-token");
        assert_eq!(requests[0].headers["copilot-integration-id"], "vscode-chat");
        assert!(requests[0].headers.contains_key("editor-version"));
    }

    #[tokio::test]
    async fn rejects_invalid_responses_and_preserves_http_failures() {
        let config = CopilotConfig::new("test-token");
        for body in ["not json", "{}", "{\"data\": null}"] {
            let http = RecordingHttpClient::new(body.as_bytes().to_vec());
            assert!(fetch_models(&config, &http).await.is_err());
        }
        let http = RecordingHttpClient::with_error_response(
            StatusCode::UNAUTHORIZED,
            br#"{"data": []}"#.to_vec(),
        );
        let err = fetch_models(&config, &http).await.unwrap_err();
        assert!(err.contains("401"));

        let http = RecordingHttpClient::with_error_response(
            StatusCode::FORBIDDEN,
            br#"{"message": "organization policy blocks Copilot"}"#.to_vec(),
        );
        let err = fetch_models(&config, &http).await.unwrap_err();
        assert!(err.contains("403"));
        assert!(err.contains("organization policy blocks Copilot"));

        let http = RecordingHttpClient::with_error(StatusCode::FORBIDDEN, "not entitled");
        assert!(fetch_models(&config, &http)
            .await
            .unwrap_err()
            .contains("403"));
    }

    #[tokio::test]
    async fn accepts_an_empty_catalog() {
        let http = RecordingHttpClient::new(br#"{"data": []}"#.to_vec());
        assert!(fetch_models(&CopilotConfig::new("test-token"), &http)
            .await
            .unwrap()
            .is_empty());
    }
}
