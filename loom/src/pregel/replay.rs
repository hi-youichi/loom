//! Replay and fork request types for Pregel checkpoints.

use crate::pregel::state::PregelStateSnapshot;

/// Supported replay operations over persisted checkpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
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
