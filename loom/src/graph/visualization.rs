//! Graph visualization utilities.
//!
//! Provides functionality to export graph structure to Graphviz DOT format
//! for visualization and debugging.

use std::fmt::Write;

use super::CompiledStateGraph;
use super::{END, START};

/// Generate Graphviz DOT format representation of the graph.
///
/// Returns a string in DOT format that can be rendered using Graphviz tools.
pub fn generate_dot<S>(graph: &CompiledStateGraph<S>) -> String
where
    S: std::fmt::Debug,
{
    let mut dot = String::from("digraph {\n");
    dot.push_str("  rankdir=LR;\n");
    dot.push_str("  node [shape=box];\n\n");

    // Add START and END nodes
    dot.push_str(&format!(
        "  \"{}\" [label=\"START\", style=bold, fillcolor=lightgreen];\n",
        START
    ));
    dot.push_str(&format!(
        "  \"{}\" [label=\"END\", style=bold, fillcolor=lightcoral];\n",
        END
    ));

    // Add regular nodes
    for (node_id, _) in &graph.nodes {
        dot.push_str(&format!("  \"{}\";\n", node_id));
    }

    dot.push_str("\n");

    // Add edges based on edge_order
    if !graph.edge_order.is_empty() {
        // Edge from START to first node
        dot.push_str(&format!(
            "  \"{}\" -> \"{}\";\n",
            START, graph.edge_order[0]
        ));

        // Edges between nodes
        for i in 1..graph.edge_order.len() {
            dot.push_str(&format!(
                "  \"{}\" -> \"{}\";\n",
                graph.edge_order[i - 1],
                graph.edge_order[i]
            ));
        }

        // Edge from last node to END
        if let Some(last_node) = graph.edge_order.last() {
            dot.push_str(&format!("  \"{}\" -> \"{}\";\n", last_node, END));
        }
    }

    dot.push_str("}\n");
    dot
}

/// Generate a simple text representation of the graph structure.
pub fn generate_text<S>(graph: &CompiledStateGraph<S>) -> String
where
    S: std::fmt::Debug,
{
    let mut text = String::new();
    writeln!(text, "Graph Structure:").unwrap();
    writeln!(text, "Nodes: {}", graph.nodes.len()).unwrap();

    writeln!(text, "\nExecution Order:").unwrap();
    writeln!(text, "  {} ->", START).unwrap();
    for (i, node_id) in graph.edge_order.iter().enumerate() {
        if i == graph.edge_order.len() - 1 {
            writeln!(text, "  {} -> {}", node_id, END).unwrap();
        } else {
            writeln!(text, "  {} ->", node_id).unwrap();
        }
    }

    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{NameNode, StateGraph};

    #[test]
    fn test_generate_dot() {
        let mut graph = StateGraph::<String>::new();
        graph.add_node("node1", std::sync::Arc::new(NameNode::new("node1")));
        graph.add_node("node2", std::sync::Arc::new(NameNode::new("node2")));
        graph.add_edge(crate::graph::START, "node1");
        graph.add_edge("node1", "node2");
        graph.add_edge("node2", crate::graph::END);

        let compiled = graph.compile().unwrap();
        let dot = generate_dot(&compiled);

        assert!(dot.contains("digraph"));
        assert!(dot.contains("START"));
        assert!(dot.contains("END"));
        assert!(dot.contains("node1"));
        assert!(dot.contains("node2"));
    }

    #[test]
    fn test_generate_text() {
        let mut graph = StateGraph::<String>::new();
        graph.add_node("node1", std::sync::Arc::new(NameNode::new("node1")));
        graph.add_edge(crate::graph::START, "node1");
        graph.add_edge("node1", crate::graph::END);

        let compiled = graph.compile().unwrap();
        let text = generate_text(&compiled);

        assert!(text.contains("Graph Structure"));
        assert!(text.contains(START)); // Use the constant directly
        assert!(text.contains(END)); // Use the constant directly
        assert!(text.contains("node1"));
    }
}
