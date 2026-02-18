//! Unit tests for ReadOnlyDirToolSource and aggregate with FileToolSource.
//!
//! Scenarios: ReadOnlyDirToolSource lists read_only_list_dir and read_only_read;
//! call_tool works for both; register_read_only_dir_tools on aggregate with register_file_tools
//! yields both writable file tools and read-only connector tools (task 9).

mod init_logging;

use graphweave::tool_source::{
    register_file_tools, register_read_only_dir_tools, ToolSource, ToolSourceError,
    TOOL_READ_ONLY_LIST_DIR, TOOL_READ_ONLY_READ_FILE,
};
use graphweave::tools::{AggregateToolSource, TOOL_LS, TOOL_READ_FILE, TOOL_WRITE_FILE};
use serde_json::json;

/// Scenario: ReadOnlyDirToolSource::new with a valid directory lists read_only_list_dir and read_only_read.
#[tokio::test]
async fn read_only_dir_tool_source_list_tools() {
    let dir = tempfile::tempdir().unwrap();
    let source = graphweave::tool_source::ReadOnlyDirToolSource::new(dir.path()).unwrap();
    let tools = source.list_tools().await.unwrap();
    assert_eq!(tools.len(), 2);
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&TOOL_READ_ONLY_LIST_DIR));
    assert!(names.contains(&TOOL_READ_ONLY_READ_FILE));
}

/// Scenario: read_only_list_dir and read_only_read work under the read-only root.
#[tokio::test]
async fn read_only_dir_tool_source_list_and_read() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ref.txt"), "reference content").unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    let source = graphweave::tool_source::ReadOnlyDirToolSource::new(dir.path()).unwrap();

    let list = source
        .call_tool(TOOL_READ_ONLY_LIST_DIR, json!({ "path": "." }))
        .await
        .unwrap();
    assert!(list.text.contains("ref.txt"));
    assert!(list.text.contains("docs"));

    let read = source
        .call_tool(TOOL_READ_ONLY_READ_FILE, json!({ "path": "ref.txt" }))
        .await
        .unwrap();
    assert_eq!(read.text, "reference content");
}

/// Scenario: register_read_only_dir_tools with non-directory or missing path returns InvalidInput.
#[tokio::test]
async fn read_only_dir_tool_source_invalid_root_errors() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("file.txt");
    std::fs::write(&file_path, "x").unwrap();
    let aggregate = AggregateToolSource::new();
    let err = register_read_only_dir_tools(&aggregate, &file_path).unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    assert!(err.to_string().to_lowercase().contains("directory"));

    let err = register_read_only_dir_tools(&aggregate, "/nonexistent/path").unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

/// Scenario: Aggregate with register_file_tools (writable) and register_read_only_dir_tools (read-only root)
/// lists both sets; writable tools work on working folder and read-only tools on the other root.
#[tokio::test]
async fn aggregate_file_tools_and_read_only_dir_tools() {
    let working = tempfile::tempdir().unwrap();
    let read_only_root = tempfile::tempdir().unwrap();
    std::fs::write(
        read_only_root.path().join("readme.txt"),
        "read-only content",
    )
    .unwrap();

    let aggregate = AggregateToolSource::new();
    register_file_tools(&aggregate, working.path()).unwrap();
    register_read_only_dir_tools(&aggregate, read_only_root.path()).unwrap();

    let tools = aggregate.list_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&TOOL_LS));
    assert!(names.contains(&TOOL_READ_FILE));
    assert!(names.contains(&TOOL_WRITE_FILE));
    assert!(names.contains(&TOOL_READ_ONLY_LIST_DIR));
    assert!(names.contains(&TOOL_READ_ONLY_READ_FILE));

    // Writable: write in working folder
    aggregate
        .call_tool(
            TOOL_WRITE_FILE,
            json!({ "path": "writable.txt", "content": "written" }),
        )
        .await
        .unwrap();
    let w = aggregate
        .call_tool(TOOL_READ_FILE, json!({ "path": "writable.txt" }))
        .await
        .unwrap();
    assert_eq!(w.text, "written");

    // Read-only connector: read from read_only_root
    let ro = aggregate
        .call_tool(TOOL_READ_ONLY_READ_FILE, json!({ "path": "readme.txt" }))
        .await
        .unwrap();
    assert_eq!(ro.text, "read-only content");
}
