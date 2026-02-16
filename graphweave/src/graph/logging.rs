//! Logging utilities for graph execution.
//!
//! Provides structured logging for graph execution events, node execution,
//! state updates, and other important events.

use std::fmt::Debug;

/// Log node execution start.
///
/// This should be called when a node starts executing.
pub fn log_node_start(node_id: &str) {
    tracing::debug!(node_id = node_id, "Starting node execution");
}

/// Log the state at the start of node execution.
///
/// Call this when a node is about to run so that each node execution
/// logs the current state (input state for that node).
pub fn log_node_state<S: Debug>(node_id: &str, state: &S) {
    tracing::debug!(node_id = node_id, state = ?state, "Node execution: state");
}

/// Log node execution completion.
///
/// This should be called when a node completes execution.
pub fn log_node_complete(node_id: &str, next: &crate::graph::Next) {
    tracing::debug!(node_id = node_id, ?next, "Node execution complete");
}

/// Log state update.
///
/// This should be called when state is updated after node execution.
pub fn log_state_update(node_id: &str) {
    tracing::debug!(node_id = node_id, "State updated");
}

/// Log graph execution start.
pub fn log_graph_start() {
    tracing::info!("Starting graph execution");
}

/// Log graph execution completion.
pub fn log_graph_complete() {
    tracing::info!("Graph execution complete");
}

/// Log graph execution error.
pub fn log_graph_error(error: &crate::error::AgentError) {
    tracing::error!(?error, "Graph execution error");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_functions() {
        // These should not panic
        log_node_start("test_node");
        log_node_state("test_node", &());
        log_node_complete("test_node", &crate::graph::Next::End);
        log_state_update("test_node");
        log_graph_start();
        log_graph_complete();
        log_graph_error(&crate::error::AgentError::ExecutionFailed(
            "test".to_string(),
        ));
    }
}
