#[allow(unused_imports)]
use rig::providers::{anthropic, chatgpt, gemini, groq, mistral, openai};

/// Dispatches to the correct rig provider client based on the provider string.
///
/// Usage:
/// ```ignore
/// provider_client!(provider, api_key, client_err, unsupported_err, |client| {
///     let agent = client.agent(model).build();
///     agent.prompt(prompt).await.map_err(client_err)?
/// })
/// ```
///
/// - `$client_err`: expression converting client construction `String` errors
/// - `$unsupported_err`: expression converting the unsupported-provider `String`
macro_rules! provider_client {
    ($provider:expr, $api_key:expr, $client_err:expr, $unsupported_err:expr, |$client:ident| $body:block) => {
        match $provider {
            "openai" => {
                let $client = openai::Client::new($api_key)
                    .map_err(|e| $client_err(e.to_string()))?;
                $body
            }
            "chatgpt" => {
                let $client = chatgpt::Client::builder()
                    .oauth()
                    .build()
                    .map_err(|e| $client_err(e.to_string()))?;
                $body
            }
            "anthropic" => {
                let $client = anthropic::Client::new($api_key)
                    .map_err(|e| $client_err(e.to_string()))?;
                $body
            }
            "gemini" => {
                let $client = gemini::Client::new($api_key)
                    .map_err(|e| $client_err(e.to_string()))?;
                $body
            }
            "groq" => {
                let $client = groq::Client::new($api_key)
                    .map_err(|e| $client_err(e.to_string()))?;
                $body
            }
            "mistral" => {
                let $client = mistral::Client::new($api_key)
                    .map_err(|e| $client_err(e.to_string()))?;
                $body
            }
            other => return Err($unsupported_err(other.to_string())),
        }
    };
}
pub(crate) use provider_client;
