use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Metadata attached to streamed messages.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StreamMetadata {
    /// Loom node id that produced the message.
    pub loom_node: String,
    /// Optional namespace for subgraph events.
    pub namespace: Option<String>,
}

/// Checkpoint event emitted when a checkpoint is created.
///
/// Contains the checkpoint id, metadata, and optionally the state snapshot.
/// This aligns with graph-based checkpoint streaming format.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckpointEvent<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Unique checkpoint identifier.
    pub checkpoint_id: String,
    /// Timestamp when checkpoint was created.
    pub timestamp: String,
    /// Step number in the graph execution (-1 for input, 0+ for loop).
    pub step: i64,
    /// The state snapshot at this checkpoint.
    pub state: S,
    /// Thread ID associated with this checkpoint.
    pub thread_id: Option<String>,
    /// Checkpoint namespace (for subgraphs).
    pub checkpoint_ns: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_metadata_fields() {
        let m = StreamMetadata {
            loom_node: "think".to_string(),
            namespace: Some("sub".to_string()),
        };
        assert_eq!(m.loom_node, "think");
        assert_eq!(m.namespace.as_deref(), Some("sub"));
    }

    #[test]
    fn stream_metadata_no_namespace() {
        let m = StreamMetadata {
            loom_node: "act".to_string(),
            namespace: None,
        };
        assert!(m.namespace.is_none());
    }

    #[test]
    fn checkpoint_event_fields() {
        let ev: CheckpointEvent<String> = CheckpointEvent {
            checkpoint_id: "cp-1".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            step: 0,
            state: "test_state".to_string(),
            thread_id: Some("t-1".to_string()),
            checkpoint_ns: Some("ns".to_string()),
        };
        assert_eq!(ev.checkpoint_id, "cp-1");
        assert_eq!(ev.step, 0);
        assert_eq!(ev.state, "test_state");
    }

    #[test]
    fn checkpoint_event_debug() {
        let ev: CheckpointEvent<i32> = CheckpointEvent {
            checkpoint_id: "cp-2".to_string(),
            timestamp: "2025-01-01".to_string(),
            step: -1,
            state: 42,
            thread_id: None,
            checkpoint_ns: None,
        };
        let s = format!("{:?}", ev);
        assert!(s.contains("cp-2"));
    }
}
