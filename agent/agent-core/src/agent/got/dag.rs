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
/// **Interaction**: Currently only exercised by unit tests.  Kept as a reusable
/// utility for future DAG consumers and to document the topological ordering
/// invariant used elsewhere in the module.
#[cfg(test)]
fn topological_sort(graph: &TaskGraph) -> Option<Vec<String>> {
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

/// Returns node ids that have no unsatisfied dependencies (all incoming edges
/// point to completed nodes).
///
/// Returns the predecessor node ids for a given node (incoming edges).
pub fn predecessors(graph: &TaskGraph, node_id: &str) -> Vec<String> {
    graph
        .edges
        .iter()
        .filter(|(_, to)| to == node_id)
        .map(|(from, _)| from.clone())
        .collect()
}

/// Returns node ids that are ready to run: all predecessors are Done.
/// Only considers nodes that exist in `node_states` with status Pending.
pub fn ready_nodes(graph: &TaskGraph, node_states: &HashMap<String, TaskNodeState>) -> Vec<String> {
    graph
        .nodes
        .iter()
        .filter(|n| {
            let Some(node_state) = node_states.get(&n.id) else {
                return false;
            };
            if !matches!(node_state.status, TaskStatus::Pending) {
                return false;
            }
            graph.edges.iter().all(|(from, to)| {
                if to != &n.id {
                    return true;
                }
                graph
                    .nodes
                    .iter()
                    .find(|m| &m.id == from)
                    .and_then(|m| node_states.get(&m.id))
                    .map(|s| matches!(s.status, TaskStatus::Done))
                    .unwrap_or(false)
            })
        })
        .map(|n| n.id.clone())
        .collect()
}

/// Appends a new subgraph to an existing TaskGraph, returning the updated graph.
///
/// Validates that `subgraph` has no edges pointing outside itself (all edges
/// reference nodes within `subgraph`), and that no `new_ids` collide with existing ids.
///
/// # Errors
/// Returns [`AppendSubgraphError`] if:
/// - An edge in `subgraph` references a node not in `subgraph`
/// - A `new_id` collides with an existing node id in `graph`
pub fn append_subgraph(
    graph: &TaskGraph,
    subgraph: TaskGraph,
    parent_id: &str,
) -> Result<TaskGraph, AppendSubgraphError> {
    let existing_ids: HashSet<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
    let sub_ids: HashSet<String> = subgraph.nodes.iter().map(|n| n.id.clone()).collect();

    // Validate no edge references outside subgraph (parent_id and existing graph nodes are allowed as sources)
    for (from, to) in &subgraph.edges {
        if !sub_ids.contains(from) && !existing_ids.contains(from) && from != parent_id {
            return Err(AppendSubgraphError::InvalidEdge(format!(
                "edge source '{from}' not in subgraph, existing nodes, or parent"
            )));
        }
        if !sub_ids.contains(to) {
            return Err(AppendSubgraphError::InvalidEdge(format!(
                "edge target '{to}' not in subgraph nodes"
            )));
        }
    }

    // Validate no id collision
    for node in &subgraph.nodes {
        if existing_ids.contains(&node.id) {
            return Err(AppendSubgraphError::IdCollision(node.id.clone()));
        }
    }

    // Prefix all new node ids with parent_id to avoid collisions
    let prefixed_nodes: Vec<TaskNode> = subgraph
        .nodes
        .into_iter()
        .map(|mut n| {
            n.id = format!("{parent_id}_{}", n.id);
            n
        })
        .collect();

    let prefixed_edges: Vec<(String, String)> = subgraph
        .edges
        .into_iter()
        .map(|(from, to)| {
            let prefixed_from = if existing_ids.contains(&from) {
                from
            } else {
                format!("{parent_id}_{from}")
            };
            let prefixed_to = format!("{parent_id}_{to}");
            (prefixed_from, prefixed_to)
        })
        .collect();

    let mut new_graph = graph.clone();
    new_graph.nodes.extend(prefixed_nodes);
    new_graph.edges.extend(prefixed_edges);

    Ok(new_graph)
}

#[derive(Debug, thiserror::Error)]
pub enum AppendSubgraphError {
    #[error("subgraph edge references node outside subgraph: {0}")]
    InvalidEdge(String),
    #[error("node id '{0}' already exists in the graph")]
    IdCollision(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_graph(nodes: Vec<(&str, TaskStatus)>, edges: Vec<(&str, &str)>) -> (TaskGraph, HashMap<String, TaskNodeState>) {
        let nodes_out: Vec<TaskNode> = nodes
            .iter()
            .map(|(id, _status)| TaskNode { id: (*id).to_string(), description: "".to_string(), tool_calls: vec![] })
            .collect();
        let node_states_out: HashMap<String, TaskNodeState> = nodes
            .into_iter()
            .map(|(id, status)| (id.to_string(), TaskNodeState { status, result: None, error: None }))
            .collect();
        let graph = TaskGraph {
            nodes: nodes_out,
            edges: edges.into_iter().map(|(f, t)| (f.to_string(), t.to_string())).collect(),
        };
        (graph, node_states_out)
    }

    #[test]
    fn topo_sort_linear() {
        let (g, _states) = make_graph(
            vec![("a", TaskStatus::Pending), ("b", TaskStatus::Pending), ("c", TaskStatus::Pending)],
            vec![("a", "b"), ("b", "c")],
        );
        let order = topological_sort(&g).unwrap();
        assert!(order.iter().position(|x| x == "a") < order.iter().position(|x| x == "b"));
        assert!(order.iter().position(|x| x == "b") < order.iter().position(|x| x == "c"));
    }

    #[test]
    fn topo_sort_cycle() {
        let (g, _states) = make_graph(
            vec![("a", TaskStatus::Pending), ("b", TaskStatus::Pending)],
            vec![("a", "b"), ("b", "a")],
        );
        assert!(topological_sort(&g).is_none());
    }

    #[test]
    fn ready_nodes_none_ready() {
        let (g, states) = make_graph(
            vec![("a", TaskStatus::Pending), ("b", TaskStatus::Pending)],
            vec![("a", "b")],
        );
        assert_eq!(ready_nodes(&g, &states), vec!["a"]);
    }

    #[test]
    fn ready_nodes_partial() {
        let (g, states) = make_graph(
            vec![
                ("a", TaskStatus::Done),
                ("b", TaskStatus::Pending),
                ("c", TaskStatus::Pending),
            ],
            vec![("a", "b"), ("b", "c")],
        );
        assert_eq!(ready_nodes(&g, &states), vec!["b"]);
    }

    #[test]
    fn append_subgraph_ok() {
        let (g, _states) = make_graph(vec![("root", TaskStatus::Pending)], vec![]);
        let (sub, _) = make_graph(
            vec![("step1", TaskStatus::Pending), ("step2", TaskStatus::Pending)],
            vec![("step1", "step2")],
        );
        let result = append_subgraph(&g, sub, "root").unwrap();
        let ids: Vec<_> = result.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"root"));
        assert!(ids.contains(&"root_step1"));
        assert!(ids.contains(&"root_step2"));
    }

    #[test]
    fn append_subgraph_collision() {
        let (g, _states) = make_graph(vec![("x", TaskStatus::Pending)], vec![]);
        let (sub, _) = make_graph(vec![("x", TaskStatus::Pending)], vec![]);
        let err = append_subgraph(&g, sub, "p").unwrap_err();
        assert!(matches!(err, AppendSubgraphError::IdCollision(ref s) if s == "x"));
    }
}