//! Graph execution error types.
//!
//! This module defines the unified error type for graph execution operations,
//! including node execution, cancellation, and human-in-the-loop interrupts.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Interrupt value that can be raised during graph execution.
///
/// When a node raises an interrupt, execution pauses and can be resumed
/// after handling the interrupt (e.g., getting user input).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interrupt {
    /// The interrupt value (can be any JSON-serializable data).
    pub value: serde_json::Value,
    /// Optional interrupt ID for identifying specific interrupts.
    #[serde(default)]
    pub id: Option<String>,
}

impl Interrupt {
    /// Creates a new interrupt with a value.
    pub fn new(value: serde_json::Value) -> Self {
        Self { value, id: None }
    }

    /// Creates a new interrupt with a value and ID.
    pub fn with_id(value: serde_json::Value, id: String) -> Self {
        Self {
            value,
            id: Some(id),
        }
    }
}

impl std::fmt::Display for Interrupt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.id {
            Some(id) => write!(f, "interrupt(id={}): {}", id, self.value),
            None => write!(f, "interrupt: {}", self.value),
        }
    }
}

/// Graph execution error type.
///
/// Used by `Node::run()`, `CompiledStateGraph`, Pregel runtime, etc.
/// Corresponds to LangGraph's `GraphBubbleUp` + `NodeError` concepts.
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum GraphError {
    /// Node execution failed (LLM call failed, tool error, subgraph error, etc.).
    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    /// Run was cancelled by runtime.
    #[error("run cancelled")]
    Cancelled,

    /// Graph execution was interrupted for human-in-the-loop scenarios.
    #[error("graph interrupted: {0}")]
    Interrupted(Interrupt),
}

impl GraphError {
    /// Returns true if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(self, GraphError::ExecutionFailed(_))
    }

    /// Returns true if this error indicates an interrupt.
    pub fn is_interrupt(&self) -> bool {
        matches!(self, GraphError::Interrupted(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_error_display_execution_failed() {
        let err = GraphError::ExecutionFailed("msg".to_string());
        let s = err.to_string();
        assert!(s.contains("execution failed"), "Display: {}", s);
        assert!(s.contains("msg"), "Display: {}", s);
    }

    #[test]
    fn graph_error_display_cancelled() {
        let err = GraphError::Cancelled;
        let s = err.to_string();
        assert!(s.contains("run cancelled"), "Display: {}", s);
    }

    #[test]
    fn graph_error_display_interrupted() {
        let interrupt = Interrupt::new(serde_json::json!({"action": "approve"}));
        let err = GraphError::Interrupted(interrupt);
        let s = err.to_string();
        assert!(s.contains("interrupt"), "Display: {}", s);
    }

    #[test]
    fn interrupt_new() {
        let interrupt = Interrupt::new(serde_json::json!({"action": "approve"}));
        assert_eq!(interrupt.value, serde_json::json!({"action": "approve"}));
        assert_eq!(interrupt.id, None);
    }

    #[test]
    fn interrupt_with_id() {
        let interrupt = Interrupt::with_id(
            serde_json::json!({"action": "approve"}),
            "interrupt_1".to_string(),
        );
        assert_eq!(interrupt.id, Some("interrupt_1".to_string()));
    }
}
