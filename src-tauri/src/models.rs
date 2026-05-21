use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock};

#[cfg(not(test))]
mod model_lists {
    pub use rig::providers::anthropic::completion::{
        CLAUDE_HAIKU_4_5, CLAUDE_OPUS_4_6, CLAUDE_OPUS_4_7, CLAUDE_SONNET_4_6,
    };
    pub use rig::providers::gemini::completion::{
        GEMINI_2_0_FLASH, GEMINI_2_0_FLASH_LITE, GEMINI_2_5_FLASH, GEMINI_2_5_FLASH_PREVIEW_04_17,
        GEMINI_2_5_PRO_EXP_03_25, GEMINI_2_5_PRO_PREVIEW_03_25, GEMINI_2_5_PRO_PREVIEW_05_06,
        GEMINI_2_5_PRO_PREVIEW_06_05, GEMINI_3_1_FLASH_LITE_PREVIEW, GEMINI_3_FLASH_PREVIEW,
    };
    pub use rig::providers::groq::{
        DEEPSEEK_R1_DISTILL_LLAMA_70B, GEMMA2_9B_IT, LLAMA_3_1_8B_INSTANT,
        LLAMA_3_2_11B_VISION_PREVIEW, LLAMA_3_2_1B_PREVIEW, LLAMA_3_2_3B_PREVIEW,
        LLAMA_3_2_70B_SPECDEC, LLAMA_3_2_70B_VERSATILE, LLAMA_3_2_90B_VISION_PREVIEW,
        LLAMA_3_70B_8192, LLAMA_3_8B_8192, LLAMA_GUARD_3_8B, MIXTRAL_8X7B_32768,
    };
    pub use rig::providers::mistral::{
        CODESTRAL, CODESTRAL_MAMBA, MINISTRAL_3B, MINISTRAL_8B, MISTRAL_LARGE, MISTRAL_NEMO,
        MISTRAL_SABA, MISTRAL_SMALL, PIXTRAL_LARGE, PIXTRAL_SMALL,
    };
    pub use rig::providers::openai::{
        GPT_4, GPT_4O, GPT_4O_2024_05_13, GPT_4O_2024_11_20, GPT_4O_MINI, GPT_4_0125_PREVIEW,
        GPT_4_0613, GPT_4_1, GPT_4_1106_PREVIEW, GPT_4_1106_VISION_PREVIEW, GPT_4_1_2025_04_14,
        GPT_4_1_MINI, GPT_4_1_NANO, GPT_4_32K, GPT_4_32K_0613, GPT_4_5_PREVIEW,
        GPT_4_5_PREVIEW_2025_02_27, GPT_4_TURBO, GPT_4_TURBO_2024_04_09, GPT_4_TURBO_PREVIEW,
        GPT_4_VISION_PREVIEW, GPT_5, GPT_5_1, GPT_5_2, GPT_5_5, GPT_5_MINI, GPT_5_NANO, O1,
        O1_2024_12_17, O1_MINI, O1_MINI_2024_09_12, O1_PREVIEW, O1_PREVIEW_2024_09_12, O1_PRO, O3,
        O3_MINI, O3_MINI_2025_01_31, O4_MINI, O4_MINI_2025_04_16,
    };

    pub const OPENAI_MODELS: &[&str] = &[
        GPT_5_5,
        GPT_5_2,
        GPT_5_1,
        GPT_5,
        GPT_5_MINI,
        GPT_5_NANO,
        GPT_4_5_PREVIEW,
        GPT_4_5_PREVIEW_2025_02_27,
        GPT_4O_2024_11_20,
        GPT_4O,
        GPT_4O_MINI,
        GPT_4O_2024_05_13,
        GPT_4_TURBO,
        GPT_4_TURBO_2024_04_09,
        GPT_4_TURBO_PREVIEW,
        GPT_4_0125_PREVIEW,
        GPT_4_1106_PREVIEW,
        GPT_4_VISION_PREVIEW,
        GPT_4_1106_VISION_PREVIEW,
        GPT_4,
        GPT_4_0613,
        GPT_4_32K,
        GPT_4_32K_0613,
        O4_MINI_2025_04_16,
        O4_MINI,
        O3,
        O3_MINI,
        O3_MINI_2025_01_31,
        O1_PRO,
        O1,
        O1_2024_12_17,
        O1_PREVIEW,
        O1_PREVIEW_2024_09_12,
        O1_MINI,
        O1_MINI_2024_09_12,
        GPT_4_1_MINI,
        GPT_4_1_NANO,
        GPT_4_1_2025_04_14,
        GPT_4_1,
    ];

    pub const ANTHROPIC_MODELS: &[&str] = &[
        CLAUDE_OPUS_4_6,
        CLAUDE_OPUS_4_7,
        CLAUDE_SONNET_4_6,
        CLAUDE_HAIKU_4_5,
    ];

    pub const GEMINI_MODELS: &[&str] = &[
        GEMINI_3_1_FLASH_LITE_PREVIEW,
        GEMINI_3_FLASH_PREVIEW,
        GEMINI_2_5_PRO_PREVIEW_06_05,
        GEMINI_2_5_PRO_PREVIEW_05_06,
        GEMINI_2_5_PRO_PREVIEW_03_25,
        GEMINI_2_5_FLASH_PREVIEW_04_17,
        GEMINI_2_5_PRO_EXP_03_25,
        GEMINI_2_5_FLASH,
        GEMINI_2_0_FLASH_LITE,
        GEMINI_2_0_FLASH,
    ];

    pub const GROQ_MODELS: &[&str] = &[
        DEEPSEEK_R1_DISTILL_LLAMA_70B,
        GEMMA2_9B_IT,
        LLAMA_3_1_8B_INSTANT,
        LLAMA_3_2_11B_VISION_PREVIEW,
        LLAMA_3_2_1B_PREVIEW,
        LLAMA_3_2_3B_PREVIEW,
        LLAMA_3_2_90B_VISION_PREVIEW,
        LLAMA_3_2_70B_SPECDEC,
        LLAMA_3_2_70B_VERSATILE,
        LLAMA_GUARD_3_8B,
        LLAMA_3_70B_8192,
        LLAMA_3_8B_8192,
        MIXTRAL_8X7B_32768,
    ];

    pub const MISTRAL_MODELS: &[&str] = &[
        CODESTRAL,
        MISTRAL_LARGE,
        PIXTRAL_LARGE,
        MISTRAL_SABA,
        MINISTRAL_3B,
        MINISTRAL_8B,
        MISTRAL_SMALL,
        PIXTRAL_SMALL,
        MISTRAL_NEMO,
        CODESTRAL_MAMBA,
    ];

    pub const CHATGPT_MODELS: &[&str] = &["gpt-5.5", "gpt-5.4", "gpt-5.4-mini", "gpt-5.3-codex"];

    pub const CHATGPT_DISCOVERABLE_MODELS: &[&str] = &[
        "gpt-5.5",
        "gpt-5.4",
        "gpt-5.4-mini",
        "gpt-5.4-pro",
        "gpt-5.3-codex",
        "gpt-5.3-codex-spark",
    ];
}

#[cfg(test)]
mod model_lists {
    pub const OPENAI_MODELS: &[&str] = &[
        "gpt-5.5",
        "gpt-5.2",
        "gpt-5.1",
        "gpt-5",
        "gpt-5-mini",
        "gpt-5-nano",
        "gpt-4.5-preview",
        "gpt-4.5-preview-2025-02-27",
        "gpt-4o-2024-11-20",
        "gpt-4o",
        "gpt-4o-mini",
        "gpt-4o-2024-05-13",
        "gpt-4-turbo",
        "gpt-4-turbo-2024-04-09",
        "gpt-4-turbo-preview",
        "gpt-4-0125-preview",
        "gpt-4-1106-preview",
        "gpt-4-vision-preview",
        "gpt-4-1106-vision-preview",
        "gpt-4",
        "gpt-4-0613",
        "gpt-4-32k",
        "gpt-4-32k-0613",
        "o4-mini-2025-04-16",
        "o4-mini",
        "o3",
        "o3-mini",
        "o3-mini-2025-01-31",
        "o1-pro",
        "o1",
        "o1-2024-12-17",
        "o1-preview",
        "o1-preview-2024-09-12",
        "o1-mini",
        "o1-mini-2024-09-12",
        "gpt-4.1-mini",
        "gpt-4.1-nano",
        "gpt-4.1-2025-04-14",
        "gpt-4.1",
    ];

    pub const ANTHROPIC_MODELS: &[&str] = &[
        "claude-opus-4-6",
        "claude-opus-4-7",
        "claude-sonnet-4-6",
        "claude-haiku-4-5",
    ];

    pub const GEMINI_MODELS: &[&str] = &[
        "gemini-3.1-flash-lite-preview",
        "gemini-3-flash-preview",
        "gemini-2.5-pro-preview-06-05",
        "gemini-2.5-pro-preview-05-06",
        "gemini-2.5-pro-preview-03-25",
        "gemini-2.5-flash-preview-04-17",
        "gemini-2.5-pro-exp-03-25",
        "gemini-2.5-flash",
        "gemini-2.0-flash-lite",
        "gemini-2.0-flash",
    ];

    pub const GROQ_MODELS: &[&str] = &[
        "deepseek-r1-distill-llama-70b",
        "gemma2-9b-it",
        "llama-3.1-8b-instant",
        "llama-3.2-11b-vision-preview",
        "llama-3.2-1b-preview",
        "llama-3.2-3b-preview",
        "llama-3.2-90b-vision-preview",
        "llama-3.2-70b-specdec",
        "llama-3.2-70b-versatile",
        "llama-guard-3-8b",
        "llama3-70b-8192",
        "llama3-8b-8192",
        "mixtral-8x7b-32768",
    ];

    pub const MISTRAL_MODELS: &[&str] = &[
        "codestral-latest",
        "mistral-large-latest",
        "pixtral-large-latest",
        "mistral-saba-latest",
        "ministral-3b-latest",
        "ministral-8b-latest",
        "mistral-small-latest",
        "pixtral-12b-2409",
        "open-mistral-nemo",
        "open-codestral-mamba",
    ];

    pub const CHATGPT_MODELS: &[&str] = &["gpt-5.5", "gpt-5.4", "gpt-5.4-mini", "gpt-5.3-codex"];

    pub const CHATGPT_DISCOVERABLE_MODELS: &[&str] = &[
        "gpt-5.5",
        "gpt-5.4",
        "gpt-5.4-mini",
        "gpt-5.4-pro",
        "gpt-5.3-codex",
        "gpt-5.3-codex-spark",
    ];
}

use model_lists::{
    ANTHROPIC_MODELS, CHATGPT_DISCOVERABLE_MODELS, CHATGPT_MODELS, GEMINI_MODELS, GROQ_MODELS,
    MISTRAL_MODELS, OPENAI_MODELS,
};

#[derive(Serialize, Clone, Debug)]
pub struct ModelInfo {
    pub model: String,
    pub provider: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct ModelInfoWithFreshness {
    pub models: Vec<ModelInfo>,
    pub is_fresh: bool,
    pub error_kind: Option<String>,
    pub error_detail: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CachedModelList {
    pub models: Vec<ModelInfo>,
    pub fetched_at: Instant,
    pub ttl: Duration,
}

impl CachedModelList {
    pub fn is_fresh(&self) -> bool {
        self.fetched_at.elapsed() < self.ttl
    }
}

pub static MODEL_CACHE: Lazy<Arc<RwLock<HashMap<String, CachedModelList>>>> =
    Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

pub static PENDING_REQUESTS: Lazy<Arc<RwLock<HashMap<String, Arc<Notify>>>>> =
    Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

pub const DEFAULT_CACHE_TTL_SECS: u64 = 300;

pub fn get_cache_ttl() -> Duration {
    match std::env::var("GOSPEL_MODEL_CACHE_TTL_SECONDS") {
        Ok(val) => val
            .parse::<u64>()
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(DEFAULT_CACHE_TTL_SECS)),
        Err(_) => Duration::from_secs(DEFAULT_CACHE_TTL_SECS),
    }
}

pub struct ModelRegistry;

impl ModelRegistry {
    pub fn models_for_provider(provider: &str) -> &'static [&'static str] {
        match provider {
            "openai" => OPENAI_MODELS,
            "chatgpt" => CHATGPT_MODELS,
            "anthropic" => ANTHROPIC_MODELS,
            "gemini" => GEMINI_MODELS,
            "groq" => GROQ_MODELS,
            "mistral" => MISTRAL_MODELS,
            _ => &[],
        }
    }

    pub fn is_chatgpt_subscription_model(model: &str) -> bool {
        CHATGPT_DISCOVERABLE_MODELS.contains(&model)
    }

    pub fn all_providers() -> &'static [&'static str] {
        &[
            "openai",
            "chatgpt",
            "anthropic",
            "gemini",
            "groq",
            "mistral",
        ]
    }

    pub fn get_available_models(has_key: impl Fn(&str) -> bool) -> Vec<ModelInfo> {
        Self::all_providers()
            .iter()
            .filter(|&&p| has_key(p))
            .flat_map(|&provider| {
                Self::models_for_provider(provider)
                    .iter()
                    .map(move |&model| ModelInfo {
                        model: model.to_string(),
                        provider: provider.to_string(),
                    })
            })
            .collect()
    }

    pub fn hardcoded_models_for(provider: &str) -> Vec<ModelInfo> {
        Self::models_for_provider(provider)
            .iter()
            .map(|&m| ModelInfo {
                model: m.to_string(),
                provider: provider.to_string(),
            })
            .collect()
    }

    pub async fn get_or_fetch<F, Fut>(
        cache_key: &str,
        provider: &str,
        force_refresh: bool,
        fetch_fn: F,
    ) -> ModelInfoWithFreshness
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<ModelInfo>, String>>,
    {
        if !force_refresh {
            let cache = MODEL_CACHE.read().await;
            if let Some(entry) = cache.get(cache_key) {
                if entry.is_fresh() {
                    return ModelInfoWithFreshness {
                        models: entry.models.clone(),
                        is_fresh: true,
                        error_kind: None,
                        error_detail: None,
                    };
                }
            }
        }

        // Register/check pending request atomically to avoid races and duplicate fetches
        let (is_waiter, notify) = {
            let mut pending = PENDING_REQUESTS.write().await;
            if let Some(notify) = pending.get(cache_key) {
                (true, notify.clone())
            } else {
                let notify = Arc::new(Notify::new());
                pending.insert(cache_key.to_string(), notify.clone());
                (false, notify)
            }
        };

        if is_waiter {
            notify.notified().await;
            let cache = MODEL_CACHE.read().await;
            if let Some(entry) = cache.get(cache_key) {
                return ModelInfoWithFreshness {
                    models: entry.models.clone(),
                    is_fresh: entry.is_fresh(),
                    error_kind: None,
                    error_detail: None,
                };
            }
            return ModelInfoWithFreshness {
                models: vec![],
                is_fresh: false,
                error_kind: Some("fetch_failed".to_string()),
                error_detail: Some("Model fetch did not complete".to_string()),
            };
        }

        let result = Self::fetch_and_cache_impl(cache_key, provider, fetch_fn).await;

        // Notify waiters and clean up
        {
            let mut pending = PENDING_REQUESTS.write().await;
            pending.remove(cache_key);
        }
        notify.notify_waiters();

        result
    }

    async fn fetch_and_cache_impl<F, Fut>(
        cache_key: &str,
        provider: &str,
        fetch_fn: F,
    ) -> ModelInfoWithFreshness
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<ModelInfo>, String>>,
    {
        match fetch_fn().await {
            Ok(models) => {
                let ttl = get_cache_ttl();
                let mut cache = MODEL_CACHE.write().await;
                cache.insert(
                    cache_key.to_string(),
                    CachedModelList {
                        models: models.clone(),
                        fetched_at: Instant::now(),
                        ttl,
                    },
                );
                ModelInfoWithFreshness {
                    models,
                    is_fresh: true,
                    error_kind: None,
                    error_detail: None,
                }
            }
            Err(e) => {
                let (error_kind, error_detail) = sanitize_model_fetch_error(&e);
                tracing::warn!("Failed to fetch models for {}: {}", provider, e);
                // Check if we have stale cached data
                let cache = MODEL_CACHE.read().await;
                if let Some(entry) = cache.get(cache_key) {
                    tracing::info!("Using stale cached models for {}", provider);
                    return ModelInfoWithFreshness {
                        models: entry.models.clone(),
                        is_fresh: false,
                        error_kind: Some(error_kind),
                        error_detail: Some(error_detail),
                    };
                }
                // No cache, return empty
                ModelInfoWithFreshness {
                    models: vec![],
                    is_fresh: false,
                    error_kind: Some(error_kind),
                    error_detail: Some(error_detail),
                }
            }
        }
    }

    pub async fn wait_for_fetch(cache_key: &str) -> Option<ModelInfoWithFreshness> {
        let notify = {
            let pending = PENDING_REQUESTS.read().await;
            pending.get(cache_key)?.clone()
        };
        notify.notified().await;

        // Fetch completed, check cache for result
        let cache = MODEL_CACHE.read().await;
        let entry = cache.get(cache_key)?;
        Some(ModelInfoWithFreshness {
            models: entry.models.clone(),
            is_fresh: entry.is_fresh(),
            error_kind: None,
            error_detail: None,
        })
    }
}

fn sanitize_model_fetch_error(error: &str) -> (String, String) {
    let lower = error.to_lowercase();
    let kind = if lower.contains("401")
        || lower.contains("403")
        || lower.contains("unauthorized")
        || lower.contains("access_token")
        || lower.contains("oauth")
    {
        "auth_failed"
    } else if lower.contains("timeout") || lower.contains("timed out") {
        "timeout"
    } else if lower.contains("429") || lower.contains("rate") {
        "rate_limited"
    } else if lower.contains("parse") || lower.contains("json") {
        "bad_response"
    } else {
        "fetch_failed"
    };
    let detail = match kind {
        "auth_failed" => "Provider credentials need attention.",
        "timeout" => "The provider did not respond in time.",
        "rate_limited" => "The provider rate limited model loading.",
        "bad_response" => "The provider returned an unreadable model response.",
        _ => "The provider could not load models.",
    };
    (kind.to_string(), detail.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn clear_model_state() {
        let mut cache = MODEL_CACHE.write().await;
        cache.clear();
        let mut pending = PENDING_REQUESTS.write().await;
        pending.clear();
    }

    #[test]
    fn test_cache_is_fresh_within_ttl() {
        let entry = CachedModelList {
            models: vec![],
            fetched_at: Instant::now(),
            ttl: Duration::from_secs(60),
        };
        assert!(entry.is_fresh());
    }

    #[test]
    fn test_cache_is_fresh_after_ttl() {
        let entry = CachedModelList {
            models: vec![],
            fetched_at: Instant::now() - Duration::from_secs(61),
            ttl: Duration::from_secs(60),
        };
        assert!(!entry.is_fresh());
    }

    #[test]
    fn test_cache_is_fresh_at_ttl_boundary() {
        let entry = CachedModelList {
            models: vec![],
            fetched_at: Instant::now() - Duration::from_secs(60),
            ttl: Duration::from_secs(60),
        };
        assert!(!entry.is_fresh());
    }

    #[test]
    fn test_get_cache_ttl_default() {
        let ttl = get_cache_ttl();
        assert_eq!(ttl, Duration::from_secs(300));
    }

    #[test]
    fn test_hardcoded_models_for_openai() {
        let models = ModelRegistry::hardcoded_models_for("openai");
        assert!(!models.is_empty());
        assert!(models.iter().all(|m| m.provider == "openai"));
    }

    #[test]
    fn test_chatgpt_subscription_models_include_current_codex_options() {
        assert!(ModelRegistry::is_chatgpt_subscription_model("gpt-5.5"));
        assert!(ModelRegistry::is_chatgpt_subscription_model("gpt-5.4"));
        assert!(ModelRegistry::is_chatgpt_subscription_model("gpt-5.4-mini"));
        assert!(ModelRegistry::is_chatgpt_subscription_model(
            "gpt-5.3-codex"
        ));
        assert!(ModelRegistry::is_chatgpt_subscription_model(
            "gpt-5.3-codex-spark"
        ));
    }

    #[test]
    fn test_chatgpt_subscription_models_reject_api_and_web_chat_models() {
        assert!(!ModelRegistry::is_chatgpt_subscription_model(
            "gpt-5.2-codex"
        ));
        assert!(!ModelRegistry::is_chatgpt_subscription_model(
            "gpt-5.1-codex"
        ));
        assert!(!ModelRegistry::is_chatgpt_subscription_model("gpt-4o"));
        assert!(!ModelRegistry::is_chatgpt_subscription_model("chat-latest"));
        assert!(!ModelRegistry::is_chatgpt_subscription_model(
            "text-embedding-3-large"
        ));
    }

    #[test]
    fn test_chatgpt_hardcoded_fallback_omits_tier_specific_models() {
        let models = ModelRegistry::models_for_provider("chatgpt");

        assert!(models.contains(&"gpt-5.5"));
        assert!(models.contains(&"gpt-5.4"));
        assert!(models.contains(&"gpt-5.4-mini"));
        assert!(models.contains(&"gpt-5.3-codex"));
        assert!(!models.contains(&"gpt-5.4-pro"));
        assert!(!models.contains(&"gpt-5.3-codex-spark"));
    }

    #[test]
    fn test_get_available_models_still_returns_hardcoded() {
        let has_key = |p: &str| -> bool { p == "openai" };
        let models = ModelRegistry::get_available_models(has_key);
        assert!(!models.is_empty());
        assert!(models.iter().all(|m| m.provider == "openai"));
    }

    #[test]
    fn test_cache_rejects_expired_entry() {
        let old = Instant::now() - Duration::from_secs(600);
        let entry = CachedModelList {
            models: vec![],
            fetched_at: old,
            ttl: Duration::from_secs(60),
        };
        assert!(!entry.is_fresh());
    }

    #[test]
    fn test_cache_accepts_recent_entry() {
        let entry = CachedModelList {
            models: vec![],
            fetched_at: Instant::now(),
            ttl: Duration::from_secs(601),
        };
        assert!(entry.is_fresh());
    }

    #[tokio::test]
    async fn test_get_or_fetch_waits_for_inflight_request() {
        clear_model_state().await;

        let cache_key = "inflight-model-cache-key";
        let fetch_count = Arc::new(AtomicUsize::new(0));
        let waiter_count = 6;
        let barrier = Arc::new(tokio::sync::Barrier::new(waiter_count + 1));
        let mut tasks = Vec::new();

        for _ in 0..waiter_count {
            let cache_key = cache_key.to_string();
            let fetch_count = fetch_count.clone();
            let barrier = barrier.clone();
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                ModelRegistry::get_or_fetch(cache_key.as_str(), "openai", false, move || {
                    let fetch_count = fetch_count.clone();
                    async move {
                        fetch_count.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        Ok(vec![ModelInfo {
                            model: "gpt-4o".to_string(),
                            provider: "openai".to_string(),
                        }])
                    }
                })
                .await
            }));
        }

        barrier.wait().await;

        for task in tasks {
            let result = task.await.expect("task panicked");
            assert!(result.is_fresh);
            assert_eq!(result.models.len(), 1);
            assert_eq!(result.models[0].model, "gpt-4o");
        }

        assert_eq!(fetch_count.load(Ordering::SeqCst), 1);
    }
}
