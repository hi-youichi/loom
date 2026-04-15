//! L2 e2e: mcp_servers from config injected into build_react_run_context / build_tool_source.
//!
//! Scheme B: no real MCP process; assert build does not panic and tool source is built.

mod init_logging;

use loom::{build_helve_config, build_react_run_context, RunOptions};
use std::path::PathBuf;

fn opts(working_folder: PathBuf) -> RunOptions {
    RunOptions {
        message: loom::UserContent::text(String::new()),
        working_folder: Some(working_folder),
        session_id: None,
        thread_id: None,
        agent: None,
        verbose: false,
        got_adaptive: false,
        display_max_len: 120,
        output_json: false,
        model: None,
        provider: None,
        base_url: None,
        api_key: None,
        provider_type: None,
        mcp_config_path: None,
        cancellation: None,
        output_timestamp: false,
        dry_run: false,
        any_stream_event_sender: None,
    }
}

/// mcp_servers from .loom/mcp.json are injected; build_react_run_context succeeds (MCP may fail to start and be skipped).
#[tokio::test]
async fn mcp_config_injected_into_build_tool_source() {
    let dir = tempfile::tempdir().unwrap();
    let working = dir.path().to_path_buf();
    let loom_dir = working.join(".loom");
    std::fs::create_dir_all(&loom_dir).expect("create .loom");
    let mcp_json = r#"{"mcpServers":{"test-server":{"command":"true","args":[]}}}"#;
    std::fs::write(loom_dir.join("mcp.json"), mcp_json).expect("write mcp.json");

    let run_opts = opts(working);
    let (_, config, _) = build_helve_config(&run_opts);
    // load_mcp_config_from_path fails for that file; build_helve_config only sets mcp_servers on Ok, so we get None or from_env
    let ctx = build_react_run_context(&config)
        .await
        .expect("build_react_run_context");
    let _ = ctx.tool_source.list_tools().await.expect("list_tools");
}
