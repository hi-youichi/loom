use std::sync::Arc;

use crate::tools::InvokeAgentTool;
use loom_llm::error::AgentError;
use memory_v2::MemoryStore;
use skill::SkillUsageStore;
use tool_basic::{
    bash::BashTool, batch::BatchTool, exa::ExaCodesearchTool, exa::ExaWebsearchTool,
    mcp::McpToolSource, powershell::PowerShellTool, register_file_tools, register_mcp_tools,
    web::WebFetcherTool,
};
use tool_core::{ArcTool, ToolRegistryLocked, YamlSpecError};
use tool_experimental::{register_file_memory_tool, register_task_tools};
use tool_extensions::twitter::TwitterSearchTool;

use env_config::McpServerDef;

use super::super::config::ReactBuildConfig;

fn to_agent_error(e: impl std::fmt::Display) -> AgentError {
    AgentError::ExecutionFailed(e.to_string())
}

pub(crate) async fn build_tool_source(
    config: &ReactBuildConfig,
    _store: &Option<Arc<dyn loom_memory::Store>>,
) -> Result<Arc<ToolRegistryLocked>, AgentError> {
    let working_folder_arc = config.working_folder.as_ref().map(|p| Arc::new(p.clone()));

    let aggregate = Arc::new(ToolRegistryLocked::new());

    aggregate
        .register_async(Box::new(WebFetcherTool::new()))
        .await;
    #[cfg(not(windows))]
    let bash_tool = if let Some(ref executor) = config.bash_executor {
        match &working_folder_arc {
            Some(wf) => {
                BashTool::with_working_folder_and_executor(Arc::clone(wf), executor.clone())
            }
            None => BashTool::with_executor(executor.clone()),
        }
    } else {
        match &working_folder_arc {
            Some(wf) => BashTool::with_working_folder(Arc::clone(wf)),
            None => BashTool::new(),
        }
    };
    #[cfg(not(windows))]
    aggregate.register_async(Box::new(bash_tool)).await;

    #[cfg(windows)]
    if let Some(ref executor) = config.bash_executor {
        let bash_tool = match &working_folder_arc {
            Some(wf) => {
                BashTool::with_working_folder_and_executor(Arc::clone(wf), executor.clone())
            }
            None => BashTool::with_executor(executor.clone()),
        };
        aggregate.register_async(Box::new(bash_tool)).await;
    } else {
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
        register_file_tools(
            aggregate.as_ref(),
            wf,
            config.skill_registry.clone(),
            Some(SkillUsageStore::new(&wf.join(".loom/skills"))),
        )
        .map_err(to_agent_error)?;

        let db_path = env_config::home::loom_home().join("tasks").join("tasks.db");
        let db_dir = db_path.parent().unwrap();
        let _ = std::fs::create_dir_all(db_dir);
        if config.goal_mode {
            if let Ok(db) = task_core::TaskDb::open(&db_path).await {
                register_task_tools(&aggregate, Arc::new(db)).await;
            }
        }

        let memory_store = Arc::new(MemoryStore::new(&MemoryStore::default_path()));
        register_file_memory_tool(&aggregate, memory_store).await;
    }
    if let Some(ref wf) = config.working_folder {
        aggregate.register_sync(Box::new(BatchTool::new(Arc::new(wf.clone()))));
    }
    // aggregate.register_sync(Box::new(LspTool::default()));

    if let Some(ref servers) = config.mcp_servers {
        for def in servers {
            match def {
                McpServerDef::Stdio {
                    name,
                    command,
                    args,
                    env,
                } => {
                    tracing::debug!(name = %name, "starting MCP stdio server");
                    let command = command.clone();
                    let args = args.clone();
                    let env_vec: Vec<(String, String)> =
                        env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    let mcp_verbose = config.mcp_verbose;
                    match McpToolSource::new_with_env(command, args, env_vec, mcp_verbose).await {
                        Ok(mcp) => {
                            if let Err(e) =
                                register_mcp_tools(aggregate.as_ref(), Arc::new(mcp)).await
                            {
                                tracing::warn!(
                                    name = %name,
                                    "mcp server registered but list/call may fail: {}",
                                    e
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                name = %name,
                                "mcp server failed to start, skipping: {}",
                                e
                            );
                        }
                    }
                }
                McpServerDef::Http {
                    name,
                    url,
                    headers,
                    oauth: _,
                    ..
                } => {
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
            tracing::debug!("starting GitHub MCP (stdio)");
            let cmd = config.mcp_github_cmd.clone();
            let args = config.mcp_github_args.clone();
            let env_github = vec![("GITHUB_TOKEN".to_string(), token.clone())];
            let mcp_verbose = config.mcp_verbose;
            match McpToolSource::new_with_env(cmd, args, env_github, mcp_verbose).await {
                Ok(mcp) => {
                    if let Err(e) = register_mcp_tools(aggregate.as_ref(), Arc::new(mcp)).await {
                        tracing::warn!("GitHub MCP registered but list/call may fail: {}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("GitHub MCP failed to start, skipping: {}", e);
                }
            }
        }
    }

    if let Some(ref tools) = config.extra_tools {
        for tool in tools.iter() {
            aggregate
                .register_async(Box::new(ArcTool(tool.clone())))
                .await;
        }
    }

    aggregate
        .register_async(Box::new(InvokeAgentTool::new(
            Arc::new(config.clone()),
            config.max_sub_agent_depth,
        )))
        .await;
    // ListAgentsTool is not available in this build (depends on loom's profile system)

    apply_registry_config(&aggregate, config)
        .await
        .map_err(to_agent_error)?;

    Ok(aggregate)
}

async fn apply_registry_config(
    aggregate: &Arc<ToolRegistryLocked>,
    config: &ReactBuildConfig,
) -> Result<(), YamlSpecError> {
    aggregate.load_yaml_specs().await?;
    if let Some(ref filter) = config.builtin_tool_filter {
        if !filter.is_noop() {
            tracing::info!(
                enabled = ?filter.enabled,
                disabled = ?filter.disabled,
                "applying builtin tool filter"
            );
            aggregate.set_filter(Some(filter.clone())).await;
        }
    }
    if config.dry_run {
        aggregate.set_dry_run(true).await;
    }
    Ok(())
}
