//! Title generation: use a Light-tier model to produce a short conversation title from the first user message.

use loom_llm::message::Message;
use loom_llm::traits::LlmClient;
use loom_llm::ChatOpenAI;
use model_spec_core::spec::ModelTier;
use std::time::Duration;

const TITLE_SYSTEM_PROMPT: &str = "Generate a concise title (max 30 characters) for a conversation that starts with this message. Reply with ONLY the title text, no quotes, no explanation. Use the same language as the user's message.";

const TITLE_TIMEOUT: Duration = Duration::from_secs(10);

const MAX_TITLE_LENGTH: usize = 80;

const FALLBACK_MAX_CHARS: usize = 20;

fn fallback_title(user_message: &str) -> String {
    let chars: Vec<char> = user_message.chars().collect();
    let truncate_len = std::cmp::min(FALLBACK_MAX_CHARS, chars.len());
    chars[..truncate_len].iter().collect()
}

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

    let providers = loom_tier::provider::load_provider_configs()?;
    let entry = loom_tier::resolve::resolve_from_spec(provider, ModelTier::Light, &providers).await?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_title_short_message() {
        let result = fallback_title("Hello");
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_fallback_title_exact_length() {
        let message = "12345678901234567890";
        let result = fallback_title(message);
        assert_eq!(result, message);
    }

    #[test]
    fn test_fallback_title_long_message() {
        let message = "1234567890123456789012345";
        let result = fallback_title(message);
        assert_eq!(result.len(), 20);
        assert_eq!(result, "12345678901234567890");
    }

    #[test]
    fn test_fallback_title_empty_message() {
        let result = fallback_title("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_fallback_title_unicode_characters() {
        let message = "你好世界，这是一个测试";
        let result = fallback_title(message);
        assert_eq!(result, message);
    }

    #[test]
    fn test_fallback_title_mixed_unicode() {
        let message = "Hello你好World世界";
        let result = fallback_title(message);
        assert_eq!(result, message);
    }

    #[test]
    fn test_constants_are_reasonable() {
        assert_eq!(TITLE_TIMEOUT.as_secs(), 10);
        assert_eq!(MAX_TITLE_LENGTH, 80);
        assert_eq!(FALLBACK_MAX_CHARS, 20);
    }

    #[test]
    fn test_fallback_title_multibyte_truncation() {
        let message = "12345678901234567890你好";
        let result = fallback_title(message);
        assert_eq!(result.len(), 20);
        assert_eq!(result, "12345678901234567890");
    }

    #[tokio::test]
    async fn test_generate_title_fallback_path() {
        let result = generate_title("test message", None).await;
        assert_eq!(result, "test message");
    }
}
