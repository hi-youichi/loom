//! Pregel runtime configuration.

use loom_graph::RetryPolicy;
use crate::types::NodeName;
use stream_event::StreamMode;

/// Checkpoint durability behavior for Pregel runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PregelDurability {
    /// Persist checkpoint data before advancing to the next step.
    #[default]
    Sync,
    /// Persist checkpoint data in the background while the next step runs.
    Async,
    /// Defer persistence until the run exits.
    Exit,
}

/// Top-level configuration for the Pregel runtime.
#[derive(Debug, Clone)]
pub struct PregelConfig {
    /// Maximum number of steps before the loop aborts.
    pub max_steps: u64,
    /// Retry policy applied per task execution.
    pub retry_policy: RetryPolicy,
    /// Checkpoint durability strategy.
    pub durability: PregelDurability,
    /// Enabled stream modes for this run.
    pub stream_mode: Vec<StreamMode>,
    /// Nodes that should interrupt before execution.
    pub interrupt_before: Vec<NodeName>,
    /// Nodes that should interrupt after execution.
    pub interrupt_after: Vec<NodeName>,
}

impl Default for PregelConfig {
    fn default() -> Self {
        Self {
            max_steps: 100,
            retry_policy: RetryPolicy::default(),
            durability: PregelDurability::default(),
            stream_mode: Vec::new(),
            interrupt_before: Vec::new(),
            interrupt_after: Vec::new(),
        }
    }
}
