#[cfg(test)]
mod tests {
    use crate::stream::{MessageChunk, StreamEvent, StreamMetadata};
    use serde_json;

    #[derive(Clone, Debug, PartialEq)]
    struct DummyState(i32);

    /// **Scenario**: Each StreamEvent variant can hold the appropriate data.
    #[tokio::test]
    async fn stream_event_variants_hold_data() {
        // Test Values variant
        let event: StreamEvent<DummyState> = StreamEvent::Values(DummyState(42));
        match event {
            StreamEvent::Values(state) => assert_eq!(state, DummyState(42)),
            _ => panic!("expected Values"),
        }

        // Test Updates variant
        let event: StreamEvent<DummyState> = StreamEvent::Updates {
            node_id: "test_node".to_string(),
            state: DummyState(123),
            namespace: Some("test_ns".to_string()),
        };
        match event {
            StreamEvent::Updates {
                node_id,
                state,
                namespace,
            } => {
                assert_eq!(node_id, "test_node");
                assert_eq!(state, DummyState(123));
                assert_eq!(namespace, Some("test_ns".to_string()));
            }
            _ => panic!("expected Updates"),
        }

        // Test Messages variant
        let event: StreamEvent<DummyState> = StreamEvent::Messages {
            chunk: MessageChunk::message("hello"),
            metadata: StreamMetadata {
                loom_node: "node1".to_string(),
                namespace: None,
            },
        };
        match event {
            StreamEvent::Messages { chunk, metadata } => {
                assert_eq!(chunk.content, "hello");
                assert_eq!(metadata.loom_node, "node1");
            }
            _ => panic!("expected Messages"),
        }

        // Test Custom variant
        let event: StreamEvent<DummyState> = StreamEvent::Custom(serde_json::json!({"test": "value"}));
        match event {
            StreamEvent::Custom(value) => {
                assert_eq!(value, serde_json::json!({"test": "value"}));
            }
            _ => panic!("expected Custom"),
        }

        // Test ToolCallChunk variant
        let event: StreamEvent<DummyState> = StreamEvent::ToolCallChunk {
            call_id: Some("call1".to_string()),
            name: Some("tool1".to_string()),
            arguments_delta: "{\"arg\": \"value\"}".to_string(),
        };
        match event {
            StreamEvent::ToolCallChunk {
                call_id,
                name,
                arguments_delta,
            } => {
                assert_eq!(call_id, Some("call1".to_string()));
                assert_eq!(name, Some("tool1".to_string()));
                assert_eq!(arguments_delta, "{\"arg\": \"value\"}".to_string());
            }
            _ => panic!("expected ToolCallChunk"),
        }

        // Test ToolCall variant
        let event: StreamEvent<DummyState> = StreamEvent::ToolCall {
            call_id: Some("call1".to_string()),
            name: "tool1".to_string(),
            arguments: serde_json::json!({"arg": "value"}),
        };
        match event {
            StreamEvent::ToolCall {
                call_id,
                name,
                arguments,
            } => {
                assert_eq!(call_id, Some("call1".to_string()));
                assert_eq!(name, "tool1".to_string());
                assert_eq!(arguments, serde_json::json!({"arg": "value"}));
            }
            _ => panic!("expected ToolCall"),
        }

        // Test ToolStart variant
        let event: StreamEvent<DummyState> = StreamEvent::ToolStart {
            call_id: Some("call1".to_string()),
            name: "tool1".to_string(),
        };
        match event {
            StreamEvent::ToolStart { call_id, name } => {
                assert_eq!(call_id, Some("call1".to_string()));
                assert_eq!(name, "tool1".to_string());
            }
            _ => panic!("expected ToolStart"),
        }

        // Test ToolOutput variant
        let event: StreamEvent<DummyState> = StreamEvent::ToolOutput {
            call_id: Some("call1".to_string()),
            name: "tool1".to_string(),
            content: "output".to_string(),
        };
        match event {
            StreamEvent::ToolOutput {
                call_id,
                name,
                content,
            } => {
                assert_eq!(call_id, Some("call1".to_string()));
                assert_eq!(name, "tool1".to_string());
                assert_eq!(content, "output".to_string());
            }
            _ => panic!("expected ToolOutput"),
        }

        // Test ToolEnd variant
        let event: StreamEvent<DummyState> = StreamEvent::ToolEnd {
            call_id: Some("call1".to_string()),
            name: "tool1".to_string(),
            result: "result".to_string(),
            is_error: false,
        };
        match event {
            StreamEvent::ToolEnd {
                call_id,
                name,
                result,
                is_error,
            } => {
                assert_eq!(call_id, Some("call1".to_string()));
                assert_eq!(name, "tool1".to_string());
                assert_eq!(result, "result".to_string());
                assert!(!is_error);
            }
            _ => panic!("expected ToolEnd"),
        }

        // Test ToolApproval variant
        let event: StreamEvent<DummyState> = StreamEvent::ToolApproval {
            call_id: Some("call1".to_string()),
            name: "tool1".to_string(),
            arguments: serde_json::json!({"arg": "value"}),
        };
        match event {
            StreamEvent::ToolApproval {
                call_id,
                name,
                arguments,
            } => {
                assert_eq!(call_id, Some("call1".to_string()));
                assert_eq!(name, "tool1".to_string());
                assert_eq!(arguments, serde_json::json!({"arg": "value"}));
            }
            _ => panic!("expected ToolApproval"),
        }
    }
}