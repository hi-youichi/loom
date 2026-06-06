//! L3 e2e: MCP config loading with mock LLM.
//!
//! Uses the same library path as the CLI: build_helve_config (discovers .loom/mcp.json)
//! then run_agent with llm_override = MockLlm, so no real API or MCP process is required.

mod init_logging;

use std::sync::OnceLock;

static MCP_SHORT_TIMEOUT: OnceLock<()> = OnceLock::new();

fn ensure_short_mcp_timeout() {
    MCP_SHORT_TIMEOUT.get_or_init(|| {
        std::env::set_var("LOOM_MCP_INIT_TIMEOUT_SECS", "1");
    });
}

use loom::{build_helve_config, MockLlm, RunCmd, RunOptions};
use loom_agent::run_agent_with_llm_override;
use std::path::PathBuf;

fn opts(working_folder: PathBuf) -> RunOptions {
    RunOptions {
        message: loom::UserContent::text("Hi"),
        working_folder: Some(working_folder),
        session_id: None,
        thread_id: None,
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
        debug_llm: false,
        provider: None,
        base_url: None,
        api_key: None,
        provider_type: None,
        any_stream_event_sender: None,
            bash_executor: None,
            extra_tools: None,
            acp_session_id: None,
            force_compact: false,
            chat_id: None,
            worktree: false,
        }
}

/// L3 e2e: project with .loom/mcp.json is discovered; run_agent with MockLlm completes and returns the mock reply.
#[tokio::test]
async fn mcp_config_discovered_and_run_with_mock_llm_returns_reply() {
    ensure_short_mcp_timeout();
    let dir = tempfile::tempdir().expect("tempdir");
    let working = dir.path().to_path_buf();
    let loom_dir = working.join(".loom");
    std::fs::create_dir_all(&loom_dir).expect("create .loom");
    let mcp_json = r#"{"mcpServers":{"test-server":{"command":"true","args":[]}}}"#;
    std::fs::write(loom_dir.join("mcp.json"), mcp_json).expect("write mcp.json");

    let opts_for_config = opts(working.clone());
    let (_, config, _) = build_helve_config(&opts_for_config);
    assert!(
        config
            .mcp_servers
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "mcp_servers should be loaded from .loom/mcp.json"
    );

    let empty_mcp = r#"{"mcpServers":{}}"#;
    std::fs::write(loom_dir.join("mcp.json"), empty_mcp).expect("write empty mcp.json");

    let opts_for_run = opts(working);
    let result = run_agent_with_llm_override(
        &opts_for_run,
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
