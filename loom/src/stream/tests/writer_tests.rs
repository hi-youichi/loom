#[cfg(test)]
mod tests {
    use crate::stream::{StreamEvent, StreamMode, StreamWriter};
    use serde_json;
    use std::collections::HashSet;
    use tokio::sync::mpsc;

    #[derive(Clone, Debug, PartialEq)]
    struct DummyState(i32);

    /// **Scenario**: StreamWriter respects stream mode settings when emitting events.
    #[tokio::test]
    async fn stream_writer_emit_custom_respects_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // Without Custom mode - should not send
        let modes_without_custom = HashSet::from_iter([StreamMode::Values]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_without_custom);
        let sent = writer.emit_custom(serde_json::json!({"test": "value"})).await;
        assert!(!sent, "should not send when Custom mode is disabled");

        // With Custom mode - should send
        let modes_with_custom = HashSet::from_iter([StreamMode::Custom]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_with_custom);
        let sent = writer.emit_custom(serde_json::json!({"test": "value"})).await;
        assert!(sent, "should send when Custom mode is enabled");

        // Verify the event was received
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::Custom(value) => {
                assert_eq!(value, serde_json::json!({"test": "value"}));
            }
            _ => panic!("expected Custom event"),
        }
    }

    /// **Scenario**: StreamWriter::emit_message only sends when Messages mode is enabled.
    #[tokio::test]
    async fn stream_writer_emit_message_respects_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // Without Messages mode - should not send
        let modes_without_messages = HashSet::from_iter([StreamMode::Values]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_without_messages);
        let sent = writer.emit_message("hello", "node1").await;
        assert!(!sent, "should not send when Messages mode is disabled");

        // With Messages mode - should send
        let modes_with_messages = HashSet::from_iter([StreamMode::Messages]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_with_messages);
        let sent = writer.emit_message("hello", "node1").await;
        assert!(sent, "should send when Messages mode is enabled");

        // Verify the event was received
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::Messages { chunk, metadata } => {
                assert_eq!(chunk.content, "hello");
                assert_eq!(metadata.loom_node, "node1");
            }
            _ => panic!("expected Messages event"),
        }
    }

    /// **Scenario**: StreamWriter::emit_values only sends when Values mode is enabled.
    #[tokio::test]
    async fn stream_writer_emit_values_respects_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // Without Values mode - should not send
        let modes_without_values = HashSet::from_iter([StreamMode::Custom]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_without_values);
        let sent = writer.emit_values(DummyState(42)).await;
        assert!(!sent, "should not send when Values mode is disabled");

        // With Values mode - should send
        let modes_with_values = HashSet::from_iter([StreamMode::Values]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_with_values);
        let sent = writer.emit_values(DummyState(42)).await;
        assert!(sent, "should send when Values mode is enabled");

        // Verify the event was received
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::Values(state) => {
                assert_eq!(state, DummyState(42));
            }
            _ => panic!("expected Values event"),
        }
    }

    /// **Scenario**: StreamWriter::try_emit_custom works in non-blocking mode.
    #[test]
    fn stream_writer_try_emit_non_blocking() {
        let (tx, _) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // Should return false when mode is disabled
        let modes_without_custom = HashSet::from_iter([StreamMode::Values]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_without_custom);
        let sent = writer.try_emit_custom(serde_json::json!({"test": "value"}));
        assert!(!sent, "should not send when Custom mode is disabled");
    }

    /// **Scenario**: StreamWriter emits nothing when no sender is available.
    #[tokio::test]
    async fn stream_writer_no_sender_returns_false() {
        let modes = HashSet::from_iter([StreamMode::Custom, StreamMode::Messages, StreamMode::Values]);
        let writer = StreamWriter::<DummyState>::new(None, modes);

        assert!(!writer.emit_custom(serde_json::json!({"test": "value"})).await);
        assert!(!writer.emit_message("hello", "node1").await);
        assert!(!writer.emit_values(DummyState(1)).await);
    }

    /// **Scenario**: StreamWriter respects Checkpoints and Debug modes for checkpoint events.
    #[tokio::test]
    async fn stream_writer_emit_checkpoint_respects_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // Without Checkpoints or Debug mode - should not send
        let modes_without_checkpoints = HashSet::from_iter([StreamMode::Values]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_without_checkpoints);
        let sent = writer
            .emit_checkpoint("cp1", "2024-01-01", 0, DummyState(1), None, None)
            .await;
        assert!(!sent, "should not send when Checkpoints mode is disabled");

        // With Checkpoints mode - should send
        let modes_with_checkpoints = HashSet::from_iter([StreamMode::Checkpoints]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_with_checkpoints);
        let sent = writer
            .emit_checkpoint("cp1", "2024-01-01", 0, DummyState(1), None, None)
            .await;
        assert!(sent, "should send when Checkpoints mode is enabled");

        // Verify the event was received
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::Checkpoint(cp) => {
                assert_eq!(cp.checkpoint_id, "cp1");
                assert_eq!(cp.state, DummyState(1));
            }
            _ => panic!("expected Checkpoint event"),
        }
    }

    /// **Scenario**: StreamWriter respects Tasks and Debug modes for task events.
    #[tokio::test]
    async fn stream_writer_emit_task_start_respects_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // Without Tasks mode - should not send
        let modes_without_tasks = HashSet::from_iter([StreamMode::Values]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_without_tasks);
        let sent = writer.emit_task_start("think", None).await;
        assert!(!sent, "should not send when Tasks mode is disabled");

        // With Tasks mode - should send
        let modes_with_tasks = HashSet::from_iter([StreamMode::Tasks]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_with_tasks);
        let sent = writer.emit_task_start("think", None).await;
        assert!(sent, "should send when Tasks mode is enabled");

        // Verify the event was received
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::TaskStart { node_id, .. } => {
                assert_eq!(node_id, "think");
            }
            _ => panic!("expected TaskStart event"),
        }

        // With Debug mode - should also send (debug includes tasks)
        let modes_with_debug = HashSet::from_iter([StreamMode::Debug]);
        let writer = StreamWriter::new(Some(tx), modes_with_debug);
        let sent = writer.emit_task_start("act", None).await;
        assert!(sent, "should send when Debug mode is enabled");

        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::TaskStart { node_id, .. } => {
                assert_eq!(node_id, "act");
            }
            _ => panic!("expected TaskStart event"),
        }
    }

    /// **Scenario**: StreamWriter::emit_task_end only sends when Tasks or Debug mode is enabled.
    #[tokio::test]
    async fn stream_writer_emit_task_end() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // Without Tasks mode - should not send
        let modes_without_tasks = HashSet::from_iter([StreamMode::Values]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_without_tasks);
        let sent = writer.emit_task_end("node1", Ok(()), None).await;
        assert!(!sent, "should not send when Tasks mode is disabled");

        // With Tasks mode - should send success
        let modes_with_tasks = HashSet::from_iter([StreamMode::Tasks]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_with_tasks);
        let sent = writer.emit_task_end("think", Ok(()), None).await;
        assert!(sent, "should send when Tasks mode is enabled");

        // Verify the success event
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::TaskEnd {
                node_id, result, ..
            } => {
                assert_eq!(node_id, "think");
                assert!(result.is_ok());
            }
            _ => panic!("expected TaskEnd event"),
        }

        // Test error case
        let sent = writer
            .emit_task_end("think", Err("test error".to_string()), None)
            .await;
        assert!(sent, "should send error event");

        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::TaskEnd {
                node_id, result, ..
            } => {
                assert_eq!(node_id, "think");
                assert!(result.is_err());
                assert_eq!(result.unwrap_err(), "test error");
            }
            _ => panic!("expected TaskEnd event"),
        }
    }

    /// **Scenario**: StreamWriter is Clone and can be used across async tasks.
    #[tokio::test]
    async fn stream_writer_is_clone() {
        let modes = HashSet::from_iter([StreamMode::Custom]);
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        let writer1 = StreamWriter::new(Some(tx.clone()), modes.clone());
        let writer2 = writer1.clone();

        // Both writers should work
        let sent1 = writer1.emit_custom(serde_json::json!({"from": "writer1"})).await;
        let sent2 = writer2.emit_custom(serde_json::json!({"from": "writer2"})).await;

        assert!(sent1, "writer1 should send");
        assert!(sent2, "writer2 should send");

        // Verify we received both messages
        let event1 = rx.recv().await.expect("should receive first event");
        let event2 = rx.recv().await.expect("should receive second event");
        
        match (event1, event2) {
            (StreamEvent::Custom(v1), StreamEvent::Custom(v2)) => {
                assert!(v1 == serde_json::json!({"from": "writer1"}) || v1 == serde_json::json!({"from": "writer2"}));
                assert!(v2 == serde_json::json!({"from": "writer1"}) || v2 == serde_json::json!({"from": "writer2"}));
            }
            _ => panic!("expected Custom events"),
        }
    }

    /// **Scenario**: Debug implementation should not panic.
    #[test]
    fn stream_writer_debug_impl() {
        let modes = HashSet::from_iter([StreamMode::Custom]);
        let (tx, _) = mpsc::channel::<StreamEvent<DummyState>>(8);
        let writer = StreamWriter::new(Some(tx), modes);

        // Should not panic when formatting
        let debug_str = format!("{:?}", writer);
        assert!(debug_str.contains("StreamWriter"));
    }

    /// **Scenario**: ToolStreamWriter respects stream mode when emitting custom events.
    #[tokio::test]
    async fn stream_writer_emit_tool_call_respects_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // Without Tools mode - should not send
        let modes_without_tools = HashSet::from_iter([StreamMode::Custom]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_without_tools);
        let sent = writer
            .emit_tool_call(Some("call1".to_string()), "test_tool".to_string(), serde_json::json!({"arg": "value"}))
            .await;
        assert!(!sent, "should not send when Tools mode is disabled");

        // With Tools mode - should send
        let modes_with_tools = HashSet::from_iter([StreamMode::Tools]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_with_tools);
        let sent = writer
            .emit_tool_call(Some("call1".to_string()), "test_tool".to_string(), serde_json::json!({"arg": "value"}))
            .await;
        assert!(sent, "should send when Tools mode is enabled");

        // Verify the event was received
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::ToolCall {
                call_id,
                name,
                arguments,
            } => {
                assert_eq!(call_id, Some("call1".to_string()));
                assert_eq!(name, "test_tool");
                assert_eq!(arguments, serde_json::json!({"arg": "value"}));
            }
            _ => panic!("expected ToolCall event"),
        }
    }

    /// **Scenario**: ToolStreamWriter emit methods work with Debug mode enabled.
    #[tokio::test]
    async fn stream_writer_emit_tool_start_debug_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // With Debug mode - should send (debug includes tools)
        let modes_with_debug = HashSet::from_iter([StreamMode::Debug]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_with_debug);
        let sent = writer.emit_tool_start(Some("call1".to_string()), "test_tool".to_string()).await;
        assert!(sent, "should send when Debug mode is enabled");

        // Verify the event was received
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::ToolStart { call_id, name } => {
                assert_eq!(call_id, Some("call1".to_string()));
                assert_eq!(name, "test_tool");
            }
            _ => panic!("expected ToolStart event"),
        }
    }

    /// **Scenario**: ToolStreamWriter emit methods work correctly for tool lifecycle events.
    #[tokio::test]
    async fn stream_writer_emit_tool_end() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);
        let modes = HashSet::from_iter([StreamMode::Tools]);
        let writer = StreamWriter::new(Some(tx.clone()), modes);

        // Test success case
        let sent = writer
            .emit_tool_end(
                Some("call1".to_string()),
                "test_tool".to_string(),
                "result".to_string(),
                false,
                None,
            )
            .await;
        assert!(sent, "should send tool end event");

        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::ToolEnd {
                call_id,
                name,
                result,
                is_error,
                raw_result: _,
            } => {
                assert_eq!(call_id, Some("call1".to_string()));
                assert_eq!(name, "test_tool");
                assert_eq!(result, "result");
                assert!(!is_error);
            }
            _ => panic!("expected ToolEnd event"),
        }

        // Test error case
        let sent = writer
            .emit_tool_end(
                Some("call2".to_string()),
                "failing_tool".to_string(),
                "error message".to_string(),
                true,
                None,
            )
            .await;
        assert!(sent, "should send tool end error event");

        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::ToolEnd {
                call_id,
                name,
                result,
                is_error,
                raw_result: _,
            } => {
                assert_eq!(call_id, Some("call2".to_string()));
                assert_eq!(name, "failing_tool");
                assert_eq!(result, "error message");
                assert!(is_error);
            }
            _ => panic!("expected ToolEnd event"),
        }
    }

    /// **Scenario**: ToolStreamWriter emit approval events work correctly.
    #[tokio::test]
    async fn stream_writer_emit_tool_approval() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);
        let modes = HashSet::from_iter([StreamMode::Tools]);
        let writer = StreamWriter::new(Some(tx.clone()), modes);

        let sent = writer
            .emit_tool_approval(
                Some("call1".to_string()),
                "sensitive_tool".to_string(),
                serde_json::json!({"file": "secret.txt"}),
            )
            .await;
        assert!(sent, "should send tool approval event");

        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::ToolApproval {
                call_id,
                name,
                arguments,
            } => {
                assert_eq!(call_id, Some("call1".to_string()));
                assert_eq!(name, "sensitive_tool");
                assert_eq!(arguments, serde_json::json!({"file": "secret.txt"}));
            }
            _ => panic!("expected ToolApproval event"),
        }
    }
}