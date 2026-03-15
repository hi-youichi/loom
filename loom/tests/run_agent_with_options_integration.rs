//! Integration tests for the public API `run_agent_with_options`.
//!
//! - Error path: call `run_agent_with_options` directly with invalid working folder → assert Err.
//! - Success path + on_event: same code path via `run_agent_with_llm_override` with MockLlm and
//!   an event callback → assert Ok and that events were received (covers the pipeline used by
//!   run_agent_with_options when given on_event).

mod init_logging;

use loom::{
    run_agent_with_options, run_agent_with_llm_override, AnyStreamEvent, MockLlm, RunCmd, RunOptions,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

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
        output_timestamp: false,
    }
}

/// Integration test: run_agent_with_options with invalid working folder returns Err.
#[tokio::test]
async fn run_agent_with_options_invalid_working_folder_returns_err() {
    let opts = opts(PathBuf::from("/definitely/not/exist/loom-run-agent-with-options-test"));
    let res = run_agent_with_options(&opts, &RunCmd::React, None).await;
    assert!(res.is_err(), "run_agent_with_options should fail for invalid working folder");
}

/// Integration test: success path with on_event (same run path as run_agent_with_options).
/// Uses run_agent_with_llm_override so no real API is needed; asserts reply and that on_event was called.
#[tokio::test]
async fn run_agent_with_options_success_path_with_on_event_receives_events() {
    let dir = tempfile::tempdir().expect("tempdir");
    let working = dir.path().to_path_buf();
    let loom_dir = working.join(".loom");
    std::fs::create_dir_all(&loom_dir).expect("create .loom");
    let mcp_json = r#"{"mcpServers":{"test-server":{"command":"true","args":[]}}}"#;
    std::fs::write(loom_dir.join("mcp.json"), mcp_json).expect("write mcp.json");

    let opts = opts(working);
    let event_count = std::sync::Arc::new(AtomicUsize::new(0));
    let count = std::sync::Arc::clone(&event_count);
    let on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>> = Some(Box::new(move |_ev| {
        count.fetch_add(1, Ordering::Relaxed);
    }));

    let reply = run_agent_with_llm_override(
        &opts,
        &RunCmd::React,
        on_event,
        Some(Box::new(MockLlm::with_no_tool_calls("Done"))),
    )
    .await
    .expect("run_agent");

    assert_eq!(reply.trim(), "Done");
    assert!(
        event_count.load(Ordering::Relaxed) >= 1,
        "on_event should have been called at least once"
    );
}

/// Integration test: run_agent_with_options with on_event and invalid working folder still returns Err.
#[tokio::test]
async fn run_agent_with_options_with_on_event_invalid_working_folder_returns_err() {
    let opts = opts(PathBuf::from("/definitely/not/exist/loom-run-agent-with-options-test"));
    let event_count = std::sync::Arc::new(AtomicUsize::new(0));
    let count = std::sync::Arc::clone(&event_count);
    let on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>> = Some(Box::new(move |_ev| {
        count.fetch_add(1, Ordering::Relaxed);
    }));

    let res = run_agent_with_options(&opts, &RunCmd::React, on_event).await;
    assert!(res.is_err());
}
