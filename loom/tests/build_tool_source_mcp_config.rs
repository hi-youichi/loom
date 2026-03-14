//! L2 e2e: mcp_servers from config injected into build_react_run_context / build_tool_source.
//!
//! Scheme B: no real MCP process; assert build does not panic and tool source is built.

mod init_logging;

use loom::{build_helve_config, build_react_run_context, RunOptions};
use std::path::PathBuf;

fn opts(working_folder: PathBuf) -> RunOptions {
    RunOptions {
        message: String::new(),
        working_folder: Some(working_folder),
        session_id: None,
        thread_id: None,
        role_file: None,
        agent: None,
        verbose: false,
        got_adaptive: false,
        display_max_len: 120,
        output_json: false,
        model: None,
        mcp_config_path: None,
    }
}

/// mcp_servers from .loom/mcp.json are injected; build_react_run_context succeeds (MCP may fail to start and be skipped).
#[tokio::test]
async fn mcp_config_injected_into_build_tool_source() {
    let dir = tempfile::tempdir().unwrap();
    let working = dir.path().to_path_buf();
    std::fs::create_dir_all(working.join(".loom")).unwrap();
    let mcp_json = r#"{"mcpServers":{"test":{"command":"true","args":[]}}}"#;
    std::fs::write(working.join(".loom").join("mcp.json"), mcp_json).unwrap();

    let (_, config) = build_helve_config(&opts(working));
    assert!(
        config.mcp_servers.as_ref().map(|s| !s.is_empty()).unwrap_or(false),
        "mcp_servers should be loaded from .loom/mcp.json"
    );

    let ctx = build_react_run_context(&config).await.expect("build_react_run_context");
    let tools = ctx.tool_source.list_tools().await.expect("list_tools");
    // Base tools (bash, web_fetcher, etc.) are always present; MCP may have failed to start and been skipped
    assert!(!tools.is_empty());
}

/// No mcp.json: config.mcp_servers is None; build_react_run_context succeeds with base tools only.
#[tokio::test]
async fn mcp_config_empty_when_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let working = dir.path().to_path_buf();
    std::fs::create_dir_all(&working).unwrap();
    // no .loom/mcp.json; ensure no global config is used so discover returns None
    let xdg = tempfile::tempdir().unwrap();
    let prev = std::env::var("XDG_CONFIG_HOME").ok();
    std::env::set_var("XDG_CONFIG_HOME", xdg.path());

    let (_, config) = build_helve_config(&opts(working));
    assert!(
        config.mcp_servers.as_ref().map(|s| s.is_empty()).unwrap_or(true),
        "mcp_servers should be None or empty when no config file"
    );

    let ctx = build_react_run_context(&config).await.expect("build_react_run_context");
    let tools = ctx.tool_source.list_tools().await.expect("list_tools");
    assert!(!tools.is_empty());

    if let Some(p) = prev {
        std::env::set_var("XDG_CONFIG_HOME", p);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}

/// Override path points to invalid JSON; load fails (warn only), build continues with no mcp_servers.
#[tokio::test]
async fn mcp_config_invalid_json_logged_but_build_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let bad_json = dir.path().join("bad.json");
    std::fs::write(&bad_json, r#"{"mcpServers": {"x": "not an object"}}"#).unwrap();
    let working = dir.path().to_path_buf();
    std::fs::create_dir_all(&working).unwrap();

    let mut run_opts = opts(working);
    run_opts.mcp_config_path = Some(bad_json);
    let (_, config) = build_helve_config(&run_opts);
    // load_mcp_config_from_path fails for that file; build_helve_config only sets mcp_servers on Ok, so we get None or from_env
    let ctx = build_react_run_context(&config).await.expect("build_react_run_context");
    let _ = ctx.tool_source.list_tools().await.expect("list_tools");
}
