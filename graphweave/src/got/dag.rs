//! DAG utilities for GoT: topological sort, ready nodes, and subgraph append.
//!
//! Used by ExecuteGraphNode to determine execution order and which nodes
//! can run in parallel. `append_subgraph` enables AGoT dynamic DAG extension.

use std::collections::{HashMap, HashSet};

use super::state::{TaskGraph, TaskNode, TaskNodeState, TaskStatus};

/// Computes a topological order of node ids.
///
/// Returns an ordering such that for every edge (u, v), u appears before v.
/// If the graph has a cycle, returns None. Nodes with no edges appear in
/// arbitrary order relative to each other.
///
/// **Interaction**: Called by `ready_nodes`, ExecuteGraphNode, and
/// `append_subgraph` to decide execution order and validate DAG.
pub fn topological_sort(graph: &TaskGraph) -> Option<Vec<String>> {
    let ids: HashSet<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
    let mut in_degree: HashMap<String, usize> = ids.iter().cloned().map(|id| (id, 0)).collect();
    let mut out_edges: HashMap<String, Vec<String>> =
        ids.iter().cloned().map(|id| (id, vec![])).collect();

    for (from, to) in &graph.edges {
        if !ids.contains(from) || !ids.contains(to) {
            continue;
        }
        out_edges.get_mut(from).unwrap().push(to.clone());
        *in_degree.get_mut(to).unwrap() += 1;
    }

    let mut queue: Vec<String> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut order = Vec::with_capacity(ids.len());
    while let Some(u) = queue.pop() {
        order.push(u.clone());
        for v in out_edges.remove(&u).unwrap_or_default() {
            let d = in_degree.get_mut(&v).unwrap();
            *d -= 1;
            if *d == 0 {
                queue.push(v);
            }
        }
    }

    if order.len() == ids.len() {
        Some(order)
    } else {
        None
    }
}

/// Returns the list of predecessor node ids for the given node.
///
/// Predecessors are nodes that have an edge (pred, node_id) in the graph.
/// Used to collect their Done results for predecessor context passing into sub-tasks.
///
/// **Interaction**: Called by ExecuteGraphNode when building the sub-task user message.
pub fn predecessors(graph: &TaskGraph, node_id: &str) -> Vec<String> {
    graph
        .edges
        .iter()
        .filter(|(_, to)| to == node_id)
        .map(|(from, _)| from.clone())
        .collect()
}

/// Returns node ids that are ready to run: all predecessors are Done.
///
/// A node is ready when every edge (pred, node_id) has node_states[pred].status == Done.
/// Nodes with no predecessors are always ready (until run). Used by ExecuteGraphNode
/// to pick the next node(s) to execute; multiple ready nodes can be run in parallel.
///
/// **Interaction**: Called each time ExecuteGraphNode runs; reads `task_graph` and
/// `node_states`.
pub fn ready_nodes(graph: &TaskGraph, node_states: &HashMap<String, TaskNodeState>) -> Vec<String> {
    let done: HashSet<&str> = node_states
        .iter()
        .filter(|(_, s)| s.status == TaskStatus::Done)
        .map(|(id, _)| id.as_str())
        .collect();

    let mut preds: HashMap<&str, Vec<&str>> = HashMap::new();
    for (from, to) in &graph.edges {
        preds.entry(to.as_str()).or_default().push(from.as_str());
    }

    graph
        .nodes
        .iter()
        .map(|n| n.id.as_str())
        .filter(|&id| {
            if node_states.contains_key(id) {
                let s = &node_states[id];
                if s.status == TaskStatus::Done || s.status == TaskStatus::Failed {
                    return false;
                }
            }
            let pre = preds.get(id).map(|v| v.as_slice()).unwrap_or(&[]);
            pre.iter().all(|p| done.contains(*p))
        })
        .map(|s| s.to_string())
        .collect()
}

/// Error returned when appending a subgraph would violate DAG invariants.
///
/// Used by `append_subgraph` when validation fails. Prevents invalid graph state.
#[derive(Debug, Clone, thiserror::Error)]
pub enum AppendSubgraphError {
    /// Appending the given nodes and edges would create a cycle.
    #[error("appending would create a cycle")]
    WouldCreateCycle,
    /// A new node has an id that already exists in the graph.
    #[error("duplicate node id: {0}")]
    DuplicateNodeId(String),
}

/// Appends a subgraph (new nodes and edges) to the task graph.
///
/// Validates: (1) no new node id duplicates an existing id; (2) after append,
/// the graph remains a DAG (no cycles). Returns `Err` without mutating if
/// validation fails. Edges may reference existing nodes (e.g. parent) and new
/// nodes; used by AGoT dynamic expansion.
///
/// **Interaction**: Called by `crate::got::adaptive::maybe_expand` when
/// dynamically extending the graph after a node completes.
pub fn append_subgraph(
    graph: &mut TaskGraph,
    nodes: Vec<TaskNode>,
    edges: Vec<(String, String)>,
) -> Result<(), AppendSubgraphError> {
    let existing_ids: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    for n in &nodes {
        if existing_ids.contains(n.id.as_str()) {
            return Err(AppendSubgraphError::DuplicateNodeId(n.id.clone()));
        }
    }

    let mut combined = graph.clone();
    combined.nodes.extend(nodes);
    combined.edges.extend(edges);

    if topological_sort(&combined).is_none() {
        return Err(AppendSubgraphError::WouldCreateCycle);
    }

    *graph = combined;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, desc: &str) -> TaskNode {
        TaskNode {
            id: id.to_string(),
            description: desc.to_string(),
            tool_calls: vec![],
        }
    }

    /// **Scenario**: Linear chain A → B → C has unique topological order.
    #[test]
    fn topological_sort_linear() {
        let graph = TaskGraph {
            nodes: vec![node("a", ""), node("b", ""), node("c", "")],
            edges: vec![("a".into(), "b".into()), ("b".into(), "c".into())],
        };
        let order = topological_sort(&graph).unwrap();
        assert_eq!(order, ["a", "b", "c"]);
    }

    /// **Scenario**: Parallel branches: A,B have no deps, C depends on A and B.
    #[test]
    fn topological_sort_parallel() {
        let graph = TaskGraph {
            nodes: vec![node("a", ""), node("b", ""), node("c", "")],
            edges: vec![("a".into(), "c".into()), ("b".into(), "c".into())],
        };
        let order = topological_sort(&graph).unwrap();
        assert_eq!(order.len(), 3);
        let pos_c = order.iter().position(|x| x == "c").unwrap();
        let pos_a = order.iter().position(|x| x == "a").unwrap();
        let pos_b = order.iter().position(|x| x == "b").unwrap();
        assert!(pos_a < pos_c);
        assert!(pos_b < pos_c);
    }

    /// **Scenario**: Cycle returns None.
    #[test]
    fn topological_sort_cycle() {
        let graph = TaskGraph {
            nodes: vec![node("a", ""), node("b", "")],
            edges: vec![("a".into(), "b".into()), ("b".into(), "a".into())],
        };
        assert!(topological_sort(&graph).is_none());
    }

    /// **Scenario**: predecessors returns empty for node with no incoming edges.
    #[test]
    fn predecessors_none() {
        let graph = TaskGraph {
            nodes: vec![node("a", ""), node("b", "")],
            edges: vec![("a".into(), "b".into())],
        };
        assert_eq!(predecessors(&graph, "a"), [] as [String; 0]);
        assert_eq!(predecessors(&graph, "c"), [] as [String; 0]);
    }

    /// **Scenario**: predecessors returns single predecessor.
    #[test]
    fn predecessors_single() {
        let graph = TaskGraph {
            nodes: vec![node("a", ""), node("b", "")],
            edges: vec![("a".into(), "b".into())],
        };
        let preds = predecessors(&graph, "b");
        assert_eq!(preds.len(), 1);
        assert!(preds.contains(&"a".to_string()));
    }

    /// **Scenario**: predecessors returns multiple predecessors.
    #[test]
    fn predecessors_multiple() {
        let graph = TaskGraph {
            nodes: vec![node("a", ""), node("b", ""), node("c", "")],
            edges: vec![("a".into(), "c".into()), ("b".into(), "c".into())],
        };
        let preds = predecessors(&graph, "c");
        assert_eq!(preds.len(), 2);
        assert!(preds.contains(&"a".to_string()));
        assert!(preds.contains(&"b".to_string()));
    }

    /// **Scenario**: ready_nodes returns nodes with all predecessors Done.
    #[test]
    fn ready_nodes_after_predecessors_done() {
        let graph = TaskGraph {
            nodes: vec![node("a", ""), node("b", ""), node("c", "")],
            edges: vec![("a".into(), "c".into()), ("b".into(), "c".into())],
        };
        let mut node_states = HashMap::new();
        assert_eq!(
            ready_nodes(&graph, &node_states),
            ["a", "b"],
            "a and b have no preds"
        );

        node_states.insert(
            "a".into(),
            TaskNodeState {
                status: TaskStatus::Done,
                result: Some("r1".into()),
                error: None,
            },
        );
        assert_eq!(ready_nodes(&graph, &node_states), ["b"], "only b ready");

        node_states.insert(
            "b".into(),
            TaskNodeState {
                status: TaskStatus::Done,
                result: Some("r2".into()),
                error: None,
            },
        );
        assert_eq!(
            ready_nodes(&graph, &node_states),
            ["c"],
            "c ready after a,b done"
        );

        node_states.insert(
            "c".into(),
            TaskNodeState {
                status: TaskStatus::Done,
                result: Some("r3".into()),
                error: None,
            },
        );
        assert!(
            ready_nodes(&graph, &node_states).is_empty(),
            "none ready when all done"
        );
    }

    /// **Scenario**: append_subgraph adds nodes and edges; topology and ready_nodes stay correct.
    #[test]
    fn append_subgraph_valid() {
        let mut graph = TaskGraph {
            nodes: vec![node("a", "A"), node("b", "B")],
            edges: vec![("a".into(), "b".into())],
        };
        let new_nodes = vec![node("c", "C"), node("d", "D")];
        let new_edges = vec![("b".into(), "c".into()), ("b".into(), "d".into())];
        append_subgraph(&mut graph, new_nodes, new_edges).unwrap();
        assert_eq!(graph.nodes.len(), 4);
        assert_eq!(graph.edges.len(), 3);
        let order = topological_sort(&graph).unwrap();
        assert_eq!(order.len(), 4);
        let pos_a = order.iter().position(|x| x == "a").unwrap();
        let pos_b = order.iter().position(|x| x == "b").unwrap();
        let pos_c = order.iter().position(|x| x == "c").unwrap();
        let pos_d = order.iter().position(|x| x == "d").unwrap();
        assert!(pos_a < pos_b);
        assert!(pos_b < pos_c);
        assert!(pos_b < pos_d);

        let mut node_states = HashMap::new();
        node_states.insert(
            "a".into(),
            TaskNodeState {
                status: TaskStatus::Done,
                result: Some("r1".into()),
                error: None,
            },
        );
        node_states.insert(
            "b".into(),
            TaskNodeState {
                status: TaskStatus::Done,
                result: Some("r2".into()),
                error: None,
            },
        );
        let ready = ready_nodes(&graph, &node_states);
        assert_eq!(ready.len(), 2);
        assert!(ready.contains(&"c".to_string()));
        assert!(ready.contains(&"d".to_string()));
    }

    /// **Scenario**: append_subgraph rejects cycle (new edge d → a would create cycle).
    #[test]
    fn append_subgraph_rejects_cycle() {
        let mut graph = TaskGraph {
            nodes: vec![node("a", "A"), node("b", "B")],
            edges: vec![("a".into(), "b".into())],
        };
        let new_nodes = vec![node("c", "C")];
        let new_edges = vec![("b".into(), "c".into()), ("c".into(), "a".into())];
        let err = append_subgraph(&mut graph, new_nodes, new_edges).unwrap_err();
        assert!(matches!(err, AppendSubgraphError::WouldCreateCycle));
        assert_eq!(graph.nodes.len(), 2);
    }

    /// **Scenario**: append_subgraph rejects duplicate node id.
    #[test]
    fn append_subgraph_rejects_duplicate_id() {
        let mut graph = TaskGraph {
            nodes: vec![node("a", "A")],
            edges: vec![],
        };
        let new_nodes = vec![node("a", "dup")];
        let err = append_subgraph(&mut graph, new_nodes, vec![]).unwrap_err();
        assert!(matches!(err, AppendSubgraphError::DuplicateNodeId(id) if id == "a"));
    }
}
