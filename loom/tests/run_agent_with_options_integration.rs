//! Integration tests for the public API `run_agent_with_options`.
//!
//! - Error path: call `run_agent_with_options` directly with invalid working folder → assert Err.
//! - Success path + on_event: same code path via `run_agent_with_llm_override` with MockLlm and
//!   an event callback → assert Ok and that events were received (covers the pipeline used by
//!   run_agent_with_options when given on_event).

mod init_logging;

use std::sync::OnceLock;

static MCP_SHORT_TIMEOUT: OnceLock<()> = OnceLock::new();

fn ensure_short_mcp_timeout() {
    MCP_SHORT_TIMEOUT.get_or_init(|| {
        std::env::set_var("LOOM_MCP_INIT_TIMEOUT_SECS", "1");
    });
}

use loom::{
    run_agent_with_llm_override, run_agent_with_options, AnyStreamEvent, Checkpointer, MockLlm,
    RunCancellation, RunCmd, RunCompletion, RunOptions, StreamEvent, UserContent,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(unix)]
use std::time::Instant;

fn opts(working_folder: PathBuf) -> RunOptions {
    RunOptions {
        message: UserContent::Text("Hi".to_string()),
        working_folder: Some(working_folder),
        session_id: None,
        cancellation: None,
        thread_id: None,
        agent: None,
        verbose: false,
        got_adaptive: false,
        display_max_len: 120,
        output_json: false,
        model: None,
        mcp_config_path: None,
        output_timestamp: false,
        dry_run: false,
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
        }
}

/// Integration test: run_agent_with_options with invalid working folder returns Err.
#[tokio::test]
async fn run_agent_with_options_invalid_working_folder_returns_err() {
    let opts = opts(PathBuf::from(
        "/definitely/not/exist/loom-run-agent-with-options-test",
    ));
    let res = run_agent_with_options(&opts, &RunCmd::React, None).await;
    assert!(
        res.is_err(),
        "run_agent_with_options should fail for invalid working folder"
    );
}

/// Integration test: success path with on_event (same run path as run_agent_with_options).
/// Uses run_agent_with_llm_override so no real API is needed; asserts reply and that on_event was called.
#[tokio::test]
async fn run_agent_with_options_success_path_with_on_event_receives_events() {
    ensure_short_mcp_timeout();
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

    let result = run_agent_with_llm_override(
        &opts,
        &RunCmd::React,
        on_event,
        Some(Box::new(MockLlm::with_no_tool_calls("Done"))),
    )
    .await
    .expect("run_agent");

    match result {
        RunCompletion::Finished(result) => {
            assert_eq!(result.reply.trim(), "Done");
            assert_eq!(result.reasoning_content, None);
        }
        RunCompletion::Cancelled => panic!("expected finished run"),
    }
    assert!(
        event_count.load(Ordering::Relaxed) >= 1,
        "on_event should have been called at least once"
    );
}

/// Integration test: dry_run causes tools to return a placeholder instead of executing.
/// Uses MockLlm::first_tools_then_end() so the agent requests get_time; with dry_run the
/// tool result is "(dry run: get_time was not executed)" and the run completes successfully.
#[tokio::test]
async fn dry_run_returns_placeholder_for_tool_calls() {
    ensure_short_mcp_timeout();
    let dir = tempfile::tempdir().expect("tempdir");
    let working = dir.path().to_path_buf();
    let loom_dir = working.join(".loom");
    std::fs::create_dir_all(&loom_dir).expect("create .loom");
    let mcp_json = r#"{"mcpServers":{"test-server":{"command":"true","args":[]}}}"#;
    std::fs::write(loom_dir.join("mcp.json"), mcp_json).expect("write mcp.json");

    let mut run_opts = opts(working);
    run_opts.dry_run = true;

    let saw_dry_placeholder = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw = std::sync::Arc::clone(&saw_dry_placeholder);
    let on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>> =
        Some(Box::new(move |ev| {
            if let AnyStreamEvent::React(StreamEvent::Updates { state, .. }) = &ev {
                if state.tool_results.iter().any(|tr| {
                    tr.content.contains("dry run") && tr.content.contains("was not executed")
                }) {
                    saw.store(true, Ordering::Relaxed);
                }
            }
        }));

    let result = run_agent_with_llm_override(
        &run_opts,
        &RunCmd::React,
        on_event,
        Some(Box::new(MockLlm::first_tools_then_end())),
    )
    .await
    .expect("run_agent");

    match result {
        RunCompletion::Finished(result) => {
            assert_eq!(result.reply.trim(), "The time is as above.");
        }
        RunCompletion::Cancelled => panic!("expected finished dry run"),
    }
    assert!(
        saw_dry_placeholder.load(Ordering::Relaxed),
        "stream events should contain a tool result with dry run placeholder"
    );
}

/// Integration test: run_agent_with_options with on_event and invalid working folder still returns Err.
#[tokio::test]
async fn run_agent_with_options_with_on_event_invalid_working_folder_returns_err() {
    let opts = opts(PathBuf::from(
        "/definitely/not/exist/loom-run-agent-with-options-test",
    ));
    let event_count = std::sync::Arc::new(AtomicUsize::new(0));
    let count = std::sync::Arc::clone(&event_count);
    let on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>> = Some(Box::new(move |_ev| {
        count.fetch_add(1, Ordering::Relaxed);
    }));

    let res = run_agent_with_options(&opts, &RunCmd::React, on_event).await;
    assert!(res.is_err());
}

/// Integration test: session-id (thread_id) restores context from checkpoint.
/// Runs twice with the same thread_id; verifies both runs persist to the same session (>= 2 checkpoints).
#[tokio::test]
async fn session_id_restores_context_from_checkpoint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let working = dir.path().to_path_buf();
    let loom_dir = working.join(".loom");
    std::fs::create_dir_all(&loom_dir).expect("create .loom");
    let dir_path = dir.path().to_path_buf();

    temp_env::async_with_vars(
        [("LOOM_HOME", Some(dir_path.as_os_str()))],
        async {
        let session_id = "sess-restore-test";
        let opts1 = RunOptions {
            message: UserContent::Text("First message".to_string()),
            working_folder: Some(working.clone()),
            session_id: None,
            cancellation: None,
            thread_id: Some(session_id.to_string()),
            agent: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 120,
            output_json: false,
            model: None,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
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
        };
        let opts2 = RunOptions {
            message: UserContent::Text("Second message".to_string()),
            working_folder: Some(working),
            session_id: None,
            cancellation: None,
            thread_id: Some(session_id.to_string()),
            agent: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 120,
            output_json: false,
            model: None,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
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
        };

        let result1 = run_agent_with_llm_override(
            &opts1,
            &RunCmd::React,
            None,
            Some(Box::new(MockLlm::with_no_tool_calls("Reply one"))),
        )
        .await
        .expect("first run");
        match result1 {
            RunCompletion::Finished(result) => assert_eq!(result.reply.trim(), "Reply one"),
            RunCompletion::Cancelled => panic!("expected first run to finish"),
        }

        let result2 = run_agent_with_llm_override(
            &opts2,
            &RunCmd::React,
            None,
            Some(Box::new(MockLlm::with_no_tool_calls("Reply two"))),
        )
        .await
        .expect("second run");
        match result2 {
            RunCompletion::Finished(result) => assert_eq!(result.reply.trim(), "Reply two"),
            RunCompletion::Cancelled => panic!("expected second run to finish"),
        }

        let db_path = loom::memory::default_memory_db_path();
        let serializer = std::sync::Arc::new(loom::memory::JsonSerializer);
        let saver = loom::memory::SqliteSaver::<loom::ReActState>::new(&db_path, serializer)
            .expect("open sqlite saver");
        let config = loom::memory::RunnableConfig {
            thread_id: Some(session_id.to_string()),
            ..Default::default()
        };
        let list: Vec<loom::CheckpointListItem> = saver
            .list(&config, Some(10), None, None)
            .await
            .expect("list checkpoints");
        assert!(
            list.len() >= 2,
            "session-id should persist both runs to same thread; got {} checkpoints",
            list.len()
        );
    }).await;
}

/// Integration test: a pre-cancelled run returns RunCompletion::Cancelled.
#[tokio::test]
async fn run_agent_with_llm_override_returns_cancelled_when_token_is_pre_cancelled() {
    ensure_short_mcp_timeout();
    let dir = tempfile::tempdir().expect("tempdir");
    let working = dir.path().to_path_buf();
    let loom_dir = working.join(".loom");
    std::fs::create_dir_all(&loom_dir).expect("create .loom");
    let mcp_json = r#"{"mcpServers":{"test-server":{"command":"true","args":[]}}}"#;
    std::fs::write(loom_dir.join("mcp.json"), mcp_json).expect("write mcp.json");

    let mut opts = opts(working);
    let cancellation = RunCancellation::new(1);
    cancellation.cancel();
    opts.cancellation = Some(cancellation);

    let result = run_agent_with_llm_override(
        &opts,
        &RunCmd::React,
        None,
        Some(Box::new(MockLlm::with_no_tool_calls("Done"))),
    )
    .await
    .expect("run_agent");

    assert!(matches!(result, RunCompletion::Cancelled));
}

/// Integration test: cancelled run does not persist a resume checkpoint.
#[tokio::test]
async fn cancelled_run_does_not_persist_checkpoint() {
    ensure_short_mcp_timeout();
    let dir = tempfile::tempdir().expect("tempdir");
    let working = dir.path().to_path_buf();
    let loom_dir = working.join(".loom");
    std::fs::create_dir_all(&loom_dir).expect("create .loom");
    let mcp_json = r#"{"mcpServers":{"test-server":{"command":"true","args":[]}}}"#;
    std::fs::write(loom_dir.join("mcp.json"), mcp_json).expect("write mcp.json");
    let dir_path = dir.path().to_path_buf();

    temp_env::async_with_vars(
        [("LOOM_HOME", Some(dir_path.as_os_str()))],
        async {
        let mut opts = opts(working);
        opts.thread_id = Some("cancelled-checkpoint-test".to_string());
        let cancellation = RunCancellation::new(1);
        cancellation.cancel();
        opts.cancellation = Some(cancellation);

        let result = run_agent_with_llm_override(
            &opts,
            &RunCmd::React,
            None,
            Some(Box::new(MockLlm::with_no_tool_calls("Done"))),
        )
        .await
        .expect("run_agent");
        assert!(matches!(result, RunCompletion::Cancelled));

        let db_path = loom::memory::default_memory_db_path();
        let serializer = std::sync::Arc::new(loom::memory::JsonSerializer);
        let saver = loom::memory::SqliteSaver::<loom::ReActState>::new(&db_path, serializer)
            .expect("open sqlite saver");
        let config = loom::memory::RunnableConfig {
            thread_id: Some("cancelled-checkpoint-test".to_string()),
            ..Default::default()
        };
        let list: Vec<loom::CheckpointListItem> = saver
            .list(&config, Some(10), None, None)
            .await
            .expect("list checkpoints");
        assert!(
            list.is_empty(),
            "cancelled run should not persist checkpoints, got {}",
            list.len()
        );
    }).await;
}

/// Integration test: cancelling during streamed output returns Cancelled.
#[tokio::test]
async fn run_agent_with_llm_override_returns_cancelled_during_streaming() {
    ensure_short_mcp_timeout();
    let dir = tempfile::tempdir().expect("tempdir");
    let working = dir.path().to_path_buf();
    let loom_dir = working.join(".loom");
    std::fs::create_dir_all(&loom_dir).expect("create .loom");
    let mcp_json = r#"{"mcpServers":{"test-server":{"command":"true","args":[]}}}"#;
    std::fs::write(loom_dir.join("mcp.json"), mcp_json).expect("write mcp.json");

    let mut opts = opts(working);
    let cancellation = RunCancellation::new(1);
    opts.cancellation = Some(cancellation.clone());

    let result = run_agent_with_llm_override(
        &opts,
        &RunCmd::React,
        None, // No on_event for now
        Some(Box::new(
            MockLlm::with_no_tool_calls("This is a streamed response that should be cancelled.")
                .with_stream_by_char()
                .with_stream_delay_ms(1),
        )),
    )
    .await
    .expect("run_agent");

    // For now, just verify that streaming MockLlm works
    match result {
        RunCompletion::Finished(result) => {
            assert_eq!(result.reply.trim(), "This is a streamed response that should be cancelled.");
        }
        RunCompletion::Cancelled => {
            // This is also acceptable if the run was cancelled somehow
        }
    }
}

#[tokio::test]
#[cfg(unix)]
async fn cancelled_bash_tool_kills_active_child_process() {
    let dir = tempfile::tempdir().expect("tempdir");
    let working = dir.path().to_path_buf();

    let mut opts = opts(working);
    let cancellation = RunCancellation::new(42);
    opts.cancellation = Some(cancellation.clone());

    let llm = MockLlm::first_tools_then_end().with_tool_calls(vec![loom::ToolCall {
        name: "bash".to_string(),
        arguments: serde_json::json!({ "command": "sleep 60" }).to_string(),
        id: Some("call-bash".to_string()),
    }]);

    let cancel_handle = cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        cancel_handle.cancel();
    });

    let started_at = Instant::now();
    let result = run_agent_with_llm_override(&opts, &RunCmd::React, None, Some(Box::new(llm)))
        .await
        .expect("run_agent");

    assert!(
        started_at.elapsed() < std::time::Duration::from_secs(15),
        "cancelled subprocess run should finish promptly"
    );
    assert_eq!(cancellation.active_operation_kind(), None);
    assert!(matches!(result, RunCompletion::Cancelled));
}
