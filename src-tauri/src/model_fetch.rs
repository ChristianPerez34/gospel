use crate::models::{ModelInfo, ModelRegistry};
use rig::client::ModelListingClient;

fn cache_scope_for_provider(provider: &str, api_key: &str) -> String {
    match provider {
        "openai" | "anthropic" | "gemini" | "mistral" => api_key.to_string(),
        _ => "shared".to_string(),
    }
}

fn should_include_completion_model(model_id: &str) -> bool {
    let id = model_id.to_lowercase();
    let exclude = [
        "embedding", "tts", "dall-e", "whisper", "moderation",
        "realtime",
    ];
    if exclude.iter().any(|p| id.contains(p)) {
        return false;
    }
    let include = ["gpt-", "o1-", "o3-", "o4-", "o5-", "codex"];
    include.iter().any(|p| id.contains(p))
}

pub async fn fetch_models_for_provider(provider: &str, api_key: &str) -> Vec<ModelInfo> {
    let cache_scope = cache_scope_for_provider(provider, api_key);
    let cache_key = format!("{}:{}", provider, cache_scope);

    ModelRegistry::get_or_fetch(&cache_key, provider, || async {
        match provider {
            "openai" => fetch_openai_models_impl(api_key).await,
            "chatgpt" => fetch_chatgpt_models_impl().await,
            "anthropic" => fetch_anthropic_models_impl(api_key).await,
            "gemini" => fetch_gemini_models_impl(api_key).await,
            "mistral" => fetch_mistral_models_impl(api_key).await,
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

    let client = reqwest::Client::new();
    let resp = client
        .get("https://chatgpt.com/backend-api/models")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("User-Agent", "gospel/0.1.0")
        .send()
        .await
        .map_err(|e| format!("failed to fetch ChatGPT models: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("ChatGPT API returned status {}", resp.status()));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse ChatGPT response: {}", e))?;

    let models: Vec<ModelInfo> = body["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let slug = m["slug"].as_str().or_else(|| m["id"].as_str())?;
                    Some(ModelInfo {
                        model: slug.to_string(),
                        provider: "chatgpt".to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    tracing::info!("Fetched {} models from ChatGPT", models.len());
    Ok(models)
}
