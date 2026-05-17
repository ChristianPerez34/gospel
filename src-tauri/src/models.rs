use serde::Serialize;

pub const OPENAI_MODELS: &[&str] = &[
    "gpt-4o",
    "gpt-4o-mini",
    "gpt-4-turbo",
    "gpt-3.5-turbo",
    "o1-preview",
    "o1-mini",
    "o1-pro",
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

#[derive(Serialize, Clone, Debug)]
pub struct ModelInfo {
    pub model: String,
    pub provider: String,
}

pub struct ModelRegistry;

impl ModelRegistry {
    pub fn models_for_provider(provider: &str) -> &'static [&'static str] {
        match provider {
            "openai" => OPENAI_MODELS,
            "anthropic" => ANTHROPIC_MODELS,
            "gemini" => GEMINI_MODELS,
            "groq" => GROQ_MODELS,
            "mistral" => MISTRAL_MODELS,
            _ => &[],
        }
    }

    pub fn all_providers() -> &'static [&'static str] {
        &["openai", "anthropic", "gemini", "groq", "mistral"]
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
}
