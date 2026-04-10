#[cfg(test)]
mod tests {
    use crate::stream::{StreamWriter, ToolStreamWriter};
    use serde_json;

    #[derive(Clone, Debug, PartialEq)]
    struct DummyState(i32);

    /// **Scenario**: ToolStreamWriter integration test.
    #[tokio::test]
    async fn test_tool_stream_writer_integration() {
        use serde_json::json;
        use std::sync::{Arc, Mutex};

        // Test ToolStreamWriter integration with emit functions
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();

        let emit_fn = move |value: serde_json::Value| {
            events_clone.lock().unwrap().push(value);
            true
        };

        let writer = ToolStreamWriter::new(emit_fn);

        // Emit custom events
        assert!(writer.emit_custom(json!({"status": "starting"})));
        assert!(writer.emit_custom(json!({"progress": 50})));
        assert!(writer.emit_custom(json!({"status": "done"})));

        // Verify events were collected
        assert_eq!(events.lock().unwrap().len(), 3);
        assert_eq!(events.lock().unwrap()[0], json!({"status": "starting"}));
        assert_eq!(events.lock().unwrap()[1], json!({"progress": 50}));
        assert_eq!(events.lock().unwrap()[2], json!({"status": "done"}));
    }

    #[tokio::test]
    async fn test_writer_noop_functionality() {
        // Test that no-op writers return false for all operations
        let writer = StreamWriter::<DummyState>::noop();
        let tool_writer = ToolStreamWriter::noop();

        assert!(!writer.emit_custom(serde_json::json!({"test": "value"})).await);
        assert!(!writer.emit_message("test", "node").await);
        assert!(!writer.emit_values(DummyState(1)).await);

        assert!(!tool_writer.emit_custom(serde_json::json!({"test": "value"})));
        assert!(!tool_writer.emit_output("test output"));
    }
}