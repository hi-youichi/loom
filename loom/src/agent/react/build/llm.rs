//! Builds the default LLM from ReactBuildConfig (OpenAI or OpenAI-compat HTTP client).
//!
//! Only `LLM_PROVIDER=openai` uses the native `async_openai` client; all other providers
//! (including the default when the model prefix is not `openai/`) use `ChatOpenAICompat`.

use crate::error::AgentError;
use crate::llm::{ChatOpenAI, ChatOpenAICompat, ModelEntry};
use crate::tool_source::ToolSource;
use crate::LlmClient;

use super::super::config::ReactBuildConfig;
use super::error::BuildRunnerError;

fn parse_provider_model(model: &str) -> Option<(&str, &str)> {
    let (provider, model_id) = model.split_once('/')?;
    let provider = provider.trim();
    let model_id = model_id.trim();
    if provider.is_empty() || model_id.is_empty() {
        return None;
    }
    Some((provider, model_id))
}

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
    let raw_model = config
        .model
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| std::env::var("MODEL").ok())
        .or_else(|| std::env::var("OPENAI_MODEL").ok())
        .unwrap_or_else(|| "gpt-4o-mini".to_string());

    // Explicit provider type: config > env (optional)
    let explicit_provider_type = config
        .llm_provider
        .clone()
        .or_else(|| std::env::var("LLM_PROVIDER").ok());
    // Inferred provider type from MODEL when explicit provider type is not set.
    let inferred_provider_type = parse_provider_model(&raw_model).map(|(provider, _)| {
        if provider.eq_ignore_ascii_case("openai") {
            "openai".to_string()
        } else {
            "openai_compat".to_string()
        }
    });
    let provider_type = explicit_provider_type
        .clone()
        .or_else(|| inferred_provider_type.clone());
    let model = parse_provider_model(&raw_model)
        .map(|(_, model_id)| model_id.to_string())
        .unwrap_or(raw_model.clone());

    if matches!(provider_type.as_deref(), Some("openai_compat" | "bigmodel")) && base_url.is_none() {
        let detail = if explicit_provider_type.is_none() {
            parse_provider_model(&raw_model)
                .map(|(provider, _)| format!("inferred from MODEL provider '{}'", provider))
                .unwrap_or_else(|| "inferred provider".to_string())
        } else {
            "LLM_PROVIDER/provider type".to_string()
        };
        return Err(BuildRunnerError::Context(AgentError::ExecutionFailed(format!(
            "OPENAI_BASE_URL is required for OpenAI-compatible providers ({detail})."
        ))));
    }

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
    let provider_type = entry.provider_type.as_deref().unwrap_or_else(|| {
        if entry.provider.eq_ignore_ascii_case("openai") {
            "openai"
        } else {
            "openai_compat"
        }
    });

    let tools = tool_source.list_tools().await.map_err(|e| {
        BuildRunnerError::Context(AgentError::ExecutionFailed(format!(
            "Failed to list tools: {}",
            e
        )))
    })?;

    match provider_type {
        "openai" => {
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
            Ok(Box::new(client) as Box<dyn LlmClient>)
        }
        _ => {
            let base_url = entry.base_url.clone()
                .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
                .ok_or_else(|| {
                    BuildRunnerError::Context(AgentError::ExecutionFailed(format!(
                        "OPENAI_BASE_URL is required for non-openai provider '{}'", provider_type
                    )))
                })?;
            let api_key = entry.api_key.clone().ok_or_else(|| {
                BuildRunnerError::Context(AgentError::ExecutionFailed(format!(
                    "api_key is required for provider '{}'", provider_type
                )))
            })?;
            tracing::debug!(provider_type = %provider_type, "build_default_llm: OpenAI-compat with tools");
            let mut client =
                ChatOpenAICompat::with_config(base_url, api_key, entry.name).with_tools(tools);
            if let Some(mode) = entry.tool_choice {
                client = client.with_tool_choice(mode);
            }
            if let Some(t) = entry.temperature {
                client = client.with_temperature(t);
            }
            Ok(Box::new(client) as Box<dyn LlmClient>)
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

    fn restore_env(key: &str, old: Option<String>) {
        match old {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn model_provider_prefix_non_openai_infers_openai_compat_and_normalizes_model() {
        let _guard = env_lock().lock().unwrap();
        let old_key = std::env::var("OPENAI_API_KEY").ok();
        let old_base = std::env::var("OPENAI_BASE_URL").ok();
        let old_provider = std::env::var("LLM_PROVIDER").ok();
        std::env::set_var("OPENAI_API_KEY", "test-key");
        std::env::set_var("OPENAI_BASE_URL", "https://open.bigmodel.cn/api/paas/v4");
        std::env::remove_var("LLM_PROVIDER");

        let mut config = crate::agent::react::config::ReactBuildConfig::from_env();
        config.model = Some("zhipuai-coding-plan/glm-5".to_string());
        config.llm_provider = None;
        let entry = model_entry_from_config(&config).unwrap();

        restore_env("OPENAI_API_KEY", old_key);
        restore_env("OPENAI_BASE_URL", old_base);
        restore_env("LLM_PROVIDER", old_provider);
        drop(_guard);

        assert_eq!(entry.provider_type.as_deref(), Some("openai_compat"));
        assert_eq!(entry.provider, "openai_compat");
        assert_eq!(entry.name, "glm-5");
    }

    #[test]
    fn model_provider_prefix_openai_infers_openai_and_normalizes_model() {
        let _guard = env_lock().lock().unwrap();
        let old_key = std::env::var("OPENAI_API_KEY").ok();
        let old_base = std::env::var("OPENAI_BASE_URL").ok();
        let old_provider = std::env::var("LLM_PROVIDER").ok();
        std::env::set_var("OPENAI_API_KEY", "test-key");
        std::env::remove_var("OPENAI_BASE_URL");
        std::env::remove_var("LLM_PROVIDER");

        let mut config = crate::agent::react::config::ReactBuildConfig::from_env();
        config.model = Some("openai/gpt-4o".to_string());
        config.llm_provider = None;
        let entry = model_entry_from_config(&config).unwrap();

        restore_env("OPENAI_API_KEY", old_key);
        restore_env("OPENAI_BASE_URL", old_base);
        restore_env("LLM_PROVIDER", old_provider);
        drop(_guard);

        assert_eq!(entry.provider_type.as_deref(), Some("openai"));
        assert_eq!(entry.provider, "openai");
        assert_eq!(entry.name, "gpt-4o");
    }

    #[test]
    fn explicit_provider_type_overrides_model_provider_inference() {
        let _guard = env_lock().lock().unwrap();
        let old_key = std::env::var("OPENAI_API_KEY").ok();
        let old_provider = std::env::var("LLM_PROVIDER").ok();
        std::env::set_var("OPENAI_API_KEY", "test-key");
        std::env::set_var("LLM_PROVIDER", "openai");

        let mut config = crate::agent::react::config::ReactBuildConfig::from_env();
        config.model = Some("zhipuai-coding-plan/glm-5".to_string());
        config.llm_provider = None;
        let entry = model_entry_from_config(&config).unwrap();

        restore_env("OPENAI_API_KEY", old_key);
        restore_env("LLM_PROVIDER", old_provider);
        drop(_guard);

        assert_eq!(entry.provider_type.as_deref(), Some("openai"));
        assert_eq!(entry.provider, "openai");
        assert_eq!(entry.name, "glm-5");
    }

    #[test]
    fn inferred_openai_compat_requires_base_url() {
        let _guard = env_lock().lock().unwrap();
        let old_key = std::env::var("OPENAI_API_KEY").ok();
        let old_base = std::env::var("OPENAI_BASE_URL").ok();
        let old_provider = std::env::var("LLM_PROVIDER").ok();
        std::env::set_var("OPENAI_API_KEY", "test-key");
        std::env::remove_var("OPENAI_BASE_URL");
        std::env::remove_var("LLM_PROVIDER");

        let mut config = crate::agent::react::config::ReactBuildConfig::from_env();
        config.model = Some("zhipuai-coding-plan/glm-5".to_string());
        config.llm_provider = None;
        let err = model_entry_from_config(&config).unwrap_err().to_string();

        restore_env("OPENAI_API_KEY", old_key);
        restore_env("OPENAI_BASE_URL", old_base);
        restore_env("LLM_PROVIDER", old_provider);
        drop(_guard);

        assert!(err.contains("OPENAI_BASE_URL is required"));
        assert!(err.contains("zhipuai-coding-plan"));
    }
}
