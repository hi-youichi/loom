//! Builds the default LLM from ReactBuildConfig (OpenAI or BigModel).
//!
//! Default is OpenAI. Use BigModel only when `LLM_PROVIDER=bigmodel` is set.

use crate::error::AgentError;
use crate::llm::{ChatBigModel, ChatOpenAI, ModelEntry};
use crate::tool_source::ToolSource;
use crate::LlmClient;

use super::super::config::ReactBuildConfig;
use super::error::BuildRunnerError;

/// Extract model configuration from ReactBuildConfig into a ModelEntry.
///
/// Priority (highest to lowest):
/// 1. ReactBuildConfig fields (openai_api_key, openai_base_url, model, llm_provider)
/// 2. Environment variables (OPENAI_API_KEY, OPENAI_BASE_URL, MODEL, LLM_PROVIDER)
/// 3. Default values
pub(crate) fn model_entry_from_config(config: &ReactBuildConfig) -> Result<ModelEntry, BuildRunnerError> {
    // API key: config > env
    let api_key = config
        .openai_api_key
        .clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .ok_or_else(|| {
            BuildRunnerError::Context(AgentError::ExecutionFailed(
                "OPENAI_API_KEY is not set".to_string(),
            ))
        })?;

    // Base URL: config > env (optional)
    let base_url = config
        .openai_base_url
        .clone()
        .or_else(|| std::env::var("OPENAI_BASE_URL").ok());

    // Model: config > env > default
    let model = config
        .model
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| std::env::var("MODEL").ok())
        .or_else(|| std::env::var("OPENAI_MODEL").ok())
        .unwrap_or_else(|| "gpt-4o-mini".to_string());

    // Provider type: config > env (optional)
    let provider_type = config
        .llm_provider
        .clone()
        .or_else(|| std::env::var("LLM_PROVIDER").ok());

    // Determine provider name based on type
    let provider = match provider_type.as_deref() {
        Some("bigmodel") => "bigmodel".to_string(),
        _ => "openai".to_string(),
    };

    // Build ModelEntry
    Ok(ModelEntry {
        id: format!("{}/{}", provider, model),
        name: model,
        provider,
        base_url,
        api_key: Some(api_key),
        provider_type,
        temperature: None,
        max_tokens: None,
        tool_choice: None,
    })
}


///
/// This is the async version that fetches tools from the tool source.
pub(crate) async fn build_default_llm_with_tool_source(
    config: &ReactBuildConfig,
    tool_source: &dyn ToolSource,
) -> Result<Box<dyn LlmClient>, BuildRunnerError> {
    let entry = model_entry_from_config(config)?;
    let provider_type = entry.provider_type.as_deref().unwrap_or("openai");

    let tools = tool_source.list_tools().await.map_err(|e| {
        BuildRunnerError::Context(AgentError::ExecutionFailed(format!(
            "Failed to list tools: {}",
            e
        )))
    })?;

    match provider_type {
        "bigmodel" => {
            let base_url = entry.base_url.clone().unwrap_or_else(|| {
                "https://open.bigmodel.cn/api/paas/v4".to_string()
            });
            let api_key = entry.api_key.clone().unwrap();
            tracing::debug!("build_default_llm: BigModel with tools");
            let client = ChatBigModel::with_config(base_url, api_key, entry.name).with_tools(tools);
            Ok(Box::new(client))
        }
        _ => {
            let mut openai_config = async_openai::config::OpenAIConfig::new();
            if let Some(ref api_key) = entry.api_key {
                openai_config = openai_config.with_api_key(api_key);
            }
            if let Some(ref base_url) = entry.base_url {
                let base_url = base_url.trim_end_matches('/');
                openai_config = openai_config.with_api_base(base_url);
            }
            tracing::debug!("build_default_llm: OpenAI with tools");
            let client = ChatOpenAI::with_config(openai_config, entry.name).with_tools(tools);
            Ok(Box::new(client))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_entry_from_react_build_config() {
        // Test that we can create a ModelEntry from ReactBuildConfig
        // This verifies the Phase 2 refactoring
        let config = crate::agent::react::config::ReactBuildConfig::from_env();
        let entry = model_entry_from_config(&config);
        
        // Should have a model name (either from env or default)
        assert!(entry.is_ok());
        let entry = entry.unwrap();
        assert!(!entry.name.is_empty());
    }
}
