//! Integration tests for BatchTool: spec, valid calls, invalid input, parallel execution.

mod init_logging;

use loom::tool_source::{ToolSource, ToolSourceError};
use loom::tools::{
    BatchTool, ReadFileTool, Tool, AggregateToolSource, TOOL_BATCH,
};
use serde_json::json;
use std::sync::Arc;

fn aggregate_with_read_and_batch(
    dir: &tempfile::TempDir,
) -> Arc<AggregateToolSource> {
    let wf = Arc::new(dir.path().canonicalize().unwrap());
    let agg = Arc::new(AggregateToolSource::new());
    agg.register_sync(Box::new(ReadFileTool::new(wf)));
    agg.register_sync(Box::new(BatchTool::new(Arc::clone(&agg))));
    agg
}

#[tokio::test]
async fn batch_tool_name_and_spec() {
    let tool = BatchTool::new(Arc::new(AggregateToolSource::new()));
    assert_eq!(tool.name(), TOOL_BATCH);
    let spec = tool.spec();
    assert_eq!(spec.name, TOOL_BATCH);
    assert!(spec.description.as_ref().unwrap().contains("parallel"));
    assert_eq!(spec.input_schema["required"], json!(["calls"]));
    let props = &spec.input_schema["properties"]["calls"];
    assert_eq!(props["minItems"], 1);
    assert_eq!(props["maxItems"], 25);
}

#[tokio::test]
async fn batch_tool_single_call() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "hello").unwrap();
    let agg = aggregate_with_read_and_batch(&dir);
    let result = agg
        .call_tool(
            TOOL_BATCH,
            json!({
                "calls": [{ "tool": "read", "parameters": { "path": "f.txt" } }]
            }),
        )
        .await
        .unwrap();
    assert!(result.text.contains("hello"));
    assert!(result.text.contains("read"));
}

#[tokio::test]
async fn batch_tool_two_calls_parallel() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "content_a").unwrap();
    std::fs::write(dir.path().join("b.txt"), "content_b").unwrap();
    let agg = aggregate_with_read_and_batch(&dir);
    let result = agg
        .call_tool(
            TOOL_BATCH,
            json!({
                "calls": [
                    { "tool": "read", "parameters": { "path": "a.txt" } },
                    { "tool": "read", "parameters": { "path": "b.txt" } }
                ]
            }),
        )
        .await
        .unwrap();
    assert!(result.text.contains("content_a"));
    assert!(result.text.contains("content_b"));
    assert!(result.text.contains("[1] read"));
    assert!(result.text.contains("[2] read"));
}

#[tokio::test]
async fn batch_tool_missing_calls_returns_error() {
    let agg = Arc::new(AggregateToolSource::new());
    agg.register_sync(Box::new(BatchTool::new(Arc::clone(&agg))));
    let result = agg.call_tool(TOOL_BATCH, json!({})).await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
    assert!(err.to_string().to_lowercase().contains("calls"));
}

#[tokio::test]
async fn batch_tool_empty_calls_returns_error() {
    let agg = Arc::new(AggregateToolSource::new());
    agg.register_sync(Box::new(BatchTool::new(Arc::clone(&agg))));
    let result = agg
        .call_tool(TOOL_BATCH, json!({ "calls": [] }))
        .await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

#[tokio::test]
async fn batch_tool_call_missing_tool_name_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let agg = aggregate_with_read_and_batch(&dir);
    let result = agg
        .call_tool(
            TOOL_BATCH,
            json!({ "calls": [{ "parameters": {} }] }),
        )
        .await;
    let err = result.unwrap_err();
    assert!(matches!(err, ToolSourceError::InvalidInput(_)));
}

#[tokio::test]
async fn batch_tool_unknown_tool_returns_error_in_result() {
    let dir = tempfile::tempdir().unwrap();
    let agg = aggregate_with_read_and_batch(&dir);
    let result = agg
        .call_tool(
            TOOL_BATCH,
            json!({
                "calls": [{ "tool": "nonexistent_tool", "parameters": {} }]
            }),
        )
        .await
        .unwrap();
    assert!(result.text.to_lowercase().contains("error"));
}
