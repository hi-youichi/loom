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
        assert_eq!(err.to_string(), "node not found: x");
    }

    /// **Scenario**: Display of MissingStart contains expected text.
    #[test]
    fn compilation_error_display_missing_start() {
        let err = CompilationError::MissingStart;
        assert_eq!(
            err.to_string(),
            "graph must have exactly one edge from START"
        );
    }

    /// **Scenario**: Display of MissingEnd contains expected text.
    #[test]
    fn compilation_error_display_missing_end() {
        let err = CompilationError::MissingEnd;
        assert_eq!(err.to_string(), "graph must have exactly one edge to END");
    }

    /// **Scenario**: Display of InvalidChain contains expected text.
    #[test]
    fn compilation_error_display_invalid_chain() {
        let err = CompilationError::InvalidChain("cycle detected".to_string());
        assert_eq!(
            err.to_string(),
            "edges must form a single linear chain from START to END: cycle detected"
        );
    }

    /// **Scenario**: Display of NodeHasBothEdgeAndConditional contains node id.
    #[test]
    fn compilation_error_display_node_has_both() {
        let err = CompilationError::NodeHasBothEdgeAndConditional("node1".to_string());
        assert_eq!(
            err.to_string(),
            "node has both edge and conditional edges: node1"
        );
    }

    /// **Scenario**: Display of InvalidConditionalPathMap contains invalid target.
    #[test]
    fn compilation_error_display_invalid_conditional() {
        let err = CompilationError::InvalidConditionalPathMap("invalid_node".to_string());
        assert_eq!(
            err.to_string(),
            "conditional path_map invalid target: invalid_node"
        );
    }
}
