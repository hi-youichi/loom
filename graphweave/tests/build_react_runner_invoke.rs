//! Integration test: build_react_runner then invoke (config → runner → one invoke).
//!
//! Phase 0 refactoring: ensures the full pipeline from ReactBuildConfig through
//! build_react_runner and ReactRunner::invoke is covered by a test.

mod init_logging;

use graphweave::{
    build_react_runner, MockLlm, ReactBuildConfig, ReactRunner,
    TotRunnerConfig, GotRunnerConfig,
};

fn minimal_config() -> ReactBuildConfig {
    ReactBuildConfig {
        db_path: None,
        thread_id: None,
        user_id: None,
        system_prompt: None,
        exa_api_key: None,
        twitter_api_key: None,
        mcp_exa_url: "https://mcp.exa.ai/mcp".to_string(),
        mcp_remote_cmd: "npx".to_string(),
        mcp_remote_args: "-y mcp-remote".to_string(),
        mcp_verbose: false,
        openai_api_key: None,
        openai_base_url: None,
        model: None,
        embedding_api_key: None,
        embedding_base_url: None,
        embedding_model: None,
        working_folder: None,
        approval_policy: None,
        compaction_config: None,
        tot_config: TotRunnerConfig::default(),
        got_config: GotRunnerConfig::default(),
    }
}

/// Scenario: build_react_runner with minimal config and MockLlm (no tool calls),
/// then invoke once; graph goes think → END and final state contains assistant message.
#[tokio::test]
async fn build_react_runner_then_invoke_one_turn() {
    let config = minimal_config();
    let llm = Box::new(MockLlm::with_no_tool_calls("Hello from mock."));
    let runner: ReactRunner = build_react_runner(&config, Some(llm), false, None)
        .await
        .expect("build_react_runner");
    let state = runner
        .invoke("Hi")
        .await
        .expect("invoke");
    let last = state.last_assistant_reply();
    let ok = last
        .as_ref()
        .map(|s| s.contains("Hello from mock."))
        .unwrap_or(false);
    assert!(
        ok,
        "expected assistant reply to contain mock content, got {:?}",
        last
    );
}
