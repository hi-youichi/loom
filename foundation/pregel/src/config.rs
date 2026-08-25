//! Pregel runtime configuration.

use crate::types::NodeName;
use anureo_graph_core::RetryPolicy;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pregel_durability_default_is_sync() {
        assert_eq!(PregelDurability::default(), PregelDurability::Sync);
    }

    #[test]
    fn test_pregel_durability_variants() {
        assert_eq!(PregelDurability::Sync, PregelDurability::Sync);
        assert_eq!(PregelDurability::Async, PregelDurability::Async);
        assert_eq!(PregelDurability::Exit, PregelDurability::Exit);

        assert_ne!(PregelDurability::Sync, PregelDurability::Async);
        assert_ne!(PregelDurability::Async, PregelDurability::Exit);
    }

    #[test]
    fn test_pregel_durability_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(PregelDurability::Sync);
        set.insert(PregelDurability::Async);
        set.insert(PregelDurability::Sync);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_pregel_config_default() {
        let config = PregelConfig::default();
        assert_eq!(config.max_steps, 100);
        assert_eq!(config.durability, PregelDurability::Sync);
        assert!(config.stream_mode.is_empty());
        assert!(config.interrupt_before.is_empty());
        assert!(config.interrupt_after.is_empty());
    }

    #[test]
    fn test_pregel_config_clone() {
        let config = PregelConfig::default();
        let cloned = config.clone();
        assert_eq!(config.max_steps, cloned.max_steps);
        assert_eq!(config.durability, cloned.durability);
    }

    #[test]
    fn test_pregel_config_debug() {
        let config = PregelConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("PregelConfig"));
    }
}
