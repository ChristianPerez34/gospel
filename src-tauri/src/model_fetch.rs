use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

use crate::models::{ModelInfo, ModelInfoWithFreshness, ModelRegistry};
use crate::providers::{provider, ModelFetchKind};

fn should_include_completion_model(model_id: &str) -> bool {
    let id = model_id.to_lowercase();
    let exclude = [
        "embedding",
        "tts",
        "dall-e",
        "whisper",
        "moderation",
        "realtime",
    ];
    if exclude.iter().any(|p| id.contains(p)) {
        return false;
    }
    let include_prefixes = ["gpt-", "o1-", "o3-", "o4-", "o5-"];
    if include_prefixes.iter().any(|p| id.starts_with(p)) {
        return true;
    }

    let include_exact = ["o1", "o3", "o4", "o5"];
    if include_exact.iter().any(|p| id == *p) {
        return true;
    }

    id.contains("codex")
}

pub async fn fetch_models_for_provider(
    provider_id: &str,
    api_key: Option<&str>,
    force_refresh: bool,
) -> ModelInfoWithFreshness {
    let entry = provider(provider_id);
    let cache_scope = entry
        .map(|entry| crate::providers::model_cache_scope_key(entry, api_key))
        .unwrap_or_else(|| api_key.unwrap_or("").to_string());
    let cache_key = format!("{}:{}", provider_id, cache_scope);

    let fetch_models =
        ModelRegistry::get_or_fetch(&cache_key, provider_id, force_refresh, || async {
            let fetch_kind = entry.map(|entry| entry.model_fetch);
            match fetch_kind {
                Some(ModelFetchKind::RigOpenAI) => {
                    fetch_openai_models_impl(api_key.unwrap_or("")).await
                }
                Some(ModelFetchKind::CustomChatGpt) => fetch_chatgpt_models_impl().await,
                Some(ModelFetchKind::RigOauthCopilot) => fetch_github_copilot_models_impl().await,
                Some(ModelFetchKind::CustomGrok) => fetch_grok_models_impl(api_key).await,
                Some(ModelFetchKind::CustomXai) => {
                    fetch_xai_models_impl(api_key.unwrap_or("")).await
                }
                Some(ModelFetchKind::RigAnthropic) => {
                    fetch_anthropic_models_impl(api_key.unwrap_or("")).await
                }
                Some(ModelFetchKind::RigGemini) => {
                    fetch_gemini_models_impl(api_key.unwrap_or("")).await
                }
                Some(ModelFetchKind::RigMistral) => {
                    fetch_mistral_models_impl(api_key.unwrap_or("")).await
                }
                Some(ModelFetchKind::CustomOpenCodeGo) => {
                    fetch_opencode_go_models_impl(api_key.unwrap_or("")).await
                }
                Some(ModelFetchKind::StaticHardcoded) | None => {
                    Ok(ModelRegistry::hardcoded_models_for(provider_id))
                }
            }
        });
    let (_, mut result) = tokio::join!(
        ModelRegistry::refresh_live_model_catalog(force_refresh),
        fetch_models
    );
    for model in &mut result.models {
        model.variants = ModelRegistry::variants_for_model(&model.provider, &model.model);
    }
    result
}

async fn fetch_openai_models_impl(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    let client = rig::providers::openai::OpenAI::new(api_key);
    let list = client
        .list_models()
        .await
        .map_err(|e| format!("failed to list OpenAI models: {}", e))?;

    let total = list.data.len();
    let models: Vec<ModelInfo> = list
        .data
        .into_iter()
        .filter(|m| should_include_completion_model(&m.id))
        .map(|m| ModelRegistry::model_info("openai", &m.id))
        .collect();

    tracing::info!(
        "Fetched {} models from OpenAI, filtered to {} completion models",
        total,
        models.len()
    );
    Ok(models)
}

async fn fetch_anthropic_models_impl(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    let client = rig::providers::anthropic::Anthropic::new(api_key);
    let list = client
        .list_models()
        .await
        .map_err(|e| format!("failed to list Anthropic models: {}", e))?;

    let models: Vec<ModelInfo> = list
        .data
        .into_iter()
        .map(|m| ModelRegistry::model_info("anthropic", &m.id))
        .collect();

    tracing::info!("Fetched {} models from Anthropic", models.len());
    Ok(models)
}

async fn fetch_gemini_models_impl(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    let client = rig::providers::gemini::Gemini::new(api_key);
    let list = client
        .list_models()
        .await
        .map_err(|e| format!("failed to list Gemini models: {}", e))?;

    let models: Vec<ModelInfo> = list
        .data
        .into_iter()
        .map(|m| ModelRegistry::model_info("gemini", &m.id))
        .collect();

    tracing::info!("Fetched {} models from Gemini", models.len());
    Ok(models)
}

async fn fetch_mistral_models_impl(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    let client = rig::providers::mistral::new(api_key);
    let list = client
        .list_models()
        .await
        .map_err(|e| format!("failed to list Mistral models: {}", e))?;

    let models: Vec<ModelInfo> = list
        .data
        .into_iter()
        .map(|m| ModelRegistry::model_info("mistral", &m.id))
        .collect();

    tracing::info!("Fetched {} models from Mistral", models.len());
    Ok(models)
}

#[derive(Deserialize)]
struct OpenCodeGoModelList {
    #[serde(default)]
    data: Vec<OpenCodeGoModel>,
}

#[derive(Deserialize)]
struct OpenCodeGoModel {
    id: String,
    #[serde(default)]
    capabilities: Option<OpenCodeGoCapabilities>,
}

#[derive(Deserialize)]
struct OpenCodeGoCapabilities {
    #[serde(default)]
    reasoning: Option<OpenCodeGoReasoning>,
}

#[derive(Deserialize)]
struct OpenCodeGoReasoning {
    #[serde(default)]
    supported: Option<bool>,
    #[serde(default)]
    effort: Option<Vec<String>>,
}

/// Read OpenCode Go's live capability metadata; Rig's `ModelInfo` drops it.
async fn fetch_opencode_go_models_impl(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    let fallback_models = ModelRegistry::hardcoded_models_for("opencode_go");

    let (list, from_live_endpoint) = match fetch_opencode_go_model_list(api_key).await {
        Ok(list) => (list, true),
        Err(error) => {
            tracing::warn!(
                "Failed to fetch OpenCode Go model capabilities ({error}); falling back to Rig model listing"
            );
            let client = crate::provider_client::opencode_go_openai_config(api_key)
                .connect(crate::provider_client::opencode_go_http(None));
            match client.list_models().await {
                Ok(list) => (
                    OpenCodeGoModelList {
                        data: list
                            .data
                            .into_iter()
                            .map(|model| OpenCodeGoModel {
                                id: model.id,
                                capabilities: None,
                            })
                            .collect(),
                    },
                    false,
                ),
                Err(error) => {
                    tracing::warn!(
                        "Failed to fetch OpenCode Go models: {}; using hardcoded base only",
                        error
                    );
                    return Ok(fallback_models);
                }
            }
        }
    };

    let live_efforts = list
        .data
        .iter()
        .filter_map(|model| {
            let reasoning = model.capabilities.as_ref()?.reasoning.as_ref()?;
            Some((
                model.id.clone(),
                if reasoning.supported == Some(false) {
                    Vec::new()
                } else {
                    reasoning.effort.clone().unwrap_or_default()
                },
            ))
        })
        .collect::<HashMap<_, _>>();
    if from_live_endpoint
        && list.data.iter().any(|model| {
            model
                .capabilities
                .as_ref()
                .is_some_and(|capabilities| capabilities.reasoning.is_some())
        })
    {
        ModelRegistry::replace_live_model_effort_provider("opencode_go", live_efforts);
    }

    let models: Vec<ModelInfo> = list
        .data
        .into_iter()
        .map(|model| ModelRegistry::model_info("opencode_go", &model.id))
        .collect();

    if models.is_empty() {
        tracing::warn!("OpenCode Go API returned no models; using hardcoded base only");
        return Ok(fallback_models);
    }

    tracing::info!("Fetched {} models from OpenCode Go", models.len());
    Ok(models)
}

async fn fetch_opencode_go_model_list(api_key: &str) -> Result<OpenCodeGoModelList, String> {
    let url = format!(
        "{}/models",
        crate::provider_client::OPENCODE_GO_OPENAI_BASE_URL
    );
    reqwest::Client::new()
        .get(url)
        .headers(crate::provider_client::opencode_go_headers(None))
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())
}

async fn fetch_github_copilot_models_impl() -> Result<Vec<ModelInfo>, String> {
    if !crate::keychain::has_github_copilot_oauth_session() {
        return Err("GitHub Copilot OAuth session not found".to_string());
    }

    let config = crate::provider_client::copilot_config().await?;
    crate::copilot_models::fetch_models(&config, &rig::rig_reqwest::shared()).await
}

const CHATGPT_CODEX_MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
// The backend omits models whose `minimal_client_version` is newer than the
// caller's and rejects requests without one. Gospel talks to the same Codex
// Responses contract through Rig rather than shipping as a Codex CLI release,
// so it claims a version above every current gate.
const CHATGPT_CODEX_CLIENT_VERSION: &str = "1.0.0";

async fn fetch_chatgpt_models_impl() -> Result<Vec<ModelInfo>, String> {
    if !crate::keychain::has_chatgpt_oauth_session() {
        return Err("ChatGPT OAuth session not found".to_string());
    }

    let fallback_models = ModelRegistry::hardcoded_models_for("chatgpt");

    let auth = match crate::provider_client::chatgpt_authenticator()
        .auth_context(&rig::rig_reqwest::shared())
        .await
    {
        Ok(auth) => auth,
        Err(e) => {
            let e = e.to_string();
            if crate::credential_failure::refresh_error_requires_reauth(&e) {
                return Err(format!(
                    "ChatGPT OAuth session expired or invalid; sign in again ({e})"
                ));
            }
            tracing::warn!("ChatGPT sign-in unavailable ({e}); using bundled model list");
            return Ok(fallback_models);
        }
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("failed to create HTTP client: {}", e))?;

    let mut request = client
        .get(CHATGPT_CODEX_MODELS_URL)
        .query(&[("client_version", CHATGPT_CODEX_CLIENT_VERSION)])
        .bearer_auth(auth.api_key.expose())
        .header("User-Agent", concat!("gospel/", env!("CARGO_PKG_VERSION")));
    if let Some(account) = auth.account_id.as_deref() {
        request = request.header("ChatGPT-Account-Id", account);
    }

    let body: serde_json::Value = match async {
        request
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await
    }
    .await
    {
        Ok(body) => body,
        Err(e) => {
            tracing::warn!("Failed to fetch ChatGPT models: {e}; using bundled model list");
            return Ok(fallback_models);
        }
    };

    let listed = crate::models::chatgpt_models_from_codex_listing(&body);
    if listed.is_empty() {
        tracing::warn!("ChatGPT listed no selectable models; using bundled model list");
        return Ok(fallback_models);
    }

    ModelRegistry::set_live_chatgpt_models(listed.iter().map(|model| model.slug.clone()));
    let efforts = listed
        .iter()
        .filter(|model| !model.efforts.is_empty())
        .map(|model| (model.slug.clone(), model.efforts.clone()))
        .collect::<HashMap<_, _>>();
    if !efforts.is_empty() {
        ModelRegistry::replace_live_model_effort_provider("chatgpt", efforts);
    }

    let models: Vec<ModelInfo> = listed
        .iter()
        .map(|model| ModelRegistry::model_info("chatgpt", &model.slug))
        .collect();
    tracing::info!("Resolved {} models for ChatGPT", models.len());
    Ok(models)
}

fn should_include_grok_model(model_id: &str) -> bool {
    let id = model_id.to_lowercase();
    if !id.starts_with("grok-") {
        return false;
    }
    let exclude = ["imagine", "image", "video", "tts", "whisper", "embedding"];
    !exclude.iter().any(|p| id.contains(p))
}

async fn fetch_grok_models_impl(api_key: Option<&str>) -> Result<Vec<ModelInfo>, String> {
    let access_token = match api_key.map(str::trim).filter(|key| !key.is_empty()) {
        Some(key) => key.to_string(),
        None => {
            if !crate::keychain::has_grok_oauth_session() {
                return Err("Grok OAuth session not found".to_string());
            }

            let auth_path = crate::keychain::grok_auth_file_path();
            match crate::grok_oauth::ensure_fresh_access_token(&auth_path).await {
                Ok(token) => token,
                Err(e) if crate::credential_failure::refresh_error_requires_reauth(&e) => {
                    return Err(format!(
                        "Grok OAuth session expired or invalid; sign in again ({e})"
                    ));
                }
                Err(e) => {
                    tracing::warn!("Grok token refresh failed ({e}); using stored access token");
                    crate::grok_oauth::access_token(&auth_path).map_err(|stored_err| {
                        format!(
                            "Grok OAuth session expired or invalid; sign in again ({e}; {stored_err})"
                        )
                    })?
                }
            }
        }
    };

    fetch_xai_developer_models("grok", &access_token).await
}

async fn fetch_xai_models_impl(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("xAI API key not configured".to_string());
    }
    fetch_xai_developer_models("xai", key).await
}

async fn fetch_xai_developer_models(
    provider_id: &str,
    access_token: &str,
) -> Result<Vec<ModelInfo>, String> {
    let fallback_models = ModelRegistry::hardcoded_models_for(provider_id);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("failed to create HTTP client: {}", e))?;

    let resp = client
        .get("https://api.x.ai/v1/models")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("User-Agent", concat!("gospel/", env!("CARGO_PKG_VERSION")))
        .send()
        .await;

    let body: serde_json::Value = match resp {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or(serde_json::Value::Null),
        Ok(r) => {
            let status = r.status();
            let body_text = r.text().await.unwrap_or_default();
            if let Some(kind) = crate::credential_failure::classify_credential_failure(&format!(
                "HTTP {status} {body_text}"
            )) {
                return Err(match kind {
                    crate::credential_failure::CredentialFailureKind::ReauthRequired => {
                        format!("HTTP {status} unauthorized while listing {provider_id} models")
                    }
                    crate::credential_failure::CredentialFailureKind::EntitlementBlocked => {
                        format!(
                            "HTTP {status} entitlement/tier-gate while listing {provider_id} models: {body_text}"
                        )
                    }
                });
            }
            tracing::warn!(
                "{provider_id} API returned status {}; using hardcoded base only",
                status
            );
            return Ok(fallback_models);
        }
        Err(e) => {
            tracing::warn!(
                "Failed to fetch {provider_id} models: {}; using hardcoded base only",
                e
            );
            return Ok(fallback_models);
        }
    };

    let mut models: Vec<ModelInfo> = Vec::new();
    if let Some(arr) = body["data"].as_array() {
        for m in arr {
            if let Some(id) = m["id"].as_str() {
                if should_include_grok_model(id)
                    && !models.iter().any(|existing| existing.model == id)
                {
                    models.push(ModelRegistry::model_info(provider_id, id));
                }
            }
        }
    }

    if models.is_empty() {
        tracing::warn!(
            "{provider_id} API returned no compatible models; using hardcoded base only"
        );
        return Ok(fallback_models);
    }

    tracing::info!(
        "Resolved {} compatible models for {provider_id}",
        models.len()
    );
    Ok(models)
}
