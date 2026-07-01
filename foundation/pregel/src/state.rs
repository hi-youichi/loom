//! Pregel checkpoint-backed state inspection types.

use checkpoint::Checkpoint;
use crate::types::{ChannelName, ChannelValue, InterruptRecord, PendingWrite};

/// Materialized runtime state loaded from a checkpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct PregelStateSnapshot {
    pub checkpoint_id: String,
    pub step: u64,
    pub channels: ChannelValue,
    pub parents: std::collections::HashMap<String, String>,
    pub children: std::collections::HashMap<String, Vec<String>>,
    pub updated_channels: Vec<ChannelName>,
    pub pending_sends: Vec<PendingWrite>,
    pub pending_writes: Vec<PendingWrite>,
    pub pending_interrupts: Vec<InterruptRecord>,
}

impl PregelStateSnapshot {
    /// Builds a state snapshot from a persisted checkpoint.
    pub fn from_checkpoint(checkpoint: &Checkpoint<ChannelValue>) -> Self {
        Self {
            checkpoint_id: checkpoint.id.clone(),
            step: checkpoint.kernel.step.max(0) as u64,
            channels: checkpoint.channel_values.clone(),
            parents: checkpoint.kernel.parents.clone(),
            children: checkpoint.kernel.children.clone(),
            updated_channels: checkpoint.updated_channels.clone().unwrap_or_default(),
            pending_sends: checkpoint.pending_sends.clone(),
            pending_writes: checkpoint.pending_writes.clone(),
            pending_interrupts: checkpoint
                .pending_interrupts
                .iter()
                .filter_map(|value| serde_json::from_value(value.clone()).ok())
                .collect(),
        }
    }
}

/// A synthetic state update routed through Pregel write application.
#[derive(Debug, Clone, PartialEq)]
pub struct StateUpdateRequest {
    pub as_node: Option<String>,
    pub values: ChannelValue,
}

/// A batch of synthetic state updates committed at one barrier.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BulkStateUpdateRequest {
    pub updates: Vec<StateUpdateRequest>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use checkpoint::Checkpoint;

    fn create_minimal_checkpoint() -> Checkpoint<serde_json::Value> {
        Checkpoint {
            id: "empty".to_string(),
            ts: "test-timestamp".to_string(),
            v: 1,
            user: (),
            kernel: checkpoint::KernelMetadata {
                source: checkpoint::CheckpointSource::default(),
                step: 5,
                created_at: None,
                parents: std::collections::HashMap::new(),
                children: std::collections::HashMap::new(),
                summary: None,
            },
            channel_values: serde_json::json!({
                "channel1": "value1",
                "channel2": 42,
            }),
            updated_channels: Some(vec!["channel1".to_string(), "channel2".to_string()]),
            pending_sends: vec![
                ("task-1".to_string(), "channel1".to_string(), serde_json::json!("send1")),
            ],
            pending_writes: vec![
                ("task-2".to_string(), "channel2".to_string(), serde_json::json!("write1")),
            ],
            pending_interrupts: vec![
                serde_json::json!({
                    "interrupt_id": "int-1",
                    "namespace": "ns-1",
                    "task_id": "task-1",
                    "node_name": "node-1",
                    "step": 3,
                    "value": "interrupt-value",
                }),
            ],
            channel_versions: std::collections::HashMap::new(),
            versions_seen: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn test_pregel_state_snapshot_from_checkpoint() {
        let checkpoint = create_minimal_checkpoint();
        let snapshot = PregelStateSnapshot::from_checkpoint(&checkpoint);

        assert_eq!(snapshot.checkpoint_id, "empty");
        assert_eq!(snapshot.step, 5);
        assert_eq!(snapshot.channels, checkpoint.channel_values);
        assert_eq!(snapshot.updated_channels, vec!["channel1".to_string(), "channel2".to_string()]);
        assert_eq!(snapshot.pending_sends.len(), 1);
        assert_eq!(snapshot.pending_writes.len(), 1);
    }

    #[test]
    fn test_pregel_state_snapshot_from_checkpoint_empty_updated_channels() {
        let mut checkpoint = create_minimal_checkpoint();
        checkpoint.updated_channels = None;

        let snapshot = PregelStateSnapshot::from_checkpoint(&checkpoint);
        assert!(snapshot.updated_channels.is_empty());
    }

    #[test]
    fn test_pregel_state_snapshot_from_checkpoint_with_negative_step() {
        let mut checkpoint = create_minimal_checkpoint();
        checkpoint.kernel.step = -3;

        let snapshot = PregelStateSnapshot::from_checkpoint(&checkpoint);
        assert_eq!(snapshot.step, 0);
    }

    #[test]
    fn test_pregel_state_snapshot_from_checkpoint_interrupt_parsing() {
        let checkpoint = create_minimal_checkpoint();
        let snapshot = PregelStateSnapshot::from_checkpoint(&checkpoint);

        assert_eq!(snapshot.pending_interrupts.len(), 1);
        let interrupt = &snapshot.pending_interrupts[0];
        assert_eq!(interrupt.interrupt_id, "int-1");
        assert_eq!(interrupt.namespace, "ns-1");
    }

    #[test]
    fn test_pregel_state_snapshot_from_checkpoint_invalid_interrupt() {
        let mut checkpoint = create_minimal_checkpoint();
        checkpoint.pending_interrupts.push(serde_json::json!("invalid interrupt data"));

        let snapshot = PregelStateSnapshot::from_checkpoint(&checkpoint);
        assert_eq!(snapshot.pending_interrupts.len(), 1);
    }

    #[test]
    fn test_pregel_state_snapshot_equality() {
        let checkpoint = create_minimal_checkpoint();
        let snapshot1 = PregelStateSnapshot::from_checkpoint(&checkpoint);
        let snapshot2 = PregelStateSnapshot::from_checkpoint(&checkpoint);

        assert_eq!(snapshot1, snapshot2);
    }

    #[test]
    fn test_pregel_state_snapshot_clone() {
        let checkpoint = create_minimal_checkpoint();
        let snapshot = PregelStateSnapshot::from_checkpoint(&checkpoint);
        let cloned = snapshot.clone();

        assert_eq!(snapshot, cloned);
        assert_eq!(snapshot.checkpoint_id, cloned.checkpoint_id);
    }

    #[test]
    fn test_state_update_request_construction() {
        let request = StateUpdateRequest {
            as_node: Some("node-1".to_string()),
            values: serde_json::json!({"key": "value"}),
        };

        assert_eq!(request.as_node, Some("node-1".to_string()));
        assert_eq!(request.values, serde_json::json!({"key": "value"}));
    }

    #[test]
    fn test_state_update_request_with_none_as_node() {
        let request = StateUpdateRequest {
            as_node: None,
            values: serde_json::json!("simple-value"),
        };

        assert!(request.as_node.is_none());
        assert_eq!(request.values, serde_json::json!("simple-value"));
    }

    #[test]
    fn test_state_update_request_equality() {
        let request1 = StateUpdateRequest {
            as_node: Some("node-1".to_string()),
            values: serde_json::json!({"key": "value"}),
        };

        let request2 = StateUpdateRequest {
            as_node: Some("node-1".to_string()),
            values: serde_json::json!({"key": "value"}),
        };

        assert_eq!(request1, request2);
    }

    #[test]
    fn test_state_update_request_clone() {
        let request = StateUpdateRequest {
            as_node: Some("node-1".to_string()),
            values: serde_json::json!({"key": "value"}),
        };

        let cloned = request.clone();
        assert_eq!(request, cloned);
    }

    #[test]
    fn test_bulk_state_update_request_default() {
        let bulk = BulkStateUpdateRequest::default();
        assert!(bulk.updates.is_empty());
    }

    #[test]
    fn test_bulk_state_update_request_with_updates() {
        let bulk = BulkStateUpdateRequest {
            updates: vec![
                StateUpdateRequest {
                    as_node: Some("node-1".to_string()),
                    values: serde_json::json!("value1"),
                },
                StateUpdateRequest {
                    as_node: None,
                    values: serde_json::json!("value2"),
                },
            ],
        };

        assert_eq!(bulk.updates.len(), 2);
    }

    #[test]
    fn test_bulk_state_update_request_equality() {
        let updates = vec![
            StateUpdateRequest {
                as_node: Some("node-1".to_string()),
                values: serde_json::json!("value"),
            },
        ];

        let bulk1 = BulkStateUpdateRequest {
            updates: updates.clone(),
        };

        let bulk2 = BulkStateUpdateRequest {
            updates: updates.clone(),
        };

        assert_eq!(bulk1, bulk2);
    }

    #[test]
    fn test_bulk_state_update_request_clone() {
        let updates = vec![
            StateUpdateRequest {
                as_node: Some("node-1".to_string()),
                values: serde_json::json!("value"),
            },
        ];

        let bulk = BulkStateUpdateRequest {
            updates: updates.clone(),
        };

        let cloned = bulk.clone();
        assert_eq!(bulk, cloned);
    }

    #[test]
    fn test_pregel_state_snapshot_empty_checkpoint() {
        let empty_checkpoint = Checkpoint {
            id: "empty".to_string(),
            ts: "test-timestamp".to_string(),
            v: 1,
            user: (),
            kernel: checkpoint::KernelMetadata {
                source: checkpoint::CheckpointSource::default(),
                step: 0,
                created_at: None,
                parents: std::collections::HashMap::new(),
                children: std::collections::HashMap::new(),
                summary: None,
            },
            channel_values: serde_json::json!({}),
            updated_channels: None,
            pending_sends: vec![],
            pending_writes: vec![],
            pending_interrupts: vec![],
            channel_versions: std::collections::HashMap::new(),
            versions_seen: std::collections::HashMap::new(),
        };

        let snapshot = PregelStateSnapshot::from_checkpoint(&empty_checkpoint);
        assert_eq!(snapshot.checkpoint_id, "empty");
        assert_eq!(snapshot.step, 0);
        assert!(snapshot.updated_channels.is_empty());
        assert!(snapshot.pending_sends.is_empty());
        assert!(snapshot.pending_writes.is_empty());
        assert!(snapshot.pending_interrupts.is_empty());
    }

    #[test]
    fn test_state_update_request_not_equal_different_values() {
        let request1 = StateUpdateRequest {
            as_node: Some("node-1".to_string()),
            values: serde_json::json!("value1"),
        };

        let request2 = StateUpdateRequest {
            as_node: Some("node-1".to_string()),
            values: serde_json::json!("value2"),
        };

        assert_ne!(request1, request2);
    }

    #[test]
    fn test_state_update_request_not_equal_different_node() {
        let request1 = StateUpdateRequest {
            as_node: Some("node-1".to_string()),
            values: serde_json::json!("value"),
        };

        let request2 = StateUpdateRequest {
            as_node: Some("node-2".to_string()),
            values: serde_json::json!("value"),
        };

        assert_ne!(request1, request2);
    }
}
