use serde::{Deserialize, Serialize};

/// Streaming modes for graph execution.
/// Each mode controls which types of events are emitted during execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamMode {
    /// Emit full state after each node completes.
    Values,
    /// Emit incremental updates with node id and state.
    Updates,
    /// Emit message chunks (LLM streaming).
    Messages,
    /// Emit custom JSON payloads from nodes or tools.
    Custom,
    /// Emit checkpoint events when checkpoints are created.
    Checkpoints,
    /// Emit task start/end events for each node execution.
    Tasks,
    /// Emit tool lifecycle events (tool_call, tool_start, tool_output, tool_end).
    Tools,
    /// Emit both checkpoints and tasks events (debug mode).
    Debug,
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_mode_all_variants_serialize() {
        let modes = vec![
            StreamMode::Values,
            StreamMode::Updates,
            StreamMode::Messages,
            StreamMode::Custom,
            StreamMode::Checkpoints,
            StreamMode::Tasks,
            StreamMode::Tools,
            StreamMode::Debug,
        ];
        for mode in &modes {
            let json = serde_json::to_string(mode).unwrap();
            let back: StreamMode = serde_json::from_str(&json).unwrap();
            assert_eq!(*mode, back);
        }
    }

    #[test]
    fn stream_mode_hash_eq() {
        use std::collections::HashSet;
        let set: HashSet<StreamMode> = [
            StreamMode::Values,
            StreamMode::Updates,
            StreamMode::Values,
        ].into_iter().collect();
        assert_eq!(set.len(), 2);
    }
}
