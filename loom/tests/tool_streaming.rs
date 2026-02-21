//! Tests for tool custom streaming functionality.
//!
//! These tests verify that tools can emit custom streaming events during execution
//! via `ToolCallContext.stream_writer` or `ToolCallContext.emit_custom()`.

mod init_logging;

use async_trait::async_trait;
use loom::stream::{StreamEvent, ToolStreamWriter};
use loom::tool_source::{ToolCallContent, ToolCallContext, ToolSource, ToolSourceError, ToolSpec};
use loom::tools::{AggregateToolSource, Tool};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

// ============================================================================
// Test Tool Implementation
// ============================================================================

/// A test tool that emits custom streaming events during execution.
struct StreamingTool {
    /// Number of progress events to emit during execution.
    progress_count: usize,
}

impl StreamingTool {
    fn new(progress_count: usize) -> Self {
        Self { progress_count }
    }
}

#[async_trait]
impl Tool for StreamingTool {
    fn name(&self) -> &str {
        "streaming_tool"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "streaming_tool".to_string(),
            description: Some("A tool that emits streaming events".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn call(
        &self,
        _args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        // Emit progress events if stream writer is available
        if let Some(ctx) = ctx {
            // Emit start event
            ctx.emit_custom(json!({"phase": "start"}));

            // Emit progress events
            for i in 1..=self.progress_count {
                let progress = (i * 100) / self.progress_count;
                ctx.emit_custom(json!({
                    "phase": "progress",
                    "step": i,
                    "percent": progress
                }));
            }

            // Emit end event
            ctx.emit_custom(json!({"phase": "done"}));
        }

        Ok(ToolCallContent {
            text: format!("Completed {} steps", self.progress_count),
        })
    }
}

// ============================================================================
// Tests for ToolCallContext Streaming
// ============================================================================

/// **Scenario**: ToolCallContext with stream_writer can emit custom events.
#[tokio::test]
async fn tool_call_context_emit_custom_sends_events() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let events_clone = events.clone();

    // Create a ToolStreamWriter that captures events
    let writer = ToolStreamWriter::new(move |value| {
        events_clone.lock().unwrap().push(value);
        true
    });

    let ctx = ToolCallContext::with_stream_writer(vec![], writer);

    // Emit events
    assert!(ctx.emit_custom(json!({"a": 1})));
    assert!(ctx.emit_custom(json!({"b": 2})));

    // Verify events were captured
    let captured = events.lock().unwrap();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0]["a"], 1);
    assert_eq!(captured[1]["b"], 2);
}

/// **Scenario**: ToolCallContext without stream_writer returns false on emit.
#[tokio::test]
async fn tool_call_context_without_writer_returns_false() {
    let ctx = ToolCallContext::new(vec![]);

    // emit_custom should return false when no writer is available
    assert!(!ctx.emit_custom(json!({"test": true})));
}

/// **Scenario**: StreamingTool emits progress events via ToolCallContext.
#[tokio::test]
async fn streaming_tool_emits_progress_events() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let events_clone = events.clone();

    // Create a ToolStreamWriter that captures events
    let writer = ToolStreamWriter::new(move |value| {
        events_clone.lock().unwrap().push(value);
        true
    });

    let ctx = ToolCallContext::with_stream_writer(vec![], writer);
    let tool = StreamingTool::new(5);

    // Call the tool
    let result = tool.call(json!({}), Some(&ctx)).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().text, "Completed 5 steps");

    // Verify all events were captured
    let captured = events.lock().unwrap();

    // Should have: 1 start + 5 progress + 1 done = 7 events
    assert_eq!(
        captured.len(),
        7,
        "Expected 7 events, got {}",
        captured.len()
    );

    // Verify start event
    assert_eq!(captured[0]["phase"], "start");

    // Verify progress events
    for i in 0..5 {
        assert_eq!(captured[i + 1]["phase"], "progress");
        assert_eq!(captured[i + 1]["step"], i + 1);
    }

    // Verify done event
    assert_eq!(captured[6]["phase"], "done");
}

/// **Scenario**: StreamingTool works without ToolCallContext (backward compatibility).
#[tokio::test]
async fn streaming_tool_works_without_context() {
    let tool = StreamingTool::new(3);

    // Call without context - should work without emitting events
    let result = tool.call(json!({}), None).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().text, "Completed 3 steps");
}

/// **Scenario**: StreamingTool works with context but no stream_writer.
#[tokio::test]
async fn streaming_tool_works_with_context_no_writer() {
    let ctx = ToolCallContext::new(vec![]);
    let tool = StreamingTool::new(3);

    // Call with context but no stream writer - should work without emitting events
    let result = tool.call(json!({}), Some(&ctx)).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().text, "Completed 3 steps");
}

/// **Scenario**: AggregateToolSource passes context to registered tools.
#[tokio::test]
async fn aggregate_tool_source_passes_context_to_tools() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let events_clone = events.clone();

    // Create a ToolStreamWriter that captures events
    let writer = ToolStreamWriter::new(move |value| {
        events_clone.lock().unwrap().push(value);
        true
    });

    let ctx = ToolCallContext::with_stream_writer(vec![], writer);

    // Register the streaming tool
    let source = AggregateToolSource::new();
    source.register_sync(Box::new(StreamingTool::new(2)));

    // Call through AggregateToolSource with context
    let result = source
        .call_tool_with_context("streaming_tool", json!({}), Some(&ctx))
        .await;
    assert!(result.is_ok());

    // Verify events were captured
    let captured = events.lock().unwrap();
    assert_eq!(
        captured.len(),
        4,
        "Expected 4 events: start + 2 progress + done"
    );
}

// ============================================================================
// Integration with StreamEvent<S>
// ============================================================================

/// **Scenario**: ToolStreamWriter can forward events to a channel of StreamEvent<S>.
#[tokio::test]
async fn tool_stream_writer_forwards_to_stream_event_channel() {
    #[derive(Clone, Debug)]
    struct TestState(i32);

    let (tx, mut rx) = mpsc::channel::<StreamEvent<TestState>>(16);

    // Create a ToolStreamWriter that sends to the channel
    let writer =
        ToolStreamWriter::new(move |value| tx.try_send(StreamEvent::Custom(value)).is_ok());

    let ctx = ToolCallContext::with_stream_writer(vec![], writer);
    let tool = StreamingTool::new(2);

    // Call the tool
    let result = tool.call(json!({}), Some(&ctx)).await;
    assert!(result.is_ok());

    // Collect events from channel
    let mut events = vec![];
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }

    // Verify events
    assert_eq!(events.len(), 4, "Expected 4 StreamEvent::Custom events");

    for event in &events {
        match event {
            StreamEvent::Custom(v) => {
                assert!(
                    v.get("phase").is_some(),
                    "Each event should have 'phase' field"
                );
            }
            _ => panic!("Expected StreamEvent::Custom"),
        }
    }
}

/// **Scenario**: Multiple tools can share the same ToolStreamWriter.
#[tokio::test]
async fn multiple_tools_share_stream_writer() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    let writer = ToolStreamWriter::new(move |_value| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        true
    });

    // Create context with shared writer
    let ctx = ToolCallContext::with_stream_writer(vec![], writer);

    // Create multiple tools
    let tool1 = StreamingTool::new(3); // 1 start + 3 progress + 1 done = 5 events
    let tool2 = StreamingTool::new(2); // 1 start + 2 progress + 1 done = 4 events

    // Call both tools with same context
    let _ = tool1.call(json!({}), Some(&ctx)).await;
    let _ = tool2.call(json!({}), Some(&ctx)).await;

    // Total should be 9 events
    assert_eq!(counter.load(Ordering::SeqCst), 9);
}
