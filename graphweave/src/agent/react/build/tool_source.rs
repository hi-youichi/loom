//! Builds tool source from ReactBuildConfig.

use std::sync::Arc;

use crate::error::AgentError;
use crate::tool_source::{
    register_file_tools, MemoryToolsSource, ToolSource, YamlSpecToolSource,
};
use crate::tools::{
    register_mcp_tools, AggregateToolSource, BashTool, TwitterSearchTool, WebFetcherTool,
};
use crate::tool_source::McpToolSource;

use super::super::config::ReactBuildConfig;

fn to_agent_error(e: impl std::fmt::Display) -> AgentError {
    AgentError::ExecutionFailed(e.to_string())
}

const DEFAULT_MEMORY_NAMESPACE: &[&str] = &["default", "memories"];

async fn register_exa_mcp(
    config: &ReactBuildConfig,
    aggregate: &AggregateToolSource,
) -> Result<(), AgentError> {
    let key = match config.exa_api_key.as_ref() {
        Some(k) => k,
        None => return Ok(()),
    };
    let url = config.mcp_exa_url.trim();
    let use_http = url.starts_with("http://") || url.starts_with("https://");

    let mcp = if use_http {
        McpToolSource::new_http(url, [("EXA_API_KEY", key.as_str())])
            .await
            .map_err(to_agent_error)?
    } else {
        let args: Vec<String> = config
            .mcp_remote_args
            .split_whitespace()
            .map(String::from)
            .collect();
        let mut args = args;
        if !args
            .iter()
            .any(|a| a == &config.mcp_exa_url || a.contains("mcp.exa.ai"))
        {
            args.push(config.mcp_exa_url.clone());
        }
        let mut env = vec![("EXA_API_KEY".to_string(), key.clone())];
        if let Ok(home) = std::env::var("HOME") {
            env.push(("HOME".to_string(), home));
        }
        McpToolSource::new_with_env(config.mcp_remote_cmd.clone(), args, env, config.mcp_verbose)
            .map_err(to_agent_error)?
    };
    register_mcp_tools(aggregate, Arc::new(mcp))
        .await
        .map_err(to_agent_error)?;
    Ok(())
}

pub(crate) async fn build_tool_source(
    config: &ReactBuildConfig,
    store: &Option<Arc<dyn crate::memory::Store>>,
) -> Result<Box<dyn ToolSource>, AgentError> {
    let has_memory = store.is_some();
    let has_exa = config.exa_api_key.is_some();
    let has_working_folder = config.working_folder.is_some();
    let has_twitter = config.twitter_api_key.is_some();

    if !has_memory && !has_exa && !has_working_folder && !has_twitter {
        let aggregate = AggregateToolSource::new();
        aggregate
            .register_async(Box::new(WebFetcherTool::new()))
            .await;
        aggregate.register_async(Box::new(BashTool::new())).await;
        let inner: Box<dyn ToolSource> = Box::new(aggregate);
        let wrapped = YamlSpecToolSource::wrap(inner)
            .await
            .map_err(to_agent_error)?;
        return Ok(Box::new(wrapped));
    }

    let aggregate = if has_memory {
        let s = store.as_ref().unwrap();
        let namespace: Vec<String> = config
            .user_id
            .as_ref()
            .map(|u| vec![u.clone(), "memories".to_string()])
            .unwrap_or_else(|| {
                DEFAULT_MEMORY_NAMESPACE
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect()
            });
        MemoryToolsSource::new(s.clone(), namespace).await
    } else {
        AggregateToolSource::new()
    };

    aggregate
        .register_async(Box::new(WebFetcherTool::new()))
        .await;
    aggregate.register_async(Box::new(BashTool::new())).await;
    if let Some(ref key) = config.twitter_api_key {
        aggregate
            .register_async(Box::new(TwitterSearchTool::new(key.clone())))
            .await;
    }
    register_exa_mcp(config, &aggregate).await?;
    if let Some(ref wf) = config.working_folder {
        register_file_tools(&aggregate, wf).map_err(to_agent_error)?;
    }

    let inner: Box<dyn ToolSource> = Box::new(aggregate);
    let wrapped = YamlSpecToolSource::wrap(inner)
        .await
        .map_err(to_agent_error)?;
    Ok(Box::new(wrapped))
}
