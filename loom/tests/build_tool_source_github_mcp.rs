//! L2: build_tool_source with GitHub MCP (github_token None vs invalid command).
//!
//! When github_token is None, no GitHub MCP is started. When github_token is set but
//! mcp_github_cmd is invalid, spawn fails and we skip GitHub MCP (build still succeeds).

mod init_logging;

use loom::{build_react_run_context, ReactBuildConfig};

fn base_config(working_folder: std::path::PathBuf) -> ReactBuildConfig {
    ReactBuildConfig {
        db_path: None,
        thread_id: None,
        user_id: None,
        system_prompt: None,
        exa_api_key: None,
        exa_codesearch_enabled: false,
        twitter_api_key: None,
        mcp_exa_url: "https://mcp.exa.ai/mcp".to_string(),
        mcp_remote_cmd: "npx".to_string(),
        mcp_remote_args: "-y mcp-remote".to_string(),
        github_token: None,
        mcp_github_cmd: "npx".to_string(),
        mcp_github_args: vec!["-y".to_string(), "@modelcontextprotocol/server-github".to_string()],
        mcp_github_url: None,
        mcp_verbose: false,
        openai_api_key: None,
        openai_base_url: None,
        model: None,
        llm_provider: None,
        openai_tool_choice: None,
        openai_temperature: None,
        embedding_api_key: None,
        embedding_base_url: None,
        embedding_model: None,
        working_folder: Some(working_folder),
        approval_policy: None,
        compaction_config: None,
        tot_config: loom::TotRunnerConfig::default(),
        got_config: loom::GotRunnerConfig::default(),
        mcp_servers: None,
        skill_registry: None,
        max_sub_agent_depth: None,
        dry_run: false,
    }
}

/// github_token None: build succeeds, no GitHub MCP started (base tools only).
#[tokio::test]
async fn github_token_none_build_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let config = base_config(dir.path().to_path_buf());
    let ctx = build_react_run_context(&config).await.expect("build_react_run_context");
    let tools = ctx.tool_source.list_tools().await.expect("list_tools");
    assert!(!tools.is_empty());
    // Base tools (e.g. bash, web_fetcher, file tools) are present; no GitHub MCP tools required.
}

/// github_token Some but invalid command: spawn fails, we skip GitHub MCP, build still succeeds.
#[tokio::test]
async fn github_mcp_invalid_command_build_succeeds_github_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = base_config(dir.path().to_path_buf());
    config.github_token = Some("x".to_string());
    config.mcp_github_cmd = "_nonexistent_command_".to_string();
    config.mcp_github_args = vec!["-y".to_string(), "nonexistent".to_string()];
    let ctx = build_react_run_context(&config).await.expect("build_react_run_context");
    let tools = ctx.tool_source.list_tools().await.expect("list_tools");
    assert!(!tools.is_empty());
}
