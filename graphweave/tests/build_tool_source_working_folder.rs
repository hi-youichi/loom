//! Integration test: build_react_run_context with working_folder set includes file tools.
//!
//! Scenario: when ReactBuildConfig has working_folder set, the built tool source
//! lists ls, read_file, write_file, move_file, delete_file, create_dir.

mod init_logging;

use graphweave::tools::{
    TOOL_CREATE_DIR, TOOL_DELETE_FILE, TOOL_LS, TOOL_MOVE_FILE, TOOL_READ_FILE,
    TOOL_WRITE_FILE,
};
use graphweave::{build_react_run_context, ReactBuildConfig};

/// Scenario: building run context with working_folder set yields tool source that includes file tools.
#[tokio::test]
async fn build_tool_source_with_working_folder_includes_file_tools() {
    let dir = tempfile::tempdir().unwrap();
    let config = ReactBuildConfig {
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
        working_folder: Some(dir.path().to_path_buf()),
        approval_policy: None,
        compaction_config: None,
        tot_config: graphweave::TotRunnerConfig::default(),
        got_config: graphweave::GotRunnerConfig::default(),
    };
    let ctx = build_react_run_context(&config).await.unwrap();
    let tools = ctx.tool_source.list_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&TOOL_LS),
        "expected ls in {:?}",
        names
    );
    assert!(names.contains(&TOOL_READ_FILE));
    assert!(names.contains(&TOOL_WRITE_FILE));
    assert!(names.contains(&TOOL_MOVE_FILE));
    assert!(names.contains(&TOOL_DELETE_FILE));
    assert!(names.contains(&TOOL_CREATE_DIR));
}
