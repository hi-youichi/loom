//! Pregel checkpoint-backed state inspection types.

use crate::memory::Checkpoint;
use crate::pregel::types::{ChannelName, ChannelValue, InterruptRecord, PendingWrite};

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
            step: checkpoint.metadata.step.max(0) as u64,
            channels: checkpoint.channel_values.clone(),
            parents: checkpoint.metadata.parents.clone(),
            children: checkpoint.metadata.children.clone(),
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
