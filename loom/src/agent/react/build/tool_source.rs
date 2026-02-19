//! Builds tool source from ReactBuildConfig.

use std::sync::Arc;

use crate::error::AgentError;
use crate::tool_source::{
    register_file_tools, MemoryToolsSource, ToolSource, YamlSpecToolSource,
};
use crate::tools::{
    AggregateToolSource, BashTool, BatchTool, ExaCodesearchTool, ExaWebsearchTool, LspTool,
    TwitterSearchTool, WebFetcherTool,
};

use super::super::config::ReactBuildConfig;

fn to_agent_error(e: impl std::fmt::Display) -> AgentError {
    AgentError::ExecutionFailed(e.to_string())
}

const DEFAULT_MEMORY_NAMESPACE: &[&str] = &["default", "memories"];

pub(crate) async fn build_tool_source(
    config: &ReactBuildConfig,
    store: &Option<Arc<dyn crate::memory::Store>>,
) -> Result<Box<dyn ToolSource>, AgentError> {
    let has_memory = store.is_some();
    let has_exa = config.exa_api_key.is_some();
    let has_working_folder = config.working_folder.is_some();
    let has_twitter = config.twitter_api_key.is_some();

    if !has_memory && !has_exa && !has_working_folder && !has_twitter {
        let aggregate = Arc::new(AggregateToolSource::new());
        aggregate
            .register_async(Box::new(WebFetcherTool::new()))
            .await;
        aggregate.register_async(Box::new(BashTool::new())).await;
        aggregate
            .register_sync(Box::new(BatchTool::new(Arc::clone(&aggregate))));
        aggregate.register_sync(Box::new(LspTool::new()));
        let inner: Box<dyn ToolSource> = Box::new(aggregate);
        let wrapped = YamlSpecToolSource::wrap(inner)
            .await
            .map_err(to_agent_error)?;
        return Ok(Box::new(wrapped));
    }

    let base = if has_memory {
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
    let aggregate = Arc::new(base);

    aggregate
        .register_async(Box::new(WebFetcherTool::new()))
        .await;
    aggregate.register_async(Box::new(BashTool::new())).await;
    if let Some(ref key) = config.twitter_api_key {
        aggregate
            .register_async(Box::new(TwitterSearchTool::new(key.clone())))
            .await;
    }
    if let Some(ref key) = config.exa_api_key {
        aggregate
            .register_async(Box::new(ExaWebsearchTool::new(key.clone())))
            .await;
        aggregate
            .register_async(Box::new(ExaCodesearchTool::new(key.clone())))
            .await;
    }
    if let Some(ref wf) = config.working_folder {
        register_file_tools(aggregate.as_ref(), wf).map_err(to_agent_error)?;
    }
    aggregate.register_sync(Box::new(BatchTool::new(Arc::clone(&aggregate))));
    aggregate.register_sync(Box::new(LspTool::new()));

    let inner: Box<dyn ToolSource> = Box::new(aggregate);
    let wrapped = YamlSpecToolSource::wrap(inner)
        .await
        .map_err(to_agent_error)?;
    Ok(Box::new(wrapped))
}
