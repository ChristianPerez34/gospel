use std::time::Duration;

use crate::models::{ModelInfo, ModelInfoWithFreshness, ModelRegistry};
use rig::client::ModelListingClient;

fn cache_scope_for_provider(provider: &str, api_key: Option<&str>) -> String {
    match provider {
        "openai" | "anthropic" | "gemini" | "mistral" => api_key.unwrap_or("").to_string(),
        _ => "shared".to_string(),
    }
}

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
    provider: &str,
    api_key: Option<&str>,
    force_refresh: bool,
) -> ModelInfoWithFreshness {
    let cache_scope = cache_scope_for_provider(provider, api_key);
    let cache_key = format!("{}:{}", provider, cache_scope);

    ModelRegistry::get_or_fetch(&cache_key, provider, force_refresh, || async {
        match provider {
            "openai" => fetch_openai_models_impl(api_key.unwrap_or("")).await,
            "chatgpt" => fetch_chatgpt_models_impl().await,
            "anthropic" => fetch_anthropic_models_impl(api_key.unwrap_or("")).await,
            "gemini" => fetch_gemini_models_impl(api_key.unwrap_or("")).await,
            "mistral" => fetch_mistral_models_impl(api_key.unwrap_or("")).await,
            _ => Ok(ModelRegistry::hardcoded_models_for(provider)),
        }
    })
    .await
}

async fn fetch_openai_models_impl(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    let client = rig::providers::openai::Client::new(api_key.to_string())
        .map_err(|e| format!("failed to create OpenAI client: {}", e))?;
    let list = client
        .list_models()
        .await
        .map_err(|e| format!("failed to list OpenAI models: {}", e))?;

    let total = list.data.len();
    let models: Vec<ModelInfo> = list
        .data
        .into_iter()
        .filter(|m| should_include_completion_model(&m.id))
        .map(|m| ModelInfo {
            model: m.id,
            provider: "openai".to_string(),
        })
        .collect();

    tracing::info!(
        "Fetched {} models from OpenAI, filtered to {} completion models",
        total,
        models.len()
    );
    Ok(models)
}

async fn fetch_anthropic_models_impl(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    let client = rig::providers::anthropic::Client::new(api_key.to_string())
        .map_err(|e| format!("failed to create Anthropic client: {}", e))?;
    let list = client
        .list_models()
        .await
        .map_err(|e| format!("failed to list Anthropic models: {}", e))?;

    let models: Vec<ModelInfo> = list
        .data
        .into_iter()
        .map(|m| ModelInfo {
            model: m.id,
            provider: "anthropic".to_string(),
        })
        .collect();

    tracing::info!("Fetched {} models from Anthropic", models.len());
    Ok(models)
}

async fn fetch_gemini_models_impl(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    let client = rig::providers::gemini::Client::new(api_key.to_string())
        .map_err(|e| format!("failed to create Gemini client: {}", e))?;
    let list = client
        .list_models()
        .await
        .map_err(|e| format!("failed to list Gemini models: {}", e))?;

    let models: Vec<ModelInfo> = list
        .data
        .into_iter()
        .map(|m| ModelInfo {
            model: m.id,
            provider: "gemini".to_string(),
        })
        .collect();

    tracing::info!("Fetched {} models from Gemini", models.len());
    Ok(models)
}

async fn fetch_mistral_models_impl(api_key: &str) -> Result<Vec<ModelInfo>, String> {
    let client = rig::providers::mistral::Client::new(api_key.to_string())
        .map_err(|e| format!("failed to create Mistral client: {}", e))?;
    let list = client
        .list_models()
        .await
        .map_err(|e| format!("failed to list Mistral models: {}", e))?;

    let models: Vec<ModelInfo> = list
        .data
        .into_iter()
        .map(|m| ModelInfo {
            model: m.id,
            provider: "mistral".to_string(),
        })
        .collect();

    tracing::info!("Fetched {} models from Mistral", models.len());
    Ok(models)
}

async fn fetch_chatgpt_models_impl() -> Result<Vec<ModelInfo>, String> {
    let auth_path = crate::keychain::chatgpt_auth_file_path();
    if !auth_path.exists() {
        return Err("ChatGPT OAuth session not found".to_string());
    }

    let auth_data: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&auth_path)
            .map_err(|e| format!("failed to read auth file: {}", e))?,
    )
    .map_err(|e| format!("failed to parse auth file: {}", e))?;

    let access_token = auth_data["access_token"]
        .as_str()
        .ok_or_else(|| "access_token not found in auth file".to_string())?;

    // There is no public ChatGPT subscription model-list API equivalent to
    // OpenAI's API-key /models endpoint. Use the private ChatGPT model
    // endpoint when available so account-tier-specific models only appear
    // when the signed-in account can see them.
    let fallback_models = ModelRegistry::hardcoded_models_for("chatgpt");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("failed to create HTTP client: {}", e))?;

    let resp = client
        .get("https://chatgpt.com/backend-api/models")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("User-Agent", "gospel/0.1.0")
        .send()
        .await;

    let body: serde_json::Value = match resp {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or(serde_json::Value::Null),
        Ok(r) => {
            tracing::warn!(
                "ChatGPT API returned status {}; using hardcoded base only",
                r.status()
            );
            return Ok(fallback_models);
        }
        Err(e) => {
            tracing::warn!(
                "Failed to fetch ChatGPT models: {}; using hardcoded base only",
                e
            );
            return Ok(fallback_models);
        }
    };

    let mut models: Vec<ModelInfo> = Vec::new();
    if let Some(arr) = body["models"].as_array() {
        for m in arr {
            if let Some(slug) = m["slug"].as_str().or_else(|| m["id"].as_str()) {
                if !models.iter().any(|existing| existing.model == slug)
                    && ModelRegistry::is_chatgpt_subscription_model(slug)
                {
                    models.push(ModelInfo {
                        model: slug.to_string(),
                        provider: "chatgpt".to_string(),
                    });
                }
            }
        }
    }

    if models.is_empty() {
        tracing::warn!("ChatGPT API returned no compatible models; using hardcoded base only");
        return Ok(fallback_models);
    }

    tracing::info!("Resolved {} compatible models for ChatGPT", models.len());
    Ok(models)
}
