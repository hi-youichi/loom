//! Initial subgraph invocation scaffolding for Pregel runtimes.

use std::sync::Arc;

use crate::pregel::runtime::PregelRuntime;
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

/// A named child Pregel runtime exposed by a node for inspection/export.
#[derive(Clone)]
pub struct PregelSubgraph {
    pub name: String,
    pub runtime: Arc<PregelRuntime>,
}

impl std::fmt::Debug for PregelSubgraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PregelSubgraph")
            .field("name", &self.name)
            .field("runtime", &self.runtime)
            .finish()
    }
}

/// One discovered subgraph entry from a recursive traversal.
#[derive(Debug, Clone)]
pub struct PregelSubgraphEntry {
    pub path: String,
    pub runtime: PregelRuntime,
}

#[cfg(test)]
mod tests {
    use crate::pregel::{CheckpointNamespace, PregelGraph, PregelRuntime, PregelSubgraph};
    use std::sync::Arc;

    #[test]
    fn checkpoint_namespace_root() {
        assert_eq!(CheckpointNamespace::root().0, "root");
    }

    #[test]
    fn checkpoint_namespace_child_empty_parent() {
        let ns = CheckpointNamespace("".to_string());
        let child = ns.child("seg");
        assert_eq!(child.0, "seg");
    }

    #[test]
    fn checkpoint_namespace_child_nonempty_parent() {
        let ns = CheckpointNamespace("a".to_string());
        let child = ns.child("b");
        assert_eq!(child.0, "a/b");
    }

    #[test]
    fn pregel_subgraph_debug_impl() {
        let subgraph = PregelSubgraph {
            name: "test".to_string(),
            runtime: Arc::new(PregelRuntime::new(PregelGraph::new())),
        };
        let debug_str = format!("{:?}", subgraph);
        assert!(debug_str.contains("PregelSubgraph"));
        assert!(debug_str.contains("test"));
    }
}
