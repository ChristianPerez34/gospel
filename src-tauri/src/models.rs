pub const OPENAI_MODELS: &[&str] = &[
    "gpt-4o",
    "gpt-4o-mini",
    "gpt-4-turbo",
    "gpt-3.5-turbo",
    "o1-preview",
    "o1-mini",
    "o1-pro",
];

pub struct ModelRegistry;

impl ModelRegistry {
    pub fn models_for_provider(provider: &str) -> &'static [&'static str] {
        match provider {
            "openai" => OPENAI_MODELS,
            _ => &[],
        }
    }

    pub fn all_providers() -> &'static [&'static str] {
        &["openai"]
    }
}
