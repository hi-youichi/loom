//! Builds the default LLM from ReactBuildConfig (OpenAI or BigModel).
//!
//! Default is OpenAI. Use BigModel only when `LLM_PROVIDER=bigmodel` is set.

use crate::error::AgentError;
use crate::llm::{ChatBigModel, ChatOpenAI, ToolChoiceMode};
use crate::tool_source::ToolSource;
use crate::LlmClient;

use super::super::config::ReactBuildConfig;
use super::error::BuildRunnerError;

/// True only when provider is explicitly set to "bigmodel". Default (no provider) → OpenAI.
fn use_bigmodel(config: &ReactBuildConfig) -> bool {
    config
        .llm_provider
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("bigmodel"))
        .unwrap_or(false)
}

/// BigModel uses the same config as OpenAI (openai_api_key, openai_base_url, model).
fn bigmodel_config_from(
    config: &ReactBuildConfig,
) -> Result<(String, String, String), BuildRunnerError> {
    let api_key = config
        .openai_api_key
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or(BuildRunnerError::NoLlm)?;
    let base_url = config
        .openai_base_url
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| "https://open.bigmodel.cn/api/paas/v4".to_string());
    let model = config
        .model
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| std::env::var("MODEL").ok())
        .or_else(|| std::env::var("OPENAI_MODEL").ok())
        .unwrap_or_else(|| "glm-4.7-flash".to_string());
    Ok((base_url, api_key.to_string(), model))
}

fn openai_config_from(
    config: &ReactBuildConfig,
) -> Result<(async_openai::config::OpenAIConfig, String), BuildRunnerError> {
    use async_openai::config::OpenAIConfig;

    let api_key = config
        .openai_api_key
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or(BuildRunnerError::NoLlm)?;
    let model = config
        .model
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| std::env::var("MODEL").ok())
        .or_else(|| std::env::var("OPENAI_MODEL").ok())
        .unwrap_or_else(|| "gpt-4o-mini".to_string());
    let mut openai_config = OpenAIConfig::new().with_api_key(api_key);
    if let Some(ref base) = config.openai_base_url {
        if !base.is_empty() {
            let base = base.trim_end_matches('/');
            openai_config = openai_config.with_api_base(base);
        }
    }
    Ok((openai_config, model))
}

fn env_temperature() -> Result<Option<f32>, BuildRunnerError> {
    let Some(raw) = std::env::var("OPENAI_TEMPERATURE").ok() else {
        return Ok(None);
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    raw.parse::<f32>().map(Some).map_err(|e| {
        BuildRunnerError::Context(AgentError::ExecutionFailed(format!(
            "invalid OPENAI_TEMPERATURE '{}': {}",
            raw, e
        )))
    })
}

fn env_tool_choice_mode() -> Result<Option<ToolChoiceMode>, BuildRunnerError> {
    let Some(raw) = std::env::var("OPENAI_TOOL_CHOICE").ok() else {
        return Ok(None);
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    raw.parse::<ToolChoiceMode>().map(Some).map_err(|e| {
        BuildRunnerError::Context(AgentError::ExecutionFailed(format!(
            "invalid OPENAI_TOOL_CHOICE '{}': {}",
            raw, e
        )))
    })
}

#[allow(dead_code)]
pub(crate) fn build_default_llm(
    config: &ReactBuildConfig,
) -> Result<Box<dyn LlmClient>, BuildRunnerError> {
    let temperature = env_temperature()?;
    let tool_choice = env_tool_choice_mode()?;
    if use_bigmodel(config) {
        let (base_url, api_key, model) = bigmodel_config_from(config)?;
        let mut client = ChatBigModel::with_config(base_url, api_key, model);
        if let Some(t) = temperature {
            client = client.with_temperature(t);
        }
        if let Some(mode) = tool_choice {
            client = client.with_tool_choice(mode);
        }
        Ok(Box::new(client))
    } else {
        let (openai_config, model) = openai_config_from(config)?;
        let mut client = ChatOpenAI::with_config(openai_config, model);
        if let Some(t) = temperature {
            client = client.with_temperature(t);
        }
        if let Some(mode) = tool_choice {
            client = client.with_tool_choice(mode);
        }
        Ok(Box::new(client))
    }
}

pub(crate) async fn build_default_llm_with_tool_source(
    config: &ReactBuildConfig,
    tool_source: &dyn ToolSource,
) -> Result<Box<dyn LlmClient>, BuildRunnerError> {
    let temperature = env_temperature()?;
    let tool_choice = env_tool_choice_mode()?;
    if use_bigmodel(config) {
        let (base_url, api_key, model) = bigmodel_config_from(config)?;
        tracing::debug!("build_default_llm: BigModel, fetching tools from tool_source");
        let mut client = ChatBigModel::new_with_tool_source(base_url, api_key, model, tool_source)
            .await
            .map_err(|e| BuildRunnerError::Context(AgentError::ExecutionFailed(e.to_string())))?;
        if let Some(t) = temperature {
            client = client.with_temperature(t);
        }
        if let Some(mode) = tool_choice {
            client = client.with_tool_choice(mode);
        }
        tracing::debug!("build_default_llm: ready (BigModel)");
        Ok(Box::new(client))
    } else {
        let (openai_config, model) = openai_config_from(config)?;
        tracing::debug!("build_default_llm: fetching tools from tool_source for model");
        let mut client = ChatOpenAI::new_with_tool_source(openai_config, model, tool_source)
            .await
            .map_err(|e| BuildRunnerError::Context(AgentError::ExecutionFailed(e.to_string())))?;
        if let Some(t) = temperature {
            client = client.with_temperature(t);
        }
        if let Some(mode) = tool_choice {
            client = client.with_tool_choice(mode);
        }
        tracing::debug!("build_default_llm: ready");
        Ok(Box::new(client))
    }
}
