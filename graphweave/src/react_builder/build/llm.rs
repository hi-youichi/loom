//! Builds the default OpenAI LLM from [`ReactBuildConfig`](super::super::config::ReactBuildConfig).
//!
//! This module is used by [`build_react_runner`](super::build_react_runner) when the caller
//! passes `llm: None` and expects the library to construct an LLM from config (e.g. env or
//! CLI). It reads `openai_api_key`, `model`, and optionally `openai_base_url` from the config
//! and returns a [`LlmClient`](crate::LlmClient) implemented by [`ChatOpenAI`](crate::llm::ChatOpenAI).
//! When a tool source is provided, tools are registered so the LLM can request tool_calls.

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

/// Builds a default OpenAI chat LLM from the given ReAct build config.
///
/// Uses [`ReactBuildConfig::openai_api_key`](super::super::config::ReactBuildConfig#structfield.openai_api_key),
/// [`ReactBuildConfig::model`](super::super::config::ReactBuildConfig#structfield.model), and
/// optionally [`ReactBuildConfig::openai_base_url`](super::super::config::ReactBuildConfig#structfield.openai_base_url)
/// to construct a [`ChatOpenAI`](crate::llm::ChatOpenAI) client wrapped as `Box<dyn LlmClient>`.
///
/// # Arguments
///
/// * `config` - ReAct build config; must have a non-empty `openai_api_key` for success.
///
/// # Returns
///
/// * `Ok(Box<dyn LlmClient>)` - A chat LLM client ready for use with [`ReactRunner`](crate::react::ReactRunner).
///
/// # Errors
///
/// * [`BuildRunnerError::NoLlm`](super::error::BuildRunnerError::NoLlm) - When `config.openai_api_key`
///   is `None` or empty (no API key available to build the default LLM).
///
/// # Behavior
///
/// * **Model**: If `config.model` is `None` or empty, defaults to `"gpt-4o-mini"`.
/// * **Base URL**: If `config.openai_base_url` is set and non-empty, it is used (trailing slash
///   trimmed); otherwise the default OpenAI API base is used via
///   [`OpenAIConfig`](async_openai::config::OpenAIConfig).
#[allow(dead_code)] // Kept for callers who build an LLM without tools (e.g. tests).
pub(crate) fn build_default_llm(
    config: &ReactBuildConfig,
) -> Result<Box<dyn LlmClient>, BuildRunnerError> {
    let (openai_config, model) = openai_config_from(config)?;
    let client = ChatOpenAI::with_config(openai_config, model);
    Ok(Box::new(client))
}

/// Builds a default OpenAI chat LLM with tools from the given tool source.
///
/// Same as [`build_default_llm`] but registers tools from `tool_source` so the LLM
/// can request tool_calls in its responses. Use the same tool source for [`ActNode`](crate::react::ActNode).
///
/// # Errors
///
/// * [`BuildRunnerError::NoLlm`] - When `config.openai_api_key` is missing.
/// * [`BuildRunnerError::Context`] - When `tool_source.list_tools()` fails.
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
