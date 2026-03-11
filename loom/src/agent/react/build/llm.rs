//! Builds the default LLM from ReactBuildConfig (OpenAI or BigModel).
//!
//! Default is OpenAI. Use BigModel only when `LLM_PROVIDER=bigmodel` is set.

use crate::error::AgentError;
use crate::llm::{ChatBigModel, ChatOpenAI};
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

#[allow(dead_code)]
pub(crate) fn build_default_llm(
    config: &ReactBuildConfig,
) -> Result<Box<dyn LlmClient>, BuildRunnerError> {
    if use_bigmodel(config) {
        let (base_url, api_key, model) = bigmodel_config_from(config)?;
        let client = ChatBigModel::with_config(base_url, api_key, model);
        Ok(Box::new(client))
    } else {
        let (openai_config, model) = openai_config_from(config)?;
        let client = ChatOpenAI::with_config(openai_config, model);
        Ok(Box::new(client))
    }
}

pub(crate) async fn build_default_llm_with_tool_source(
    config: &ReactBuildConfig,
    tool_source: &dyn ToolSource,
) -> Result<Box<dyn LlmClient>, BuildRunnerError> {
    if use_bigmodel(config) {
        let (base_url, api_key, model) = bigmodel_config_from(config)?;
        tracing::debug!("build_default_llm: BigModel, fetching tools from tool_source");
        let client = ChatBigModel::new_with_tool_source(base_url, api_key, model, tool_source)
            .await
            .map_err(|e| BuildRunnerError::Context(AgentError::ExecutionFailed(e.to_string())))?;
        tracing::debug!("build_default_llm: ready (BigModel)");
        Ok(Box::new(client))
    } else {
        let (openai_config, model) = openai_config_from(config)?;
        tracing::debug!("build_default_llm: fetching tools from tool_source for model");
        let client = ChatOpenAI::new_with_tool_source(openai_config, model, tool_source)
            .await
            .map_err(|e| BuildRunnerError::Context(AgentError::ExecutionFailed(e.to_string())))?;
        tracing::debug!("build_default_llm: ready");
        Ok(Box::new(client))
    }
}
