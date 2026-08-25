//! Replay and fork request types for Pregel checkpoints.

use crate::state::PregelStateSnapshot;

/// Supported replay operations over persisted checkpoints.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReplayMode {
    ResumeFromCheckpoint(String),
    ForkFromCheckpoint(String),
    InspectCheckpoint(String),
}

/// A replay request optionally scoped to a checkpoint namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayRequest {
    pub mode: ReplayMode,
    pub namespace: Option<String>,
}

/// Result of a replay-oriented state operation.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayResult {
    pub snapshot: PregelStateSnapshot,
    pub forked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_state_snapshot() -> PregelStateSnapshot {
        PregelStateSnapshot {
            checkpoint_id: "checkpoint-123".to_string(),
            step: 5,
            channels: serde_json::json!({"channel1": "value1"}),
            parents: std::collections::HashMap::new(),
            children: std::collections::HashMap::new(),
            updated_channels: vec!["channel1".to_string()],
            pending_sends: vec![],
            pending_writes: vec![],
            pending_interrupts: vec![],
        }
    }

    #[test]
    fn test_replay_mode_variants() {
        let resume = ReplayMode::ResumeFromCheckpoint("check-1".to_string());
        let fork = ReplayMode::ForkFromCheckpoint("check-2".to_string());
        let inspect = ReplayMode::InspectCheckpoint("check-3".to_string());

        assert_eq!(resume, resume);
        assert_eq!(fork, fork);
        assert_eq!(inspect, inspect);

        assert_ne!(resume, fork);
        assert_ne!(fork, inspect);
        assert_ne!(inspect, resume);
    }

    #[test]
    fn test_replay_mode_equality() {
        let mode1 = ReplayMode::ResumeFromCheckpoint("check-1".to_string());
        let mode2 = ReplayMode::ResumeFromCheckpoint("check-1".to_string());
        assert_eq!(mode1, mode2);

        let mode3 = ReplayMode::ResumeFromCheckpoint("check-2".to_string());
        assert_ne!(mode1, mode3);
    }

    #[test]
    fn test_replay_mode_clone() {
        let mode = ReplayMode::ResumeFromCheckpoint("check-1".to_string());
        let cloned = mode.clone();
        assert_eq!(mode, cloned);
    }

    #[test]
    fn test_replay_mode_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();

        set.insert(ReplayMode::ResumeFromCheckpoint("check-1".to_string()));
        set.insert(ReplayMode::ForkFromCheckpoint("check-2".to_string()));
        set.insert(ReplayMode::ResumeFromCheckpoint("check-1".to_string()));

        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_replay_request_construction() {
        let mode = ReplayMode::ResumeFromCheckpoint("check-1".to_string());
        let request = ReplayRequest {
            mode: mode.clone(),
            namespace: Some("ns-1".to_string()),
        };

        assert_eq!(request.mode, mode);
        assert_eq!(request.namespace, Some("ns-1".to_string()));
    }

    #[test]
    fn test_replay_request_with_none_namespace() {
        let mode = ReplayMode::ForkFromCheckpoint("check-2".to_string());
        let request = ReplayRequest {
            mode,
            namespace: None,
        };

        assert!(request.namespace.is_none());
    }

    #[test]
    fn test_replay_request_equality() {
        let request1 = ReplayRequest {
            mode: ReplayMode::ResumeFromCheckpoint("check-1".to_string()),
            namespace: Some("ns-1".to_string()),
        };

        let request2 = ReplayRequest {
            mode: ReplayMode::ResumeFromCheckpoint("check-1".to_string()),
            namespace: Some("ns-1".to_string()),
        };

        assert_eq!(request1, request2);
    }

    #[test]
    fn test_replay_request_clone() {
        let request = ReplayRequest {
            mode: ReplayMode::InspectCheckpoint("check-1".to_string()),
            namespace: Some("ns-1".to_string()),
        };

        let cloned = request.clone();
        assert_eq!(request.mode, cloned.mode);
        assert_eq!(request.namespace, cloned.namespace);
    }

    #[test]
    fn test_replay_result_construction() {
        let snapshot = create_test_state_snapshot();
        let result = ReplayResult {
            snapshot: snapshot.clone(),
            forked: true,
        };

        assert_eq!(result.snapshot.checkpoint_id, "checkpoint-123");
        assert_eq!(result.snapshot.step, 5);
        assert!(result.forked);
    }

    #[test]
    fn test_replay_result_with_forked_false() {
        let snapshot = create_test_state_snapshot();
        let result = ReplayResult {
            snapshot,
            forked: false,
        };

        assert!(!result.forked);
    }

    #[test]
    fn test_replay_result_equality() {
        let snapshot1 = create_test_state_snapshot();
        let snapshot2 = create_test_state_snapshot();

        let result1 = ReplayResult {
            snapshot: snapshot1,
            forked: true,
        };

        let result2 = ReplayResult {
            snapshot: snapshot2,
            forked: true,
        };

        assert_eq!(result1, result2);
    }

    #[test]
    fn test_replay_result_clone() {
        let snapshot = create_test_state_snapshot();
        let result = ReplayResult {
            snapshot: snapshot.clone(),
            forked: false,
        };

        let cloned = result.clone();
        assert_eq!(result, cloned);
        assert_eq!(result.snapshot.checkpoint_id, cloned.snapshot.checkpoint_id);
    }

    #[test]
    fn test_replay_result_snapshot_fields() {
        let snapshot = create_test_state_snapshot();
        let result = ReplayResult {
            snapshot,
            forked: true,
        };

        assert_eq!(result.snapshot.checkpoint_id, "checkpoint-123");
        assert_eq!(result.snapshot.step, 5);
        assert_eq!(
            result.snapshot.channels,
            serde_json::json!({"channel1": "value1"})
        );
        assert_eq!(
            result.snapshot.updated_channels,
            vec!["channel1".to_string()]
        );
    }

    #[test]
    fn test_replay_mode_inspect_different_from_resume() {
        let inspect = ReplayMode::InspectCheckpoint("check-1".to_string());
        let resume = ReplayMode::ResumeFromCheckpoint("check-1".to_string());

        assert_ne!(inspect, resume);
    }

    #[test]
    fn test_replay_result_not_equal_with_different_forked() {
        let snapshot1 = create_test_state_snapshot();
        let snapshot2 = create_test_state_snapshot();

        let result1 = ReplayResult {
            snapshot: snapshot1,
            forked: true,
        };

        let result2 = ReplayResult {
            snapshot: snapshot2,
            forked: false,
        };

        assert_ne!(result1, result2);
    }

    #[test]
    fn test_replay_request_not_equal_with_different_namespace() {
        let request1 = ReplayRequest {
            mode: ReplayMode::ResumeFromCheckpoint("check-1".to_string()),
            namespace: Some("ns-1".to_string()),
        };

        let request2 = ReplayRequest {
            mode: ReplayMode::ResumeFromCheckpoint("check-1".to_string()),
            namespace: Some("ns-2".to_string()),
        };

        assert_ne!(request1, request2);
    }
}
