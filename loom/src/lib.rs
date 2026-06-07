//! # Loom
//!
//! Run orchestration and shared types for building stateful AI agents.
//!
//! This crate provides the `cli_run` module which contains profile loading,
//! helve config building, and model resolution logic.
//!
//! All core types are now in their own crates — import them directly:
//!
//! - `loom_graph` — `StateGraph`, `CompiledStateGraph`, `Node`, `Next`, `Agent`, `channels`, `managed`
//! - `loom_llm` — `LlmClient`, `ChatOpenAI`, `MockLlm`, `Message`, `ToolCall`, etc.
//! - `loom_types` — `ReActState`, `ToolResult`, `ToolCall`, `ModelConfig`, approval types
//! - `loom_tools` — `ToolSource`, `ToolSpec`, `McpToolSource`, `BashTool`, etc.
//! - `loom_memory` — `Checkpointer`, `Store`, `MemorySaver`, `SqliteSaver`
//! - `loom_stream` — `StreamEvent`, `StreamMode`, `StreamWriter`
//! - `loom_helve` — `HelveConfig`, `assemble_system_prompt`, approval policy
//! - `loom_tier` — tier resolution, model registry, LLM factory
//! - `loom_cache` — `Cache`, `InMemoryCache`
//! - `loom_compress` — context compression / compaction
//! - `loom_protocol` — WebSocket message types, streaming protocol
//! - `loom_pregel` — low-level Pregel graph runtime
//! - `loom_commands` — slash command parsing and execution
//! - `loom_model_spec` — `ModelSpec`, resolvers
//! - `loom_background_review` — background review system
//! - `loom_stream_display` — stream event display/rendering
//! - `loom_worktree` — git worktree isolation
//! - `loom_lsp` — LSP integration
//! - `loom_cli_types` — `RunOptions`, `RunCmd`, `RunCompletion`, `AnyStreamEvent`
//! - `loom_react_config` — `ReactBuildConfig`, profile types

pub mod cli_run;

/// Global lock for tests that modify `LOOM_HOME` or `OPENAI_BASE_URL` env vars.
/// Use in any test that sets/removes these env vars to prevent data races.
#[cfg(test)]
pub fn env_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

/// When running `cargo test -p loom`, initializes tracing from `RUST_LOG` so that
/// unit tests in `src/**` (e.g. `openai.rs` `mod tests`) can print logs with `--nocapture`.
#[cfg(test)]
mod test_logging {
    use ctor::ctor;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::Layer;

    #[ctor]
    fn init() {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
        let _ = tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_test_writer()
                    .with_filter(filter),
            )
            .try_init();
    }
}

#[cfg(test)]
mod run_agent_options_tests {
    use std::sync::OnceLock;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[cfg(unix)]
    use std::time::Instant;

    use loom_llm::client::MockLlm;
    use loom_cli_types::{RunCancellation, RunCmd, RunCompletion, RunOptions, AnyStreamEvent};
    use loom_types::state::ReActState;
    use loom_llm::message::UserContent;
    #[cfg(unix)]
    use loom_llm::ToolCall;
    use loom_memory::{default_memory_db_path, JsonSerializer, SqliteSaver, RunnableConfig, CheckpointListItem, Checkpointer};

    use loom_agent::{run_agent_with_llm_override, run_agent_with_options};

    static MCP_SHORT_TIMEOUT: OnceLock<()> = OnceLock::new();

    fn ensure_short_mcp_timeout() {
        MCP_SHORT_TIMEOUT.get_or_init(|| {
            std::env::set_var("LOOM_MCP_INIT_TIMEOUT_SECS", "1");
        });
    }

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

    #[tokio::test(flavor = "current_thread")]
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

    #[tokio::test(flavor = "current_thread")]
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

    #[tokio::test(flavor = "current_thread")]
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
                if let AnyStreamEvent::React(loom_stream::StreamEvent::Updates { state, .. }) = &ev {
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

    #[tokio::test(flavor = "current_thread")]
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

    #[tokio::test(flavor = "current_thread")]
    async fn session_id_restores_context_from_checkpoint() {
        let _lock = crate::env_test_lock().lock().unwrap();
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

            let db_path = default_memory_db_path();
            let serializer = std::sync::Arc::new(JsonSerializer);
            let saver = SqliteSaver::<ReActState>::new(&db_path, serializer)
                .expect("open sqlite saver");
            let config = RunnableConfig {
                thread_id: Some(session_id.to_string()),
                ..Default::default()
            };
            let list: Vec<CheckpointListItem> = saver
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

    #[tokio::test(flavor = "current_thread")]
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

    #[tokio::test(flavor = "current_thread")]
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

            let db_path = default_memory_db_path();
            let serializer = std::sync::Arc::new(JsonSerializer);
            let saver = SqliteSaver::<ReActState>::new(&db_path, serializer)
                .expect("open sqlite saver");
            let config = RunnableConfig {
                thread_id: Some("cancelled-checkpoint-test".to_string()),
                ..Default::default()
            };
            let list: Vec<CheckpointListItem> = saver
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

    #[tokio::test(flavor = "current_thread")]
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
            None,
            Some(Box::new(
                MockLlm::with_no_tool_calls("This is a streamed response that should be cancelled.")
                    .with_stream_by_char()
                    .with_stream_delay_ms(1),
            )),
        )
        .await
        .expect("run_agent");

        match result {
            RunCompletion::Finished(result) => {
                assert_eq!(result.reply.trim(), "This is a streamed response that should be cancelled.");
            }
            RunCompletion::Cancelled => {
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    #[cfg(unix)]
    async fn cancelled_bash_tool_kills_active_child_process() {
        let dir = tempfile::tempdir().expect("tempdir");
        let working = dir.path().to_path_buf();

        let mut opts = opts(working);
        let cancellation = RunCancellation::new(42);
        opts.cancellation = Some(cancellation.clone());

        let llm = MockLlm::first_tools_then_end().with_tool_calls(vec![ToolCall {
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
            started_at.elapsed() < std::time::Duration::from_secs(30),
            "cancelled subprocess run should finish promptly"
        );
        assert_eq!(cancellation.active_operation_kind(), None);
        assert!(matches!(result, RunCompletion::Cancelled));
    }
}
