//! Graph compilation error.
//!
//! Returned by `StateGraph::compile` when edges reference unknown nodes or
//! do not form a single linear chain from START to END.

use thiserror::Error;

/// Error when compiling a state graph (e.g. edge references unknown node, invalid chain).
///
/// Returned by `StateGraph::compile()`. Validation ensures every id in
/// edges (except START/END) exists in the node map and edges form exactly one
/// linear chain from START to END.
#[derive(Debug, Error)]
pub enum CompilationError {
    /// A node id in an edge was not registered via `add_node` (and is not START/END).
    #[error("node not found: {0}")]
    NodeNotFound(String),

    /// No edge has from_id == START, or more than one such edge.
    #[error("graph must have exactly one edge from START")]
    MissingStart,

    /// No edge has to_id == END, or more than one such edge.
    #[error("graph must have exactly one edge to END")]
    MissingEnd,

    /// Edges do not form a single linear chain (e.g. branch, cycle, disconnected).
    #[error("edges must form a single linear chain from START to END: {0}")]
    InvalidChain(String),

    /// A node has both an outgoing edge and conditional edges; it must have exactly one.
    #[error("node has both edge and conditional edges: {0}")]
    NodeHasBothEdgeAndConditional(String),

    /// A value in a conditional path_map is not a valid node id or END.
    #[error("conditional path_map invalid target: {0}")]
    InvalidConditionalPathMap(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Scenario**: Display of NodeNotFound contains "node not found" and the node id.
    #[test]
    fn compilation_error_display_node_not_found() {
        let err = CompilationError::NodeNotFound("x".to_string());
        let s = err.to_string();
        assert!(
            s.contains("node not found"),
            "Display should contain 'node not found': {}",
            s
        );
        assert!(s.contains("x"), "Display should contain node id: {}", s);
    }

    /// **Scenario**: Display of MissingStart contains "exactly one edge from START".
    #[test]
    fn compilation_error_display_missing_start() {
        let err = CompilationError::MissingStart;
        let s = err.to_string();
        assert!(
            s.to_lowercase().contains("start"),
            "Display should mention START: {}",
            s
        );
    }

    /// **Scenario**: Display of MissingEnd contains "exactly one edge to END".
    #[test]
    fn compilation_error_display_missing_end() {
        let err = CompilationError::MissingEnd;
        let s = err.to_string();
        assert!(
            s.to_lowercase().contains("end"),
            "Display should mention END: {}",
            s
        );
    }

    /// **Scenario**: Display of InvalidChain contains "single linear chain" and the reason.
    #[test]
    fn compilation_error_display_invalid_chain() {
        let err = CompilationError::InvalidChain("reason".to_string());
        let s = err.to_string();
        assert!(
            s.contains("single linear chain") || s.contains("linear chain"),
            "Display should contain chain message: {}",
            s
        );
        assert!(s.contains("reason"), "Display should contain reason: {}", s);
    }
}
