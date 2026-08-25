//! Initial subgraph invocation scaffolding for Pregel runtimes.

use std::sync::Arc;

use crate::runtime::PregelRuntime;
use crate::types::{ChannelValue, InterruptRecord, TaskId};

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
    use super::*;

    #[test]
    fn test_checkpoint_namespace_root() {
        let ns = CheckpointNamespace::root();
        assert_eq!(ns.0, "root");
    }

    #[test]
    fn test_checkpoint_namespace_child_from_root() {
        let root = CheckpointNamespace::root();
        let child = root.child("segment1");
        assert_eq!(child.0, "root/segment1");
    }

    #[test]
    fn test_checkpoint_namespace_child_from_empty() {
        let empty = CheckpointNamespace("".to_string());
        let child = empty.child("segment1");
        assert_eq!(child.0, "segment1");
    }

    #[test]
    fn test_checkpoint_namespace_child_chain() {
        let ns = CheckpointNamespace::root().child("a").child("b").child("c");
        assert_eq!(ns.0, "root/a/b/c");
    }

    #[test]
    fn test_checkpoint_namespace_equality() {
        let ns1 = CheckpointNamespace::root().child("test");
        let ns2 = CheckpointNamespace::root().child("test");
        assert_eq!(ns1, ns2);

        let ns3 = CheckpointNamespace::root().child("other");
        assert_ne!(ns1, ns3);
    }

    #[test]
    fn test_checkpoint_namespace_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(CheckpointNamespace::root().child("a"));
        set.insert(CheckpointNamespace::root().child("b"));
        set.insert(CheckpointNamespace::root().child("a"));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_checkpoint_namespace_serialization_roundtrip() {
        let ns = CheckpointNamespace::root().child("test").child("segment");
        let serialized = serde_json::to_string(&ns).unwrap();
        let deserialized: CheckpointNamespace = serde_json::from_str(&serialized).unwrap();
        assert_eq!(ns, deserialized);
    }

    #[test]
    fn test_subgraph_result_variants() {
        let completed = SubgraphResult::Completed(serde_json::json!("result"));
        let interrupted = SubgraphResult::Interrupted(InterruptRecord {
            interrupt_id: "int-1".to_string(),
            namespace: "ns-1".to_string(),
            task_id: "task-1".to_string(),
            node_name: "node-1".to_string(),
            step: 5,
            value: serde_json::json!("interrupt-value"),
        });
        let cancelled = SubgraphResult::Cancelled;
        let failed = SubgraphResult::Failed("error message".to_string());

        assert_eq!(completed, completed);
        assert_eq!(interrupted, interrupted);
        assert_eq!(cancelled, cancelled);
        assert_eq!(failed, failed);

        assert_ne!(completed, interrupted);
        assert_ne!(interrupted, cancelled);
        assert_ne!(cancelled, failed);
    }

    #[test]
    fn test_subgraph_result_equality() {
        let result1 = SubgraphResult::Completed(serde_json::json!("value"));
        let result2 = SubgraphResult::Completed(serde_json::json!("value"));
        assert_eq!(result1, result2);

        let result3 = SubgraphResult::Completed(serde_json::json!("other"));
        assert_ne!(result1, result3);
    }

    #[test]
    fn test_subgraph_invocation_fields() {
        let invocation = SubgraphInvocation {
            parent_task_id: "parent-task-1".to_string(),
            parent_checkpoint_id: Some("checkpoint-1".to_string()),
            child_namespace: CheckpointNamespace::root().child("child"),
            entry_input: serde_json::json!("input-data"),
        };

        assert_eq!(invocation.parent_task_id, "parent-task-1");
        assert_eq!(
            invocation.parent_checkpoint_id,
            Some("checkpoint-1".to_string())
        );
        assert_eq!(invocation.child_namespace.0, "root/child");
        assert_eq!(invocation.entry_input, serde_json::json!("input-data"));
    }

    #[test]
    fn test_subgraph_inviation_clone() {
        let invocation = SubgraphInvocation {
            parent_task_id: "parent-task-1".to_string(),
            parent_checkpoint_id: Some("checkpoint-1".to_string()),
            child_namespace: CheckpointNamespace::root().child("child"),
            entry_input: serde_json::json!("input-data"),
        };

        let cloned = invocation.clone();
        assert_eq!(invocation.parent_task_id, cloned.parent_task_id);
        assert_eq!(invocation.child_namespace, cloned.child_namespace);
        assert_eq!(invocation.entry_input, cloned.entry_input);
    }
}
