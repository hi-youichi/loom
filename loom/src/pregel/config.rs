//! Pregel runtime configuration.

use crate::graph::RetryPolicy;
use crate::pregel::types::NodeName;
use crate::stream::StreamMode;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_conservative() {
        let config = PregelConfig::default();
        assert_eq!(config.max_steps, 100);
        assert!(config.stream_mode.is_empty());
        assert!(config.interrupt_before.is_empty());
        assert!(config.interrupt_after.is_empty());
        assert!(matches!(config.durability, PregelDurability::Sync));
    }
}
