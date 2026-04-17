//! Title generation: use a Light-tier model to produce a short conversation title from the first user message.

use crate::llm::{ChatOpenAI, LlmClient};
use crate::message::Message;
use model_spec_core::spec::ModelTier;
use std::time::Duration;

const TITLE_SYSTEM_PROMPT: &str = "Generate a concise title (max 30 characters) for a conversation that starts with this message. Reply with ONLY the title text, no quotes, no explanation. Use the same language as the user's message.";

const TITLE_TIMEOUT: Duration = Duration::from_secs(10);

const MAX_TITLE_LENGTH: usize = 80;

const FALLBACK_MAX_CHARS: usize = 20;

/// Generate a title for a conversation based on the first user message.
///
/// Uses the Light-tier model from the same provider as the given model string.
/// Falls back to truncating the message if the LLM call fails or times out.
///
/// # Arguments
/// * `user_message` - The text of the first user message
/// * `model` - Optional model string (e.g. "openai/gpt-4o") used to resolve the provider for Light tier
pub async fn generate_title(user_message: &str, model: Option<&str>) -> String {
    match generate_title_llm(user_message, model).await {
        Some(title) => title,
        None => fallback_title(user_message),
    }
}

async fn generate_title_llm(user_message: &str, model: Option<&str>) -> Option<String> {
    let model_str = model?;
    let (provider, _model_id) = model_str.split_once('/')?;

    let providers = load_providers()?;
    let registry = crate::llm::ModelRegistry::global();
    let entry = registry
        .resolve_tier(provider, ModelTier::Light, &providers)
        .await?;

    let config = async_openai::config::OpenAIConfig::new()
        .with_api_key(entry.api_key.unwrap_or_default());
    let config = match entry.base_url {
        Some(ref url) => config.with_api_base(url),
        None => config,
    };

    let client = ChatOpenAI::with_config(config, entry.id).with_temperature(0.3);

    let messages = vec![
        Message::system(TITLE_SYSTEM_PROMPT),
        Message::user(user_message),
    ];

    let result = tokio::time::timeout(TITLE_TIMEOUT, client.invoke(&messages)).await;

    match result {
        Ok(Ok(response)) => {
            let title = response.content.trim().to_string();
            if title.is_empty() {
                return None;
            }
            let truncated: String = title.chars().take(MAX_TITLE_LENGTH).collect();
            Some(truncated)
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "title generation LLM call failed");
            None
        }
        Err(_) => {
            tracing::warn!("title generation timed out after {:?}", TITLE_TIMEOUT);
            None
        }
    }
}

fn fallback_title(user_message: &str) -> String {
    let truncated: String = user_message.chars().take(FALLBACK_MAX_CHARS).collect();
    if user_message.chars().count() > FALLBACK_MAX_CHARS {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

fn load_providers() -> Option<Vec<crate::llm::ProviderConfig>> {
    let config = env_config::load_full_config("loom").ok()?;
    Some(
        config
            .providers
            .into_iter()
            .map(|p| crate::llm::ProviderConfig {
                name: p.name,
                base_url: p.base_url,
                api_key: p.api_key,
                provider_type: p.provider_type,
                fetch_models: p.fetch_models.unwrap_or(false),
            })
            .collect(),
    )
}
