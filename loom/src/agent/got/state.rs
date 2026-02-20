//! GoT (Graph of Thoughts) state: TaskGraph, TaskNode, TaskNodeState, GotState.
//!
//! Used by PlanGraphNode (writes `task_graph`) and ExecuteGraphNode (reads/writes
//! `node_states`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::state::ToolCall;

/// Execution status of a single task node in the DAG.
///
/// Written by ExecuteGraphNode: Pending until run, Running during execution,
/// Done or Failed when complete.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    /// Not yet executed.
    Pending,
    /// Currently executing (optional; may go straight to Done/Failed).
    Running,
    /// Completed successfully; `TaskNodeState::result` is set.
    Done,
    /// Execution failed; `TaskNodeState::error` is set.
    Failed,
}

/// One node in the task DAG: id, description, optional tool template.
///
/// Produced by PlanGraphNode from LLM output. ExecuteGraphNode runs each node
/// (e.g. via ReAct or single tool call) and writes result into `GotState::node_states`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    /// Unique id within the graph (e.g. "read_a", "merge").
    pub id: String,
    /// Human-readable description for the LLM/sub-task.
    pub description: String,
    /// Optional template or initial tool_calls for this sub-task.
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

/// DAG definition: nodes and directed edges (from_id, to_id).
///
/// Edges mean "from must complete before to". Used by ExecuteGraphNode for
/// topological order and ready set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskGraph {
    /// All task nodes.
    pub nodes: Vec<TaskNode>,
    /// (from_id, to_id): from must complete before to can run.
    pub edges: Vec<(String, String)>,
}

/// Runtime state for one task node: status, result, error.
///
/// Written by ExecuteGraphNode when a node is run. Read by merge/distill
/// nodes (multiple predecessors) and for streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNodeState {
    pub status: TaskStatus,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

impl Default for TaskNodeState {
    fn default() -> Self {
        Self {
            status: TaskStatus::Pending,
            result: None,
            error: None,
        }
    }
}

/// State for the GoT graph: task DAG and per-node execution state.
///
/// PlanGraphNode sets `task_graph`; ExecuteGraphNode reads `task_graph` and
/// updates `node_states`. Checkpointer serializes the full `GotState`.
///
/// **Interaction**: Flows through `StateGraph<GotState>`; see `crate::agent::got::runner`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GotState {
    /// User message used by PlanGraphNode to generate the DAG.
    #[serde(default)]
    pub input_message: String,
    /// Task DAG produced by PlanGraphNode.
    pub task_graph: TaskGraph,
    /// Per-node execution state (key = node id).
    #[serde(default)]
    pub node_states: HashMap<String, TaskNodeState>,
}

impl Default for GotState {
    fn default() -> Self {
        Self {
            input_message: String::new(),
            task_graph: TaskGraph::default(),
            node_states: HashMap::new(),
        }
    }
}

impl GotState {
    /// Returns a combined result string for display (e.g. last node's result or concatenation).
    ///
    /// Used by CLI/API to show final output when the graph has finished.
    pub fn summary_result(&self) -> String {
        let done: Vec<_> = self
            .node_states
            .iter()
            .filter(|(_, s)| s.status == TaskStatus::Done && s.result.is_some())
            .collect();
        if done.is_empty() {
            return String::new();
        }
        // Prefer nodes that are "sinks" (no outgoing edge) as final output.
        let from_ids: std::collections::HashSet<_> = self
            .task_graph
            .edges
            .iter()
            .map(|(from, _)| from.as_str())
            .collect();
        let sink_ids: std::collections::HashSet<_> = self
            .task_graph
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .filter(|id| !from_ids.contains(id))
            .collect();
        for (id, s) in &done {
            if sink_ids.contains(id.as_str()) {
                if let Some(ref r) = s.result {
                    return r.clone();
                }
            }
        }
        done.last()
            .and_then(|(_, s)| s.result.as_ref())
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str) -> TaskNode {
        TaskNode {
            id: id.to_string(),
            description: format!("desc-{id}"),
            tool_calls: vec![],
        }
    }

    #[test]
    fn task_node_state_default_is_pending_without_result() {
        let s = TaskNodeState::default();
        assert_eq!(s.status, TaskStatus::Pending);
        assert!(s.result.is_none());
        assert!(s.error.is_none());
    }

    #[test]
    fn got_state_default_is_empty() {
        let s = GotState::default();
        assert!(s.input_message.is_empty());
        assert!(s.task_graph.nodes.is_empty());
        assert!(s.task_graph.edges.is_empty());
        assert!(s.node_states.is_empty());
    }

    #[test]
    fn summary_result_returns_empty_when_no_done_nodes() {
        let s = GotState::default();
        assert!(s.summary_result().is_empty());
    }

    #[test]
    fn summary_result_prefers_sink_node_result() {
        let s = GotState {
            input_message: "q".to_string(),
            task_graph: TaskGraph {
                nodes: vec![node("a"), node("b")],
                edges: vec![("a".to_string(), "b".to_string())],
            },
            node_states: [
                (
                    "a".to_string(),
                    TaskNodeState {
                        status: TaskStatus::Done,
                        result: Some("from a".to_string()),
                        error: None,
                    },
                ),
                (
                    "b".to_string(),
                    TaskNodeState {
                        status: TaskStatus::Done,
                        result: Some("from b".to_string()),
                        error: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        };
        assert_eq!(s.summary_result(), "from b");
    }

    #[test]
    fn summary_result_falls_back_to_any_done_result_when_no_sink_has_result() {
        let s = GotState {
            input_message: "q".to_string(),
            task_graph: TaskGraph {
                nodes: vec![node("a")],
                edges: vec![("a".to_string(), "a".to_string())],
            },
            node_states: [(
                "a".to_string(),
                TaskNodeState {
                    status: TaskStatus::Done,
                    result: Some("fallback".to_string()),
                    error: None,
                },
            )]
            .into_iter()
            .collect(),
        };
        assert_eq!(s.summary_result(), "fallback");
    }
}
