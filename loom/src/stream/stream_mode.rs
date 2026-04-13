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
    /// Emit tool lifecycle events (tool_call, tool_start, tool_output, tool_end, tool_approval).
    Tools,
    /// Emit both checkpoints and tasks events (debug mode).
    Debug,
}
