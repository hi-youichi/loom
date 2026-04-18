//! Integration test: builtin tool filter (enabled/disabled) works end-to-end.
//!
//! When ReactBuildConfig has `builtin_tool_filter` set, the built tool source
//! only lists and allows tools that pass the filter.

mod init_logging;

use loom::{build_react_run_context, BuiltinToolFilter, ReactBuildConfig};

fn make_config(filter: Option<BuiltinToolFilter>) -> (ReactBuildConfig, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let config = ReactBuildConfig {
        db_path: None,
        thread_id: None,
        trace_thread_id: None,
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
        mcp_github_args: vec![
            "-y".to_string(),
            "@modelcontextprotocol/server-github".to_string(),
        ],
        mcp_github_url: None,
        mcp_verbose: false,
        openai_api_key: None,
        openai_base_url: None,
        model: None,
        model_tier: None,
        llm_provider: None,
        openai_temperature: None,
        embedding_api_key: None,
        embedding_base_url: None,
        embedding_model: None,
        working_folder: Some(dir.path().to_path_buf()),
        approval_policy: None,
        compaction_config: None,
        tot_config: loom::TotRunnerConfig::default(),
        got_config: loom::GotRunnerConfig::default(),
        mcp_servers: None,
        skill_registry: None,
        max_sub_agent_depth: None,
        dry_run: false,
        builtin_tool_filter: filter,
    };
    (config, dir)
}

/// Scenario: disabled blacklist removes write_file and edit from available tools.
#[tokio::test]
async fn disabled_blacklist_removes_tools() {
    let filter = BuiltinToolFilter {
        enabled: None,
        disabled: Some(vec![
            "write_file".to_string(),
            "edit".to_string(),
            "multiedit".to_string(),
            "delete_file".to_string(),
        ]),
    };
    let (config, _dir) = make_config(Some(filter));
    let ctx = build_react_run_context(&config).await.unwrap();
    let tools = ctx.tool_source.list_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();

    assert!(
        !names.contains(&"write_file"),
        "write_file should be filtered out, got {:?}",
        names
    );
    assert!(
        !names.contains(&"edit"),
        "edit should be filtered out, got {:?}",
        names
    );
    assert!(
        !names.contains(&"multiedit"),
        "multiedit should be filtered out, got {:?}",
        names
    );
    assert!(
        !names.contains(&"delete_file"),
        "delete_file should be filtered out, got {:?}",
        names
    );
    // read and ls should still be present
    assert!(
        names.contains(&"read"),
        "read should be present, got {:?}",
        names
    );
    assert!(
        names.contains(&"ls"),
        "ls should be present, got {:?}",
        names
    );
}

/// Scenario: enabled whitelist keeps only specified tools.
#[tokio::test]
async fn enabled_whitelist_keeps_only_listed() {
    let filter = BuiltinToolFilter {
        enabled: Some(vec![
            "read".to_string(),
            "grep".to_string(),
            "glob".to_string(),
            "ls".to_string(),
        ]),
        disabled: None,
    };
    let (config, _dir) = make_config(Some(filter));
    let ctx = build_react_run_context(&config).await.unwrap();
    let tools = ctx.tool_source.list_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();

    // Only the 4 whitelisted tools should be present
    assert!(
        names.contains(&"read"),
        "read should be present, got {:?}",
        names
    );
    assert!(
        names.contains(&"grep"),
        "grep should be present, got {:?}",
        names
    );
    assert!(
        names.contains(&"glob"),
        "glob should be present, got {:?}",
        names
    );
    assert!(
        names.contains(&"ls"),
        "ls should be present, got {:?}",
        names
    );
    // write_file should NOT be present
    assert!(
        !names.contains(&"write_file"),
        "write_file should be filtered out, got {:?}",
        names
    );
    assert!(
        !names.contains(&"edit"),
        "edit should be filtered out, got {:?}",
        names
    );
}

/// Scenario: calling a disabled tool returns NotFound error.
#[tokio::test]
async fn calling_disabled_tool_returns_error() {
    let filter = BuiltinToolFilter {
        enabled: None,
        disabled: Some(vec!["write_file".to_string()]),
    };
    let (config, _dir) = make_config(Some(filter));
    let ctx = build_react_run_context(&config).await.unwrap();
    let result = ctx
        .tool_source
        .call_tool("write_file", serde_json::json!({}))
        .await;
    assert!(result.is_err(), "expected error for disabled tool");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("disabled"),
        "error should mention 'disabled', got: {}",
        err
    );
}

/// Scenario: no filter means all tools available (baseline).
#[tokio::test]
async fn no_filter_all_tools_available() {
    let (config, _dir) = make_config(None);
    let ctx = build_react_run_context(&config).await.unwrap();
    let tools = ctx.tool_source.list_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();

    assert!(
        names.contains(&"write_file"),
        "write_file should be available without filter, got {:?}",
        names
    );
    assert!(
        names.contains(&"read"),
        "read should be available without filter, got {:?}",
        names
    );
}
