use std::fmt::Debug;

/// Metadata attached to streamed messages.
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
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