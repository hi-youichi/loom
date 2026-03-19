//! Initial subgraph invocation scaffolding for Pregel runtimes.

use crate::pregel::types::{ChannelValue, InterruptRecord, TaskId};

/// Checkpoint namespace used to isolate a subgraph lineage.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CheckpointNamespace(pub String);

impl CheckpointNamespace {
    pub fn root() -> Self {
        Self("root".to_string())
    }

    pub fn child(&self, segment: impl AsRef<str>) -> Self {
        if self.0.is_empty() {
            Self(segment.as_ref().to_string())
        } else {
            Self(format!("{}/{}", self.0, segment.as_ref()))
        }
    }
}

/// Request to invoke a child Pregel runtime from a parent task.
#[derive(Debug, Clone, PartialEq)]
pub struct SubgraphInvocation {
    pub parent_task_id: TaskId,
    pub parent_checkpoint_id: Option<String>,
    pub child_namespace: CheckpointNamespace,
    pub entry_input: ChannelValue,
}

/// Result of a child Pregel runtime execution.
#[derive(Debug, Clone, PartialEq)]
pub enum SubgraphResult {
    Completed(ChannelValue),
    Interrupted(InterruptRecord),
    Cancelled,
    Failed(String),
}
