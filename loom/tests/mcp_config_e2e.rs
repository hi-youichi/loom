//! L3 e2e: MCP config loading with mock LLM.
//!
//! Uses the same library path as the CLI: build_helve_config (discovers .loom/mcp.json)
//! then run_agent with llm_override = MockLlm, so no real API or MCP process is required.

mod init_logging;

use loom::{
    build_helve_config, run_agent_with_llm_override, MockLlm, RunCmd, RunOptions,
};
use std::path::PathBuf;

fn opts(working_folder: PathBuf) -> RunOptions {
    RunOptions {
        message: "Hi".to_string(),
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
        cancellation: None,
        output_timestamp: false,
        dry_run: false,
    }
}

/// L3 e2e: project with .loom/mcp.json is discovered; run_agent with MockLlm completes and returns the mock reply.
#[tokio::test]
async fn mcp_config_discovered_and_run_with_mock_llm_returns_reply() {
    let dir = tempfile::tempdir().expect("tempdir");
    let working = dir.path().to_path_buf();
    let loom_dir = working.join(".loom");
    std::fs::create_dir_all(&loom_dir).expect("create .loom");
    let mcp_json = r#"{"mcpServers":{"test-server":{"command":"true","args":[]}}}"#;
    std::fs::write(loom_dir.join("mcp.json"), mcp_json).expect("write mcp.json");

    let opts = opts(working.clone());
    let (_, config, _) = build_helve_config(&opts);
    assert!(
        config.mcp_servers.as_ref().map(|s| !s.is_empty()).unwrap_or(false),
        "mcp_servers should be loaded from .loom/mcp.json"
    );

    let result = run_agent_with_llm_override(
        &opts,
        &RunCmd::React,
        None,
        Some(Box::new(MockLlm::with_no_tool_calls("Done"))),
    )
    .await
    .expect("run_agent");

    match &result {
        loom::RunCompletion::Finished(r) => assert_eq!(r.reply.trim(), "Done"),
        loom::RunCompletion::Cancelled => panic!("expected finished run"),
    }
}
