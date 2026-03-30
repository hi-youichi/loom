//! Builds the default LLM from ReactBuildConfig (OpenAI or OpenAI-compat HTTP client).
//!
//! Default is OpenAI. Use the compat client when `LLM_PROVIDER=openai_compat` or `=bigmodel`.

use crate::error::AgentError;
use crate::llm::{ChatOpenAI, ChatOpenAICompat, ModelEntry};
use crate::tool_source::ToolSource;
use crate::LlmClient;

use super::super::config::ReactBuildConfig;
use super::error::BuildRunnerError;

/// Extract model configuration from ReactBuildConfig into a ModelEntry.
///
/// Priority (highest to lowest):
/// 1. ReactBuildConfig fields (credentials, model, provider, openai_temperature)
/// 2. Environment variables for any unset fields above (including `OPENAI_TEMPERATURE`)
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
        Some("openai_compat") => "openai_compat".to_string(),
        _ => "openai".to_string(),
    };

    let temperature = config
        .openai_temperature
        .clone()
        .or_else(|| std::env::var("OPENAI_TEMPERATURE").ok())
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| match s.parse::<f32>() {
            Ok(v) if v.is_finite() => Some(v),
            _ => {
                tracing::warn!(
                    value = %s,
                    "ignoring invalid OPENAI_TEMPERATURE (expected a finite number)"
                );
                None
            }
        });

    // Build ModelEntry
    Ok(ModelEntry {
        id: format!("{}/{}", provider, model),
        name: model,
        provider,
        base_url,
        api_key: Some(api_key),
        provider_type,
        temperature,
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
        "openai_compat" | "bigmodel" => {
            let base_url = entry.base_url.clone().unwrap_or_else(|| {
                "https://open.bigmodel.cn/api/paas/v4".to_string()
            });
            let api_key = entry.api_key.clone().unwrap();
            tracing::debug!("build_default_llm: OpenAI-compat with tools");
            let mut client =
                ChatOpenAICompat::with_config(base_url, api_key, entry.name).with_tools(tools);
            if let Some(mode) = entry.tool_choice {
                client = client.with_tool_choice(mode);
            }
            if let Some(t) = entry.temperature {
                client = client.with_temperature(t);
            }
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
            let mut client = ChatOpenAI::with_config(openai_config, entry.name).with_tools(tools);
            if let Some(mode) = entry.tool_choice {
                client = client.with_tool_choice(mode);
            }
            if let Some(t) = entry.temperature {
                client = client.with_temperature(t);
            }
            Ok(Box::new(client))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_model_entry_from_react_build_config() {
        let _guard = env_lock().lock().unwrap();
        let had_key = std::env::var("OPENAI_API_KEY").ok();
        std::env::set_var("OPENAI_API_KEY", "test-key-for-model-entry");
        let config = crate::agent::react::config::ReactBuildConfig::from_env();
        let entry = model_entry_from_config(&config);
        match had_key {
            Some(v) => std::env::set_var("OPENAI_API_KEY", v),
            None => std::env::remove_var("OPENAI_API_KEY"),
        }
        drop(_guard);

        assert!(entry.is_ok(), "model_entry_from_config: {:?}", entry.err());
        let entry = entry.unwrap();
        assert!(!entry.name.is_empty());
    }
}
