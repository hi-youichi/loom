//! Integration tests for ApplyPatchTool: Add file, Update file, Delete file.

mod init_logging;

use graphweave::tool_source::{ToolSource, ToolSourceError};
use graphweave::tools::{AggregateToolSource, TOOL_APPLY_PATCH};
use graphweave::tool_source::register_file_tools;
use serde_json::json;

fn aggregate_with_file_tools(dir: &tempfile::TempDir) -> AggregateToolSource {
    let agg = AggregateToolSource::new();
    register_file_tools(&agg, dir.path()).unwrap();
    agg
}

#[tokio::test]
async fn apply_patch_add_file() {
    let dir = tempfile::tempdir().unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let patch = r#"*** Begin Patch
*** Add File: hello.txt
+line one
+line two
*** End Patch"#;
    let result = agg
        .call_tool(TOOL_APPLY_PATCH, json!({ "patchText": patch }))
        .await
        .unwrap();
    assert!(result.text.contains("Applied"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("hello.txt")).unwrap(),
        "line one\nline two"
    );
}

#[tokio::test]
async fn apply_patch_update_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "old\nkeep\nend").unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let patch = r#"*** Begin Patch
*** Update File: f.txt
@@
-old
+NEW
*** End of File
*** End Patch"#;
    let result = agg
        .call_tool(TOOL_APPLY_PATCH, json!({ "patchText": patch }))
        .await
        .unwrap();
    assert!(result.text.contains("Applied"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "NEW\nkeep\nend"
    );
}

#[tokio::test]
async fn apply_patch_delete_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("gone.txt"), "content").unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let patch = r#"*** Begin Patch
*** Delete File: gone.txt
*** End Patch"#;
    let result = agg
        .call_tool(TOOL_APPLY_PATCH, json!({ "patchText": patch }))
        .await
        .unwrap();
    assert!(result.text.contains("Applied"));
    assert!(!dir.path().join("gone.txt").exists());
}

#[tokio::test]
async fn apply_patch_missing_patch_text_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let result = agg
        .call_tool(TOOL_APPLY_PATCH, json!({}))
        .await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

#[tokio::test]
async fn apply_patch_missing_begin_end_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let result = agg
        .call_tool(
            TOOL_APPLY_PATCH,
            json!({ "patchText": "*** Add File: x.txt\n+content" }),
        )
        .await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

#[tokio::test]
async fn apply_patch_path_outside_working_folder_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let agg = aggregate_with_file_tools(&dir);
    let patch = r#"*** Begin Patch
*** Add File: ../../../etc/escape.txt
+content
*** End Patch"#;
    let result = agg
        .call_tool(TOOL_APPLY_PATCH, json!({ "patchText": patch }))
        .await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}
