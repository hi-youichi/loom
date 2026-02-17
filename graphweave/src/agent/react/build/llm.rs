//! Builds the default OpenAI LLM from ReactBuildConfig.

use crate::error::AgentError;
use crate::llm::ChatOpenAI;
use crate::tool_source::ToolSource;
use crate::LlmClient;

use super::super::config::ReactBuildConfig;
use super::error::BuildRunnerError;

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
        .unwrap_or("gpt-4o-mini")
        .to_string();
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
    let (openai_config, model) = openai_config_from(config)?;
    let client = ChatOpenAI::with_config(openai_config, model);
    Ok(Box::new(client))
}

pub(crate) async fn build_default_llm_with_tool_source(
    config: &ReactBuildConfig,
    tool_source: &dyn ToolSource,
) -> Result<Box<dyn LlmClient>, BuildRunnerError> {
    let (openai_config, model) = openai_config_from(config)?;
    let client = ChatOpenAI::new_with_tool_source(openai_config, model, tool_source)
        .await
        .map_err(|e| BuildRunnerError::Context(AgentError::ExecutionFailed(e.to_string())))?;
    Ok(Box::new(client))
}
