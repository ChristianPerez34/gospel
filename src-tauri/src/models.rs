use rig::catalog::{Catalog, ModelSpec};
use rig::completion::Effort;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock as StdRwLock};
use std::time::{Duration, Instant};
#[cfg(not(test))]
use tokio::sync::Mutex;
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
    pub use rig::providers::groq::LLAMA_3_1_8B_INSTANT;
    pub use rig::providers::mistral::{
        CODESTRAL, MINISTRAL_3B, MINISTRAL_8B, MISTRAL_LARGE, MISTRAL_SMALL,
    };

    // Model ids rig dropped from its constant lists; kept so the hardcoded
    // fallback lists stay stable.
    pub const DEEPSEEK_R1_DISTILL_LLAMA_70B: &str = "deepseek-r1-distill-llama-70b";
    pub const GEMMA2_9B_IT: &str = "gemma2-9b-it";
    pub const LLAMA_3_2_11B_VISION_PREVIEW: &str = "llama-3.2-11b-vision-preview";
    pub const LLAMA_3_2_1B_PREVIEW: &str = "llama-3.2-1b-preview";
    pub const LLAMA_3_2_3B_PREVIEW: &str = "llama-3.2-3b-preview";
    pub const LLAMA_3_2_90B_VISION_PREVIEW: &str = "llama-3.2-90b-vision-preview";
    pub const LLAMA_3_2_70B_SPECDEC: &str = "llama-3.2-70b-specdec";
    pub const LLAMA_3_2_70B_VERSATILE: &str = "llama-3.2-70b-versatile";
    pub const LLAMA_GUARD_3_8B: &str = "llama-guard-3-8b";
    pub const LLAMA_3_70B_8192: &str = "llama3-70b-8192";
    pub const LLAMA_3_8B_8192: &str = "llama3-8b-8192";
    pub const MIXTRAL_8X7B_32768: &str = "mixtral-8x7b-32768";
    pub const PIXTRAL_LARGE: &str = "pixtral-large-latest";
    pub const MISTRAL_SABA: &str = "mistral-saba-latest";
    pub const PIXTRAL_SMALL: &str = "pixtral-12b-2409";
    pub const MISTRAL_NEMO: &str = "open-mistral-nemo";
    pub const CODESTRAL_MAMBA: &str = "open-codestral-mamba";
    pub use rig::providers::openai::{
        GPT_4, GPT_4O, GPT_4O_2024_05_13, GPT_4O_2024_11_20, GPT_4O_MINI, GPT_4_0125_PREVIEW,
        GPT_4_0613, GPT_4_1, GPT_4_1106_PREVIEW, GPT_4_1106_VISION_PREVIEW, GPT_4_1_2025_04_14,
        GPT_4_1_MINI, GPT_4_1_NANO, GPT_4_32K, GPT_4_32K_0613, GPT_4_5_PREVIEW,
        GPT_4_5_PREVIEW_2025_02_27, GPT_4_TURBO, GPT_4_TURBO_2024_04_09, GPT_4_TURBO_PREVIEW,
        GPT_4_VISION_PREVIEW, GPT_5, GPT_5_1, GPT_5_2, GPT_5_5, GPT_5_MINI, GPT_5_NANO, O1,
        O1_2024_12_17, O1_MINI, O1_MINI_2024_09_12, O1_PREVIEW, O1_PREVIEW_2024_09_12, O1_PRO, O3,
        O3_MINI, O3_MINI_2025_01_31, O4_MINI, O4_MINI_2025_04_16,
    };
    pub use rig::providers::xai::{
        GROK_2_1212, GROK_2_VISION_1212, GROK_3, GROK_3_FAST, GROK_3_MINI, GROK_3_MINI_FAST, GROK_4,
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

    pub const CHATGPT_MODELS: &[&str] = &[
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
        "gpt-5.5",
        "gpt-5.4",
        "gpt-5.4-mini",
        "gpt-5.3-codex-spark",
    ];

    pub const CHATGPT_DISCOVERABLE_MODELS: &[&str] = &[
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
        "gpt-5.5",
        "gpt-5.4",
        "gpt-5.4-mini",
        "gpt-5.4-pro",
        "gpt-5.3-codex",
        "gpt-5.3-codex-spark",
    ];

    pub const GITHUB_COPILOT_MODELS: &[&str] = &[];

    pub const GROK_MODELS: &[&str] = &[
        GROK_4,
        GROK_3,
        GROK_3_FAST,
        GROK_3_MINI,
        GROK_3_MINI_FAST,
        GROK_2_1212,
        GROK_2_VISION_1212,
    ];

    // OpenCode Go catalog (https://opencode.ai/docs/go/). Live list comes from
    // https://opencode.ai/zen/go/v1/models; this is the documented fallback.
    pub const OPENCODE_GO_MODELS: &[&str] = &[
        "grok-4.7",
        "grok-4.6",
        "glm-5.3-flash",
        "glm-5.3",
        "glm-5.2",
        "gpt-6-luna",
        "gpt-5.6-luna",
        "kimi-k3",
        "kimi-k2.7-code",
        "kimi-k2.6",
        "longcat-2.0",
        "longcat-2.5-preview-free",
        "mimo-v2.6-flash",
        "mimo-v2.6-pro",
        "mimo-v2.5",
        "mimo-v2.5-pro",
        "minimax-m3",
        "minimax-m2.7",
        "muse-spark-1.3-contributor",
        "muse-spark-1.2-contributor",
        "qwen3.8-max",
        "qwen3.8-flash",
        "qwen3.7-plus",
        "deepseek-v4.1-flash",
        "deepseek-v4-pro",
        "deepseek-v4-flash",
        "deepseek-v4-flash-vision-exp",
        "hy4-preview",
        "hy3",
        "space-bunny-free",
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

    pub const CHATGPT_MODELS: &[&str] = &[
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
        "gpt-5.5",
        "gpt-5.4",
        "gpt-5.4-mini",
        "gpt-5.3-codex-spark",
    ];

    pub const CHATGPT_DISCOVERABLE_MODELS: &[&str] = &[
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
        "gpt-5.5",
        "gpt-5.4",
        "gpt-5.4-mini",
        "gpt-5.4-pro",
        "gpt-5.3-codex",
        "gpt-5.3-codex-spark",
    ];

    pub const GITHUB_COPILOT_MODELS: &[&str] = &[];

    pub const GROK_MODELS: &[&str] = &[
        "grok-4-0709",
        "grok-3",
        "grok-3-fast",
        "grok-3-mini",
        "grok-3-mini-fast",
        "grok-2-1212",
        "grok-2-vision-1212",
    ];

    pub const OPENCODE_GO_MODELS: &[&str] = &[
        "grok-4.7",
        "grok-4.6",
        "glm-5.3-flash",
        "glm-5.3",
        "glm-5.2",
        "gpt-6-luna",
        "gpt-5.6-luna",
        "kimi-k3",
        "kimi-k2.7-code",
        "kimi-k2.6",
        "longcat-2.0",
        "longcat-2.5-preview-free",
        "mimo-v2.6-flash",
        "mimo-v2.6-pro",
        "mimo-v2.5",
        "mimo-v2.5-pro",
        "minimax-m3",
        "minimax-m2.7",
        "muse-spark-1.3-contributor",
        "muse-spark-1.2-contributor",
        "qwen3.8-max",
        "qwen3.8-flash",
        "qwen3.7-plus",
        "deepseek-v4.1-flash",
        "deepseek-v4-pro",
        "deepseek-v4-flash",
        "deepseek-v4-flash-vision-exp",
        "hy4-preview",
        "hy3",
        "space-bunny-free",
    ];
}

use model_lists::{
    ANTHROPIC_MODELS, CHATGPT_DISCOVERABLE_MODELS, CHATGPT_MODELS, GEMINI_MODELS,
    GITHUB_COPILOT_MODELS, GROK_MODELS, GROQ_MODELS, MISTRAL_MODELS, OPENAI_MODELS,
    OPENCODE_GO_MODELS,
};

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ModelVariant {
    pub id: String,
    pub name: String,
    pub description: String,
    pub deprecated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelVariantDefinition {
    id: String,
    name: String,
    description: String,
    deprecated: bool,
    effort: Effort,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelVariantWarning {
    pub kind: String,
    pub variant: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedModelVariant {
    pub variant: Option<String>,
    pub effort: Option<Effort>,
    pub additional_params: Option<serde_json::Value>,
    pub warning: Option<ModelVariantWarning>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ModelInfo {
    pub model: String,
    pub provider: String,
    pub variants: Vec<ModelVariant>,
    #[serde(skip)]
    pub(crate) copilot_supported_endpoints: Option<Vec<String>>,
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

pub static MODEL_CACHE: LazyLock<Arc<RwLock<HashMap<String, CachedModelList>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(HashMap::new())));

type PendingMap = Arc<RwLock<HashMap<String, Arc<Notify>>>>;
pub static PENDING_REQUESTS: LazyLock<PendingMap> =
    LazyLock::new(|| Arc::new(RwLock::new(HashMap::new())));

pub const DEFAULT_CACHE_TTL_SECS: u64 = 300;
#[cfg(not(test))]
const LIVE_MODEL_CATALOG_URL: &str = "https://models.dev/api.json?type=chat";
#[cfg(not(test))]
const LIVE_MODEL_CATALOG_TTL: Duration = Duration::from_secs(DEFAULT_CACHE_TTL_SECS);
// ponytail: coalesce forced refreshes for 10s so parallel provider fetches share one request.
#[cfg(not(test))]
const LIVE_MODEL_CATALOG_FORCE_COALESCE: Duration = Duration::from_secs(10);
// ponytail: raise this cap if models.dev's chat catalog grows beyond 16 MiB.
#[cfg(not(test))]
const MAX_LIVE_MODEL_CATALOG_BYTES: usize = 16 * 1024 * 1024;

type LiveModelEffortCatalog = HashMap<String, HashMap<String, Vec<Effort>>>;

#[derive(Default)]
struct CachedLiveModelEfforts {
    by_provider: LiveModelEffortCatalog,
    provider_overrides: LiveModelEffortCatalog,
    #[cfg(not(test))]
    etag: Option<String>,
}

#[derive(Deserialize)]
struct ModelsDevProviderCatalog {
    #[serde(default)]
    models: HashMap<String, ModelsDevModel>,
}

#[derive(Deserialize)]
struct ModelsDevModel {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    reasoning: Option<bool>,
    #[serde(default)]
    reasoning_options: Option<serde_json::Value>,
}

static LIVE_MODEL_EFFORTS: LazyLock<StdRwLock<CachedLiveModelEfforts>> =
    LazyLock::new(|| StdRwLock::new(CachedLiveModelEfforts::default()));
#[cfg(not(test))]
static LIVE_MODEL_CATALOG_REFRESHED_AT: LazyLock<Mutex<Option<Instant>>> =
    LazyLock::new(|| Mutex::new(None));

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
    #[cfg(not(test))]
    pub(crate) async fn refresh_live_model_catalog(force_refresh: bool) {
        let mut refreshed_at = LIVE_MODEL_CATALOG_REFRESHED_AT.lock().await;
        if let Some(last_refresh) = *refreshed_at {
            let age = last_refresh.elapsed();
            if age < LIVE_MODEL_CATALOG_FORCE_COALESCE
                || (!force_refresh && age < LIVE_MODEL_CATALOG_TTL)
            {
                return;
            }
        }

        let etag = LIVE_MODEL_EFFORTS
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .etag
            .clone();
        match fetch_live_model_efforts(etag.as_deref()).await {
            Ok(Some(catalog)) if !catalog.by_provider.is_empty() => {
                let provider_count = catalog.by_provider.len();
                let mut cached = LIVE_MODEL_EFFORTS
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                cached.by_provider = catalog.by_provider;
                cached.etag = catalog.etag;
                tracing::info!(
                    providers = provider_count,
                    "Refreshed live model reasoning metadata from models.dev"
                );
            }
            Ok(Some(_)) => {
                tracing::warn!("models.dev returned no reasoning metadata; retaining the prior catalog");
            }
            Ok(None) => tracing::debug!("models.dev reasoning metadata has not changed"),
            Err(error) => tracing::warn!(
                "Failed to refresh live model reasoning metadata; using cached or bundled data: {error}"
            ),
        }
        *refreshed_at = Some(Instant::now());
    }

    #[cfg(not(test))]
    pub(crate) fn replace_live_model_effort_provider(
        provider: &str,
        model_efforts: HashMap<String, Vec<String>>,
    ) {
        let model_efforts = model_efforts
            .into_iter()
            .map(|(model, values)| {
                (
                    model,
                    values
                        .iter()
                        .filter_map(|value| parse_live_effort(value))
                        .collect(),
                )
            })
            .collect();
        LIVE_MODEL_EFFORTS
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .provider_overrides
            .insert(models_dev_provider_id(provider).to_string(), model_efforts);
    }

    pub fn model_info(provider: &str, model: &str) -> ModelInfo {
        ModelInfo {
            model: model.to_string(),
            provider: provider.to_string(),
            variants: Self::variants_for_model(provider, model),
            copilot_supported_endpoints: None,
        }
    }

    pub fn variants_for_model(provider: &str, model: &str) -> Vec<ModelVariant> {
        Self::variant_definitions(provider, model)
            .into_iter()
            .filter(|variant| !variant.deprecated)
            .map(|variant| ModelVariant {
                id: variant.id,
                name: variant.name,
                description: variant.description,
                deprecated: variant.deprecated,
            })
            .collect()
    }

    pub fn resolve_model_variant(
        provider: &str,
        model: &str,
        variant: Option<&str>,
    ) -> ResolvedModelVariant {
        let default_additional_params = Self::reasoning_stream_params(provider, model);
        let Some(variant) = variant.map(str::trim).filter(|value| !value.is_empty()) else {
            return ResolvedModelVariant {
                variant: None,
                effort: None,
                additional_params: default_additional_params,
                warning: None,
            };
        };

        if let Some(definition) = Self::variant_definition(provider, model, variant) {
            let warning = if definition.deprecated {
                Some(ModelVariantWarning {
                    kind: "deprecated".to_string(),
                    variant: variant.to_string(),
                    message: format!(
                        "Model variant '{}' is deprecated. It will run for this session, but it is hidden from new selections.",
                        definition.name
                    ),
                })
            } else {
                None
            };
            return ResolvedModelVariant {
                variant: Some(definition.id),
                effort: Some(definition.effort),
                additional_params: default_additional_params,
                warning,
            };
        }

        ResolvedModelVariant {
            variant: None,
            effort: None,
            additional_params: default_additional_params,
            warning: Some(ModelVariantWarning {
                kind: "missing".to_string(),
                variant: variant.to_string(),
                message: format!(
                    "Model variant '{}' is no longer available for {} {}; using Default for this turn.",
                    variant, provider, model
                ),
            }),
        }
    }

    fn reasoning_stream_params(provider: &str, model: &str) -> Option<serde_json::Value> {
        let supports_summary = match provider {
            "openai" => matches!(model, "gpt-5.1" | "gpt-5.2"),
            "chatgpt" => model != "gpt-5.6-luna" && Self::is_chatgpt_subscription_model(model),
            "opencode_go" => {
                matches!(
                    crate::provider_client::opencode_go_protocol(model),
                    crate::provider_client::OpencodeGoProtocol::OpenAiResponses
                ) && Self::reasoning_effort_levels(provider, model)
                    .is_some_and(|levels| !levels.is_empty())
            }
            _ => false,
        };
        supports_summary.then(|| {
            json!({
                "reasoning": {
                    "summary": "auto"
                }
            })
        })
    }

    fn variant_definition(
        provider: &str,
        model: &str,
        variant: &str,
    ) -> Option<ModelVariantDefinition> {
        Self::variant_definitions(provider, model)
            .into_iter()
            .find(|definition| definition.id == variant)
    }

    fn variant_definitions(provider: &str, model: &str) -> Vec<ModelVariantDefinition> {
        let Some(levels) = Self::reasoning_effort_levels(provider, model) else {
            return Vec::new();
        };

        levels
            .into_iter()
            .map(variant_definition_for_effort)
            .collect()
    }

    fn reasoning_effort_levels(provider: &str, model: &str) -> Option<Vec<Effort>> {
        if provider == "chatgpt"
            && (model == "gpt-5.6-luna" || !Self::is_chatgpt_subscription_model(model))
        {
            return None;
        }

        live_model_effort_levels(provider, model).or_else(|| {
            catalog_model_spec(provider, model)
                .filter(|spec| spec.reasoning.supported)
                .map(|spec| spec.reasoning.levels.clone())
        })
    }

    pub fn models_for_provider(provider: &str) -> &'static [&'static str] {
        match provider {
            "openai" => OPENAI_MODELS,
            "chatgpt" => CHATGPT_MODELS,
            "github_copilot" => GITHUB_COPILOT_MODELS,
            "grok" => GROK_MODELS,
            "xai" => GROK_MODELS,
            "anthropic" => ANTHROPIC_MODELS,
            "gemini" => GEMINI_MODELS,
            "groq" => GROQ_MODELS,
            "mistral" => MISTRAL_MODELS,
            "opencode_go" => OPENCODE_GO_MODELS,
            _ => &[],
        }
    }

    pub fn is_chatgpt_subscription_model(model: &str) -> bool {
        CHATGPT_DISCOVERABLE_MODELS.contains(&model)
    }

    pub(crate) async fn copilot_supported_endpoints(model: &str) -> Option<Vec<String>> {
        let cache = MODEL_CACHE.read().await;
        cache.values().find_map(|entry| {
            entry
                .models
                .iter()
                .find(|info| info.provider == "github_copilot" && info.model == model)
                .and_then(|info| info.copilot_supported_endpoints.clone())
        })
    }

    #[allow(dead_code)]
    pub fn is_oauth_provider(provider: &str) -> bool {
        crate::providers::is_oauth_provider(provider)
    }

    pub fn provider_display_name(provider: &str) -> &'static str {
        crate::providers::provider_display_name(provider)
    }

    pub fn provider_auth_type(provider: &str) -> &'static str {
        crate::providers::provider_auth_type(provider)
    }

    pub fn all_providers() -> &'static [&'static str] {
        static PROVIDERS: LazyLock<Vec<&'static str>> =
            LazyLock::new(crate::providers::provider_ids);
        PROVIDERS.as_slice()
    }

    #[allow(dead_code)]
    pub fn get_available_models(has_key: impl Fn(&str) -> bool) -> Vec<ModelInfo> {
        Self::all_providers()
            .iter()
            .filter(|&&p| has_key(p))
            .flat_map(|&provider| {
                Self::models_for_provider(provider)
                    .iter()
                    .map(move |&model| Self::model_info(provider, model))
            })
            .collect()
    }

    pub fn hardcoded_models_for(provider: &str) -> Vec<ModelInfo> {
        Self::models_for_provider(provider)
            .iter()
            .map(|&m| Self::model_info(provider, m))
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

        // Register/check pending request atomically to avoid races and duplicate fetches.
        // The waiter's Notified future is created and enabled while still holding the
        // write lock, so registration happens-before the guard releases. Without this,
        // a leader that completes between the map read and the waiter's first poll
        // drops the wakeup (notify_waiters stores no permit for latecomers) and the
        // waiter hangs forever.
        let notify;
        let (must_rejoin, wait) = {
            let mut pending = PENDING_REQUESTS.write().await;
            if let Some(n) = pending.get(cache_key) {
                notify = n.clone();
                let mut w = Box::pin(notify.notified());
                w.as_mut().enable();
                (force_refresh, Some(w))
            } else {
                notify = Arc::new(Notify::new());
                pending.insert(cache_key.to_string(), notify.clone());
                (false, None)
            }
        };

        if let Some(wait) = wait {
            wait.await;
            if must_rejoin {
                return Self::fetch_and_cache_impl(cache_key, provider, fetch_fn).await;
            }
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
}

fn parse_live_model_effort_catalog(
    json: &str,
) -> Result<LiveModelEffortCatalog, serde_json::Error> {
    let providers: HashMap<String, ModelsDevProviderCatalog> = serde_json::from_str(json)?;
    let mut catalog = HashMap::new();

    for (provider_id, provider) in providers {
        let mut models = HashMap::new();
        for (model_id, model) in provider.models {
            let levels = model
                .reasoning_options
                .as_ref()
                .and_then(parse_live_effort_options)
                .or_else(|| {
                    model
                        .reasoning
                        .filter(|supports_reasoning| !supports_reasoning)
                        .map(|_| Vec::new())
                });
            let Some(levels) = levels else {
                continue;
            };

            models.insert(model_id.clone(), levels.clone());
            if let Some(id) = model.id.filter(|id| id != &model_id) {
                models.insert(id, levels);
            }
        }
        if !models.is_empty() {
            catalog.insert(provider_id, models);
        }
    }

    Ok(catalog)
}

fn parse_live_effort_options(options: &serde_json::Value) -> Option<Vec<Effort>> {
    let options = options.as_array()?;
    let mut efforts = Vec::new();
    for option in options {
        if option.get("type").and_then(serde_json::Value::as_str) != Some("effort") {
            continue;
        }
        for value in option
            .get("values")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(effort) = value.as_str().and_then(parse_live_effort) else {
                continue;
            };
            if !efforts.contains(&effort) {
                efforts.push(effort);
            }
        }
    }
    Some(efforts)
}

fn parse_live_effort(value: &str) -> Option<Effort> {
    match value.trim().to_ascii_lowercase().as_str() {
        "minimal" => Some(Effort::Minimal),
        "low" => Some(Effort::Low),
        "medium" => Some(Effort::Medium),
        "high" => Some(Effort::High),
        "xhigh" | "x-high" | "extra_high" | "extra-high" => Some(Effort::XHigh),
        "max" | "maximum" => Some(Effort::Max),
        _ => None,
    }
}

fn models_dev_provider_id(provider: &str) -> &str {
    match provider {
        "gemini" => "google",
        "github_copilot" => "github-copilot",
        "grok" => "xai",
        "opencode_go" => "opencode-go",
        _ => provider,
    }
}

fn live_model_effort_levels(provider: &str, model: &str) -> Option<Vec<Effort>> {
    let catalog = LIVE_MODEL_EFFORTS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    live_effort_levels_from(&catalog.provider_overrides, provider, model)
        .or_else(|| live_effort_levels_from(&catalog.by_provider, provider, model))
}

fn live_effort_levels_from(
    catalog: &LiveModelEffortCatalog,
    provider: &str,
    model: &str,
) -> Option<Vec<Effort>> {
    catalog
        .get(models_dev_provider_id(provider))?
        .get(model)
        .cloned()
}

#[cfg(not(test))]
async fn fetch_live_model_efforts(
    etag: Option<&str>,
) -> Result<Option<CachedLiveModelEfforts>, String> {
    let client = reqwest::Client::new();
    let mut request = client
        .get(LIVE_MODEL_CATALOG_URL)
        .timeout(Duration::from_secs(8));
    if let Some(etag) = etag {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }

    let mut response = request.send().await.map_err(|error| error.to_string())?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(None);
    }
    response = response
        .error_for_status()
        .map_err(|error| error.to_string())?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_LIVE_MODEL_CATALOG_BYTES as u64)
    {
        return Err("models.dev response exceeded the 16 MiB size limit".to_string());
    }

    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if body.len().saturating_add(chunk.len()) > MAX_LIVE_MODEL_CATALOG_BYTES {
            return Err("models.dev response exceeded the 16 MiB size limit".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    let json = std::str::from_utf8(&body).map_err(|error| error.to_string())?;
    let by_provider = parse_live_model_effort_catalog(json).map_err(|error| error.to_string())?;
    if by_provider.is_empty() {
        return Err("models.dev returned no model effort metadata".to_string());
    }

    Ok(Some(CachedLiveModelEfforts {
        by_provider,
        provider_overrides: HashMap::new(),
        etag,
    }))
}

fn catalog_model_spec(provider: &str, model: &str) -> Option<&'static ModelSpec> {
    let catalog = Catalog::builtin();
    let vendor = match provider {
        "gemini" => "gcp.gemini",
        "github_copilot" => "copilot",
        "grok" => "xai",
        _ => provider,
    };
    if let Some(spec) = catalog.resolve(&format!("{vendor}/{model}")) {
        return Some(spec);
    }

    match provider {
        "chatgpt" => catalog.resolve(&format!("openai/{model}")),
        "opencode_go" => {
            if let Some(spec) = catalog.resolve(&format!("openai/{model}")) {
                return Some(spec);
            }

            // ponytail: gateway IDs must resolve unambiguously by exact model ID; provider capability metadata would remove this ceiling.
            let mut matches = catalog.iter().filter(|spec| spec.id == model);
            let first = matches.next()?;
            let reasoning = &first.reasoning;
            matches
                .all(|spec| {
                    spec.reasoning.supported == reasoning.supported
                        && spec.reasoning.levels == reasoning.levels
                })
                .then_some(first)
        }
        _ => None,
    }
}

fn variant_definition_for_effort(effort: Effort) -> ModelVariantDefinition {
    let label = match effort {
        Effort::Minimal => "Minimal",
        Effort::Low => "Low",
        Effort::Medium => "Medium",
        Effort::High => "High",
        Effort::XHigh => "Extra high",
        Effort::Max => "Max",
        _ => effort.as_str(),
    };
    let description = match effort {
        Effort::Minimal => "Minimal reasoning effort for simple prompts.",
        Effort::Low => "Lower reasoning effort for faster responses.",
        Effort::Medium => "Medium reasoning effort for balanced responses.",
        Effort::High => "Higher reasoning effort for harder prompts.",
        Effort::XHigh => {
            "Extra-high reasoning effort for difficult prompts that benefit from deeper deliberation."
        }
        Effort::Max => "Maximum reasoning effort for the most demanding prompts.",
        _ => "Provider-supported reasoning effort.",
    };
    let id = if effort == Effort::XHigh {
        "reasoning-extra-high".to_string()
    } else {
        format!("reasoning-{}", effort.as_str())
    };

    ModelVariantDefinition {
        id,
        name: format!("{label} reasoning"),
        description: description.to_string(),
        deprecated: false,
        effort,
    }
}

fn sanitize_model_fetch_error(error: &str) -> (String, String) {
    use crate::credential_failure::{
        classify_credential_failure, entitlement_model_fetch_detail, CredentialFailureKind,
    };

    if let Some(kind) = classify_credential_failure(error) {
        return match kind {
            CredentialFailureKind::ReauthRequired => (
                "auth_failed".to_string(),
                "Provider credentials need attention.".to_string(),
            ),
            CredentialFailureKind::EntitlementBlocked => (
                "entitlement_failed".to_string(),
                entitlement_model_fetch_detail().to_string(),
            ),
        };
    }

    let lower = error.to_lowercase();
    let kind = if lower.contains("timeout") || lower.contains("timed out") {
        "timeout"
    } else if lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("rate-limited")
        || lower.contains("too many requests")
    {
        "rate_limited"
    } else if lower.contains("parse") || lower.contains("json") {
        "bad_response"
    } else if lower.contains("403")
        || lower.contains("unauthorized")
        || lower.contains("access_token")
        || lower.contains("oauth")
    {
        // Legacy auth signals that did not match the credential classifier.
        "auth_failed"
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

    // The async tests share the process-wide MODEL_CACHE / PENDING_REQUESTS
    // globals and clear_model_state() wipes the whole maps, so they must not
    // run in parallel or one test deletes the other's in-flight entry.
    static TEST_SERIAL: LazyLock<tokio::sync::Mutex<()>> =
        LazyLock::new(|| tokio::sync::Mutex::new(()));

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

    fn expected_reasoning_variant_ids() -> Vec<&'static str> {
        vec![
            "reasoning-low",
            "reasoning-medium",
            "reasoning-high",
            "reasoning-extra-high",
        ]
    }

    fn variant_ids(model: &ModelInfo) -> Vec<&str> {
        model
            .variants
            .iter()
            .map(|variant| variant.id.as_str())
            .collect::<Vec<_>>()
    }

    #[test]
    fn openai_reasoning_variants_decorate_supported_parent_models() {
        let model = ModelRegistry::model_info("openai", "gpt-5.2");

        assert_eq!(model.model, "gpt-5.2");
        assert_eq!(variant_ids(&model), expected_reasoning_variant_ids());
    }

    #[test]
    fn openai_reasoning_variant_resolves_to_rig_effort() {
        let resolved =
            ModelRegistry::resolve_model_variant("openai", "gpt-5.2", Some("reasoning-high"));

        assert_eq!(resolved.variant.as_deref(), Some("reasoning-high"));
        assert_eq!(resolved.effort, Some(Effort::High));
        assert_eq!(
            resolved.additional_params,
            Some(json!({ "reasoning": { "summary": "auto" } }))
        );
        assert!(resolved.warning.is_none());
    }

    #[test]
    fn openai_default_reasoning_model_requests_streamable_summary() {
        let resolved = ModelRegistry::resolve_model_variant("openai", "gpt-5.2", None);

        assert_eq!(resolved.variant, None);
        assert_eq!(
            resolved.additional_params,
            Some(json!({
                "reasoning": {
                    "summary": "auto"
                }
            }))
        );
        assert!(resolved.warning.is_none());
    }

    #[test]
    fn variants_follow_rig_catalog_effort_levels() {
        let chatgpt_model = ModelRegistry::model_info("chatgpt", "gpt-5.4-pro");
        assert_eq!(
            variant_ids(&chatgpt_model),
            vec!["reasoning-medium", "reasoning-high", "reasoning-extra-high"]
        );

        let anthropic_model = ModelRegistry::model_info("anthropic", "claude-opus-4-6");
        assert_eq!(
            variant_ids(&anthropic_model),
            vec![
                "reasoning-low",
                "reasoning-medium",
                "reasoning-high",
                "reasoning-max"
            ]
        );

        let resolved = ModelRegistry::resolve_model_variant(
            "anthropic",
            "claude-opus-4-6",
            Some("reasoning-max"),
        );
        assert_eq!(resolved.effort, Some(Effort::Max));
        assert!(resolved.additional_params.is_none());
    }

    #[test]
    fn live_models_dev_efforts_are_provider_scoped_and_ignore_other_option_types() {
        let catalog = parse_live_model_effort_catalog(
            r#"{
                "opencode-go": { "models": {
                    "gpt-6-luna": { "reasoning": true, "reasoning_options": [
                        { "type": "effort", "values": ["none", "low", "medium", "high", "xhigh", "max"] }
                    ] },
                    "budget-model": { "reasoning": true, "reasoning_options": [
                        { "type": "budget_tokens", "min": -1, "max": 32768 }
                    ] }
                } },
                "xai": { "models": {
                    "grok-3-mini": { "reasoning": true, "reasoning_options": [
                        { "type": "effort", "values": ["low", "high"] }
                    ] }
                } }
            }"#,
        )
        .unwrap();

        assert_eq!(
            live_effort_levels_from(&catalog, "opencode_go", "gpt-6-luna"),
            Some(vec![
                Effort::Low,
                Effort::Medium,
                Effort::High,
                Effort::XHigh,
                Effort::Max
            ])
        );
        assert_eq!(
            live_effort_levels_from(&catalog, "grok", "grok-3-mini"),
            Some(vec![Effort::Low, Effort::High])
        );
        assert_eq!(
            live_effort_levels_from(&catalog, "opencode_go", "budget-model"),
            Some(Vec::new())
        );
    }

    #[test]
    fn chatgpt_luna_keeps_its_existing_no_variant_override() {
        assert!(ModelRegistry::model_info("chatgpt", "gpt-5.6-luna")
            .variants
            .is_empty());
    }

    #[test]
    fn chatgpt_reasoning_variant_resolves_to_rig_effort() {
        let resolved = ModelRegistry::resolve_model_variant(
            "chatgpt",
            "gpt-5.4-pro",
            Some("reasoning-extra-high"),
        );

        assert_eq!(resolved.variant.as_deref(), Some("reasoning-extra-high"));
        assert_eq!(resolved.effort, Some(Effort::XHigh));
        assert_eq!(
            resolved.additional_params,
            Some(json!({ "reasoning": { "summary": "auto" } }))
        );
        assert!(resolved.warning.is_none());
    }

    #[test]
    fn chatgpt_default_reasoning_model_requests_streamable_summary() {
        let resolved = ModelRegistry::resolve_model_variant("chatgpt", "gpt-5.6-sol", None);

        assert_eq!(resolved.variant, None);
        assert_eq!(
            resolved.additional_params,
            Some(json!({
                "reasoning": {
                    "summary": "auto"
                }
            }))
        );
        assert!(resolved.warning.is_none());
    }

    #[test]
    fn opencode_go_gpt6_luna_exposes_and_resolves_reasoning_effort_variants() {
        let expected = [
            ("reasoning-low", Effort::Low),
            ("reasoning-medium", Effort::Medium),
            ("reasoning-high", Effort::High),
            ("reasoning-extra-high", Effort::XHigh),
            ("reasoning-max", Effort::Max),
        ];
        let model = ModelRegistry::model_info("opencode_go", "gpt-6-luna");

        assert_eq!(
            variant_ids(&model),
            expected.iter().map(|(id, _)| *id).collect::<Vec<_>>()
        );
        for (variant, effort) in expected {
            let resolved =
                ModelRegistry::resolve_model_variant("opencode_go", "gpt-6-luna", Some(variant));
            assert_eq!(resolved.variant.as_deref(), Some(variant));
            assert_eq!(resolved.effort, Some(effort));
            assert_eq!(
                resolved.additional_params,
                Some(json!({ "reasoning": { "summary": "auto" } }))
            );
            assert!(resolved.warning.is_none());
        }

        let default = ModelRegistry::resolve_model_variant("opencode_go", "gpt-6-luna", None);
        assert_eq!(default.variant, None);
        assert_eq!(
            default.additional_params,
            Some(json!({ "reasoning": { "summary": "auto" } }))
        );
    }

    #[test]
    fn missing_model_variant_falls_back_to_default_with_warning() {
        let resolved =
            ModelRegistry::resolve_model_variant("openai", "gpt-5.2", Some("missing-variant"));

        assert_eq!(resolved.variant, None);
        assert_eq!(resolved.effort, None);
        assert_eq!(
            resolved.additional_params,
            Some(json!({
                "reasoning": {
                    "summary": "auto"
                }
            }))
        );
        assert_eq!(
            resolved
                .warning
                .as_ref()
                .map(|warning| warning.kind.as_str()),
            Some("missing")
        );
    }

    #[test]
    fn test_chatgpt_subscription_models_include_current_codex_options() {
        assert!(ModelRegistry::is_chatgpt_subscription_model("gpt-5.6-sol"));
        assert!(ModelRegistry::is_chatgpt_subscription_model(
            "gpt-5.6-terra"
        ));
        assert!(ModelRegistry::is_chatgpt_subscription_model("gpt-5.6-luna"));
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

        assert_eq!(models.first(), Some(&"gpt-5.6-sol"));
        assert!(models.contains(&"gpt-5.6-sol"));
        assert!(models.contains(&"gpt-5.6-terra"));
        assert!(models.contains(&"gpt-5.6-luna"));
        assert!(models.contains(&"gpt-5.5"));
        assert!(models.contains(&"gpt-5.4"));
        assert!(models.contains(&"gpt-5.4-mini"));
        assert!(models.contains(&"gpt-5.3-codex-spark"));
        assert!(!models.contains(&"gpt-5.3-codex"));
        assert!(!models.contains(&"gpt-5.4-pro"));
    }

    #[test]
    fn test_github_copilot_is_registered_as_oauth_provider() {
        assert!(ModelRegistry::all_providers().contains(&"github_copilot"));
        assert!(ModelRegistry::is_oauth_provider("github_copilot"));
        assert_eq!(
            ModelRegistry::provider_display_name("github_copilot"),
            "GitHub Copilot"
        );
        assert_eq!(ModelRegistry::provider_auth_type("github_copilot"), "oauth");
    }

    #[test]
    fn test_grok_is_registered_as_oauth_provider() {
        assert!(ModelRegistry::all_providers().contains(&"grok"));
        assert_eq!(ModelRegistry::provider_display_name("grok"), "Grok");
        assert!(!ModelRegistry::models_for_provider("grok").is_empty());
        assert!(ModelRegistry::models_for_provider("grok")
            .iter()
            .all(|model| model.starts_with("grok-")));
    }

    #[test]
    fn test_xai_is_registered_as_api_key_provider() {
        assert!(ModelRegistry::all_providers().contains(&"xai"));
        assert_eq!(ModelRegistry::provider_display_name("xai"), "xAI");
        assert!(!ModelRegistry::is_oauth_provider("xai"));
        assert_eq!(ModelRegistry::provider_auth_type("xai"), "api_key");
        assert_eq!(
            ModelRegistry::models_for_provider("xai"),
            ModelRegistry::models_for_provider("grok")
        );
        assert!(ModelRegistry::hardcoded_models_for("xai")
            .iter()
            .all(|model| model.provider == "xai"));
    }

    #[test]
    fn sanitize_model_fetch_error_distinguishes_entitlement_from_reauth() {
        let (kind, detail) = sanitize_model_fetch_error(
            "HTTP 403 Forbidden: entitlement check failed for subscription tier",
        );
        assert_eq!(kind, "entitlement_failed");
        assert!(detail.contains("Signing in again will not fix this"));

        let (kind, detail) =
            sanitize_model_fetch_error("Grok OAuth session expired or invalid; sign in again");
        assert_eq!(kind, "auth_failed");
        assert_eq!(detail, "Provider credentials need attention.");
    }

    #[test]
    fn oauth_provider_ids_are_the_registered_credentialed_oauth_providers() {
        assert_eq!(
            crate::providers::oauth_provider_ids(),
            ["chatgpt", "github_copilot", "grok"]
        );
        assert_eq!(
            ModelRegistry::all_providers(),
            [
                "openai",
                "chatgpt",
                "github_copilot",
                "grok",
                "xai",
                "anthropic",
                "gemini",
                "groq",
                "mistral",
                "opencode_go"
            ]
        );
        assert!(ModelRegistry::is_oauth_provider("chatgpt"));
        assert!(ModelRegistry::is_oauth_provider("github_copilot"));
        assert!(ModelRegistry::is_oauth_provider("grok"));
        assert!(!ModelRegistry::is_oauth_provider("openai"));
        assert_eq!(ModelRegistry::provider_auth_type("chatgpt"), "oauth");
        assert_eq!(ModelRegistry::provider_auth_type("grok"), "oauth");
        assert_eq!(ModelRegistry::provider_auth_type("openai"), "api_key");
        assert_eq!(
            ModelRegistry::provider_display_name("chatgpt"),
            "ChatGPT Plus/Pro"
        );
        assert_eq!(
            ModelRegistry::provider_display_name("github_copilot"),
            "GitHub Copilot"
        );
        assert_eq!(ModelRegistry::provider_display_name("grok"), "Grok");
        for id in crate::providers::oauth_provider_ids() {
            assert!(ModelRegistry::all_providers().contains(&id));
            assert_eq!(ModelRegistry::provider_auth_type(id), "oauth");
            assert_ne!(ModelRegistry::provider_display_name(id), "Unknown Provider");
        }
    }

    #[test]
    fn test_github_copilot_has_no_hardcoded_model_fallback() {
        assert!(ModelRegistry::models_for_provider("github_copilot").is_empty());
        assert!(ModelRegistry::hardcoded_models_for("github_copilot").is_empty());
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
        let _serial = TEST_SERIAL.lock().await;
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
                        Ok(vec![ModelRegistry::model_info("openai", "gpt-4o")])
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

    /// Regression test for the waiter path of `get_or_fetch`: with a pending
    /// entry pre-inserted, the caller must take the waiter branch and wake
    /// when the leader completes. The `Notified` future is `enable()`d under
    /// the pending lock, so a `notify_waiters()` that fires before the
    /// waiter's first poll still wakes it; the timeouts below turn a lost
    /// wakeup into a test failure instead of a hang.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn waiter_wakes_when_leader_finished_before_first_poll() {
        let _serial = TEST_SERIAL.lock().await;
        clear_model_state().await;

        let cache_key = "lost-wakeup-cache-key";
        let leader_notify = Arc::new(Notify::new());
        {
            let mut pending = PENDING_REQUESTS.write().await;
            pending.insert(cache_key.to_string(), leader_notify.clone());
        }

        // Spawn the waiter; the pending entry makes it take the waiter path.
        let task = tokio::spawn(async move {
            tokio::time::timeout(
                Duration::from_secs(5),
                ModelRegistry::get_or_fetch(cache_key, "openai", false, || async {
                    Err("fetch_fn must not run for a waiter".to_string())
                }),
            )
            .await
        });

        // Wait until the spawned waiter has picked up the pending entry and
        // released the PENDING_REQUESTS lock: its Notify clone bumps the strong
        // count to 3 (map entry + this clone + the waiter's clone).
        tokio::time::timeout(Duration::from_secs(5), async {
            while Arc::strong_count(&leader_notify) < 3 {
                tokio::task::yield_now().await;
            }
            // Once the write lock is available again, the waiter's registration
            // block — including enable() on its Notified — has completed.
            let _pending = PENDING_REQUESTS.write().await;
        })
        .await
        .expect("waiter never reached its registration block");

        // Mirror a completing leader fetch: drop the pending entry, populate
        // the cache, then wake waiters.
        {
            let mut pending = PENDING_REQUESTS.write().await;
            pending.remove(cache_key);
        }
        {
            let mut cache = MODEL_CACHE.write().await;
            cache.insert(
                cache_key.to_string(),
                CachedModelList {
                    models: vec![ModelRegistry::model_info("openai", "gpt-4o")],
                    fetched_at: Instant::now(),
                    ttl: Duration::from_secs(60),
                },
            );
        }
        leader_notify.notify_waiters();

        let result = task
            .await
            .expect("waiter task panicked")
            .expect("waiter timed out waiting for wakeup");
        assert!(result.is_fresh);
        assert_eq!(result.models.len(), 1);
        assert_eq!(result.models[0].model, "gpt-4o");
        assert!(result.error_kind.is_none());
    }
}
