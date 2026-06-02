//! Models listing for OpenAI API.

use async_openai::config::OpenAIConfig;

use crate::traits::ModelInfo;
use crate::types::error::LlmError;

/// List available models from OpenAI API.
pub async fn list_models(
    config: &OpenAIConfig,
) -> Result<Vec<ModelInfo>, LlmError> {
    // For now, return an empty list or use config to fetch models
    // The actual implementation would use the client to call /v1/models
    Ok(Vec::new())
}