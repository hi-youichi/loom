//! Builds tool source from ReactBuildConfig.

use std::sync::Arc;

use crate::error::AgentError;
use crate::tool_source::{
    register_file_tools, McpToolSource, MemoryToolsSource, ToolSource, ToolSourceError,
    YamlSpecToolSource,
};
use crate::tools::{
    register_mcp_tools, register_mcp_tools_with_specs, AggregateToolSource, BashTool, BatchTool,
    ExaCodesearchTool, ExaWebsearchTool, InvokeAgentTool, LspTool, TwitterSearchTool,
    WebFetcherTool,
};
#[cfg(windows)]
use crate::tools::powershell::PowerShellTool;

use env_config::McpServerDef;

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
    let working_folder_arc = config.working_folder.as_ref().map(|p| Arc::new(p.clone()));

    if !has_memory && !has_exa && !has_working_folder && !has_twitter {
        let aggregate = Arc::new(AggregateToolSource::new());
        aggregate
            .register_async(Box::new(WebFetcherTool::new()))
            .await;
        let bash_tool = match &working_folder_arc {
            Some(wf) => BashTool::with_working_folder(Arc::clone(wf)),
            None => BashTool::new(),
        };
        aggregate.register_async(Box::new(bash_tool)).await;
        #[cfg(windows)]
        {
            let ps_tool = match &working_folder_arc {
                Some(wf) => PowerShellTool::with_working_folder(Arc::clone(wf)),
                None => PowerShellTool::new(),
            };
            aggregate.register_async(Box::new(ps_tool)).await;
        }
        aggregate.register_sync(Box::new(BatchTool::new(Arc::clone(&aggregate))));
        aggregate.register_sync(Box::new(LspTool::new()));
        if let Some(ref servers) = config.mcp_servers {
            for def in servers {
                match def {
                    McpServerDef::Stdio {
                        name,
                        command,
                        args,
                        env,
                    } => {
                        tracing::debug!(name = %name, "starting MCP stdio server (spawn_blocking, pre-fetch tools)");
                        let command = command.clone();
                        let args = args.clone();
                        let env_vec: Vec<(String, String)> =
                            env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                        let mcp_verbose = config.mcp_verbose;
                        let create_result = tokio::task::spawn_blocking(move || {
                            let mcp = McpToolSource::new_with_env(
                                command,
                                args,
                                env_vec.into_iter(),
                                mcp_verbose,
                            )
                            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
                            let specs = mcp.list_tools_sync()?;
                            Ok::<_, ToolSourceError>((mcp, specs))
                        })
                        .await;
                        match create_result {
                            Ok(Ok((mcp, specs))) => {
                                register_mcp_tools_with_specs(
                                    aggregate.as_ref(),
                                    Arc::new(mcp),
                                    specs,
                                )
                                .await;
                            }
                            Ok(Err(e)) => {
                                tracing::warn!(
                                    name = %name,
                                    "mcp server failed to start or list tools, skipping: {}",
                                    e
                                );
                            }
                            Err(join_e) => {
                                tracing::warn!(
                                    name = %name,
                                    "mcp server spawn_blocking join failed: {}",
                                    join_e
                                );
                            }
                        }
                    }
                    McpServerDef::Http { name, url, headers } => {
                        let headers_iter = headers.iter().map(|(k, v)| (k.as_str(), v.as_str()));
                        match McpToolSource::new_http(url.clone(), headers_iter).await {
                            Ok(mcp) => {
                                if let Err(e) =
                                    register_mcp_tools(aggregate.as_ref(), Arc::new(mcp)).await
                                {
                                    tracing::warn!(
                                        name = %name,
                                        "mcp server (HTTP) registered but list/call may fail: {}",
                                        e
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    name = %name,
                                    "mcp server (HTTP) failed to connect, skipping: {}",
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }
        if let Some(ref token) = config.github_token {
            let use_http = config
                .mcp_github_url
                .as_deref()
                .map(|u| u.starts_with("http://") || u.starts_with("https://"))
                .unwrap_or(false);
            if use_http {
                let url = config.mcp_github_url.as_deref().unwrap();
                match McpToolSource::new_http(url, [("Authorization", format!("Bearer {}", token))])
                    .await
                {
                    Ok(mcp) => {
                        if let Err(e) = register_mcp_tools(aggregate.as_ref(), Arc::new(mcp)).await
                        {
                            tracing::warn!(
                                "GitHub MCP (HTTP) registered but list/call may fail: {}",
                                e
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("GitHub MCP (HTTP) failed to connect, skipping: {}", e);
                    }
                }
            } else {
                tracing::debug!("starting GitHub MCP (stdio, spawn_blocking, pre-fetch tools)");
                let cmd = config.mcp_github_cmd.clone();
                let args = config.mcp_github_args.clone();
                let env_github = vec![("GITHUB_TOKEN".to_string(), token.clone())];
                let mcp_verbose = config.mcp_verbose;
                let create_result = tokio::task::spawn_blocking(move || {
                    let mcp =
                        McpToolSource::new_with_env(cmd, args, env_github.into_iter(), mcp_verbose)
                            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
                    let specs = mcp.list_tools_sync()?;
                    Ok::<_, ToolSourceError>((mcp, specs))
                })
                .await;
                match create_result {
                    Ok(Ok((mcp, specs))) => {
                        register_mcp_tools_with_specs(aggregate.as_ref(), Arc::new(mcp), specs)
                            .await;
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("GitHub MCP failed to start or list tools, skipping: {}", e);
                    }
                    Err(join_e) => {
                        tracing::warn!("GitHub MCP spawn_blocking join failed: {}", join_e);
                    }
                }
            }
        }
        aggregate
            .register_async(Box::new(InvokeAgentTool::new(
                Arc::new(config.clone()),
                config.max_sub_agent_depth,
            )))
            .await;
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
    let bash_tool = match &working_folder_arc {
        Some(wf) => BashTool::with_working_folder(Arc::clone(wf)),
        None => BashTool::new(),
    };
    aggregate.register_async(Box::new(bash_tool)).await;
    
    // Register PowerShell tool on Windows
    #[cfg(windows)]
    {
        let ps_tool = match &working_folder_arc {
            Some(wf) => PowerShellTool::with_working_folder(Arc::clone(wf)),
            None => PowerShellTool::new(),
        };
        aggregate.register_async(Box::new(ps_tool)).await;
    }
    
    if let Some(ref key) = config.twitter_api_key {
        aggregate
            .register_async(Box::new(TwitterSearchTool::new(key.clone())))
            .await;
    }
    if let Some(ref key) = config.exa_api_key {
        aggregate
            .register_async(Box::new(ExaWebsearchTool::new(key.clone())))
            .await;
        if config.exa_codesearch_enabled {
            aggregate
                .register_async(Box::new(ExaCodesearchTool::new(key.clone())))
                .await;
        }
    }
    if let Some(ref wf) = config.working_folder {
        register_file_tools(aggregate.as_ref(), wf, config.skill_registry.clone())
            .map_err(to_agent_error)?;
    }
    aggregate.register_sync(Box::new(BatchTool::new(Arc::clone(&aggregate))));
    aggregate.register_sync(Box::new(LspTool::new()));

    if let Some(ref servers) = config.mcp_servers {
        for def in servers {
            match def {
                McpServerDef::Stdio {
                    name,
                    command,
                    args,
                    env,
                } => {
                    tracing::debug!(name = %name, "starting MCP stdio server (spawn_blocking, pre-fetch tools)");
                    let command = command.clone();
                    let args = args.clone();
                    let env_vec: Vec<(String, String)> =
                        env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    let mcp_verbose = config.mcp_verbose;
                    let create_result = tokio::task::spawn_blocking(move || {
                        let mcp = McpToolSource::new_with_env(
                            command,
                            args,
                            env_vec.into_iter(),
                            mcp_verbose,
                        )
                        .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
                        let specs = mcp.list_tools_sync()?;
                        Ok::<_, ToolSourceError>((mcp, specs))
                    })
                    .await;
                    match create_result {
                        Ok(Ok((mcp, specs))) => {
                            register_mcp_tools_with_specs(aggregate.as_ref(), Arc::new(mcp), specs)
                                .await;
                        }
                        Ok(Err(e)) => {
                            tracing::warn!(
                                name = %name,
                                "mcp server failed to start or list tools, skipping: {}",
                                e
                            );
                        }
                        Err(join_e) => {
                            tracing::warn!(
                                name = %name,
                                "mcp server spawn_blocking join failed: {}",
                                join_e
                            );
                        }
                    }
                }
                McpServerDef::Http { name, url, headers } => {
                    let headers_iter = headers.iter().map(|(k, v)| (k.as_str(), v.as_str()));
                    match McpToolSource::new_http(url.clone(), headers_iter).await {
                        Ok(mcp) => {
                            if let Err(e) =
                                register_mcp_tools(aggregate.as_ref(), Arc::new(mcp)).await
                            {
                                tracing::warn!(
                                    name = %name,
                                    "mcp server (HTTP) registered but list/call may fail: {}",
                                    e
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                name = %name,
                                "mcp server (HTTP) failed to connect, skipping: {}",
                                e
                            );
                        }
                    }
                }
            }
        }
    }
    if let Some(ref token) = config.github_token {
        let use_http = config
            .mcp_github_url
            .as_deref()
            .map(|u| u.starts_with("http://") || u.starts_with("https://"))
            .unwrap_or(false);
        if use_http {
            let url = config.mcp_github_url.as_deref().unwrap();
            match McpToolSource::new_http(url, [("Authorization", format!("Bearer {}", token))])
                .await
            {
                Ok(mcp) => {
                    if let Err(e) = register_mcp_tools(aggregate.as_ref(), Arc::new(mcp)).await {
                        tracing::warn!(
                            "GitHub MCP (HTTP) registered but list/call may fail: {}",
                            e
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!("GitHub MCP (HTTP) failed to connect, skipping: {}", e);
                }
            }
        } else {
            tracing::debug!("starting GitHub MCP (stdio, spawn_blocking, pre-fetch tools)");
            let cmd = config.mcp_github_cmd.clone();
            let args = config.mcp_github_args.clone();
            let env_github = vec![("GITHUB_TOKEN".to_string(), token.clone())];
            let mcp_verbose = config.mcp_verbose;
            let create_result = tokio::task::spawn_blocking(move || {
                let mcp =
                    McpToolSource::new_with_env(cmd, args, env_github.into_iter(), mcp_verbose)
                        .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
                let specs = mcp.list_tools_sync()?;
                Ok::<_, ToolSourceError>((mcp, specs))
            })
            .await;
            match create_result {
                Ok(Ok((mcp, specs))) => {
                    register_mcp_tools_with_specs(aggregate.as_ref(), Arc::new(mcp), specs).await;
                }
                Ok(Err(e)) => {
                    tracing::warn!("GitHub MCP failed to start or list tools, skipping: {}", e);
                }
                Err(join_e) => {
                    tracing::warn!("GitHub MCP spawn_blocking join failed: {}", join_e);
                }
            }
        }
    }

    aggregate
        .register_async(Box::new(InvokeAgentTool::new(
            Arc::new(config.clone()),
            config.max_sub_agent_depth,
        )))
        .await;

    let inner: Box<dyn ToolSource> = Box::new(aggregate);
    let wrapped = YamlSpecToolSource::wrap(inner)
        .await
        .map_err(to_agent_error)?;
    Ok(Box::new(wrapped))
}
