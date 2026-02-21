//! Integration tests for MultieditTool: new file, multiple edits in order, atomicity.

mod init_logging;

use loom::tool_source::register_file_tools;
use loom::tool_source::{ToolSource, ToolSourceError};
use loom::tools::{AggregateToolSource, TOOL_MULTIEDIT};
use serde_json::json;

fn aggregate_with_file_tools(dir: &tempfile::TempDir) -> AggregateToolSource {
    let agg = AggregateToolSource::new();
    register_file_tools(&agg, dir.path()).unwrap();
    agg
}

#[tokio::test]
async fn multiedit_new_file_with_single_edit() {
    let dir = tempfile::tempdir().unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let result = agg
        .call_tool(
            TOOL_MULTIEDIT,
            json!({
                "path": "new.txt",
                "edits": [{ "oldString": "", "newString": "first line" }]
            }),
        )
        .await
        .unwrap();
    assert!(result.text.contains("Created"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("new.txt")).unwrap(),
        "first line"
    );
}

#[tokio::test]
async fn multiedit_existing_file_multiple_edits() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "a\nb\nc").unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let result = agg
        .call_tool(
            TOOL_MULTIEDIT,
            json!({
                "path": "f.txt",
                "edits": [
                    { "oldString": "a", "newString": "A" },
                    { "oldString": "c", "newString": "C" }
                ]
            }),
        )
        .await
        .unwrap();
    assert!(result.text.contains("Applied"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "A\nb\nC"
    );
}

#[tokio::test]
async fn multiedit_missing_path_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let result = agg
        .call_tool(
            TOOL_MULTIEDIT,
            json!({ "edits": [{ "oldString": "", "newString": "x" }] }),
        )
        .await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

#[tokio::test]
async fn multiedit_empty_edits_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "x").unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let result = agg
        .call_tool(TOOL_MULTIEDIT, json!({ "path": "f.txt", "edits": [] }))
        .await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

#[tokio::test]
async fn multiedit_path_outside_working_folder_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let result = agg
        .call_tool(
            TOOL_MULTIEDIT,
            json!({
                "path": "../outside.txt",
                "edits": [{ "oldString": "", "newString": "x" }]
            }),
        )
        .await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

#[tokio::test]
async fn multiedit_old_string_not_found_returns_error_no_write() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "original").unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let result = agg
        .call_tool(
            TOOL_MULTIEDIT,
            json!({
                "path": "f.txt",
                "edits": [{ "oldString": "not_in_file", "newString": "y" }]
            }),
        )
        .await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "original"
    );
}
