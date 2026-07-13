//! End-to-end test: prove `build_react_config` registers the workflow tool
//! AND its `workflow` builtin skill before returning the SkillRegistry.
//!
//! This is the path the CLI, ACP, and telegram-bot front-ends actually use.
//! It catches the bug where `register_extra_tools` was called AFTER
//! `build_react_config` returned, silently dropping the workflow tool's
//! `workflow` builtin skill from the agent's SkillRegistry.

use std::path::PathBuf;
use std::sync::Arc;

use agent::run::build_react_config;
use agent::run::RunOptions;
use loom_llm::message::UserContent;
use tool_core::Tool;
use tool_workflow::default_workflow_tool_provider;

fn make_run_options(
    working: PathBuf,
    provider: Option<agent::run::ExtraToolsProvider>,
) -> RunOptions {
    RunOptions {
        message: UserContent::Text(String::new()),
        working_folder: Some(working),
        session_id: None,
        cancellation: None,
        thread_id: Some("test-thread".to_string()),
        agent: None,
        verbose: false,
        verbose_level: 0,
        got_adaptive: false,
        display_max_len: 4096,
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
        default_extra_tools_provider: provider,
        acp_session_id: None,
        force_compact: false,
        chat_id: None,
        worktree: false,
        goal_mode: false,
        acp_mcp_servers: None,
        effort: None,
        tier: None,
    }
}

#[test]
fn build_react_config_with_provider_registers_workflow() {
    let dir = tempfile::tempdir().expect("tempdir");
    let opts = make_run_options(dir.path().to_path_buf(), Some(default_workflow_tool_provider()));

    let (_config, _resolved, skill_registry) = build_react_config(&opts);

    let registry = skill_registry
        .as_ref()
        .expect("skill_registry should be Some when provider is set");
    let names: Vec<String> = registry
        .list()
        .iter()
        .map(|e| e.metadata.name.clone())
        .collect();
    assert!(
        names.contains(&"workflow".to_string()),
        "registry should contain workflow builtin skill when provider is set, got: {:?}",
        names
    );
}

#[test]
fn build_react_config_provider_pushes_workflow_tool_into_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let opts = make_run_options(dir.path().to_path_buf(), Some(default_workflow_tool_provider()));

    let (config, _resolved, _registry) = build_react_config(&opts);

    let extra: &Arc<Vec<Arc<dyn Tool>>> = config
        .extra_tools
        .as_ref()
        .expect("config.extra_tools should be populated by default_extra_tools_provider");
    let names: Vec<&str> = extra.iter().map(|t| t.name()).collect();
    assert!(
        names.contains(&"workflow"),
        "config.extra_tools should contain the workflow tool from the provider, got: {:?}",
        names
    );
}
