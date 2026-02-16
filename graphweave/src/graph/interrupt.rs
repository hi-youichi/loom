//! Interrupt mechanism for graph execution.
//!
//! Provides support for interrupting graph execution, useful for human-in-the-loop
//! scenarios where execution needs to pause for user input or approval.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::error::AgentError;

/// Interrupt value that can be raised during graph execution.
///
/// When a node raises an interrupt, execution pauses and can be resumed
/// after handling the interrupt (e.g., getting user input).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interrupt {
    /// The interrupt value (can be any JSON-serializable data).
    pub value: serde_json::Value,
    /// Optional interrupt ID for identifying specific interrupts.
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

/// Error raised when a graph is interrupted.
///
/// This error should be caught by the graph executor to handle the interrupt.
#[derive(Debug, Clone, Error)]
#[error("Graph interrupted: {0:?}")]
pub struct GraphInterrupt(pub Interrupt);

impl From<Interrupt> for GraphInterrupt {
    fn from(interrupt: Interrupt) -> Self {
        GraphInterrupt(interrupt)
    }
}

/// Trait for handling interrupts during graph execution.
///
/// Implement this trait to define custom interrupt handling logic.
pub trait InterruptHandler: Send + Sync {
    /// Handle an interrupt and return a value to continue execution.
    ///
    /// This method is called when an interrupt is raised. The handler can
    /// perform actions like prompting the user, logging, or modifying state,
    /// then return a value that will be used to continue execution.
    fn handle_interrupt(&self, interrupt: &Interrupt) -> Result<serde_json::Value, AgentError>;
}

/// Default interrupt handler that returns the interrupt value as-is.
#[derive(Debug, Clone)]
pub struct DefaultInterruptHandler;

impl InterruptHandler for DefaultInterruptHandler {
    fn handle_interrupt(&self, interrupt: &Interrupt) -> Result<serde_json::Value, AgentError> {
        Ok(interrupt.value.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interrupt_new() {
        let interrupt = Interrupt::new(serde_json::json!({"action": "approve"}));
        assert_eq!(interrupt.value, serde_json::json!({"action": "approve"}));
        assert_eq!(interrupt.id, None);
    }

    #[test]
    fn test_interrupt_with_id() {
        let interrupt = Interrupt::with_id(
            serde_json::json!({"action": "approve"}),
            "interrupt_1".to_string(),
        );
        assert_eq!(interrupt.value, serde_json::json!({"action": "approve"}));
        assert_eq!(interrupt.id, Some("interrupt_1".to_string()));
    }

    #[test]
    fn test_graph_interrupt_from_interrupt() {
        let interrupt = Interrupt::new(serde_json::json!({"action": "approve"}));
        let graph_interrupt = GraphInterrupt::from(interrupt);
        assert_eq!(
            graph_interrupt.0.value,
            serde_json::json!({"action": "approve"})
        );
    }

    #[test]
    fn test_default_interrupt_handler() {
        let handler = DefaultInterruptHandler;
        let interrupt = Interrupt::new(serde_json::json!({"action": "approve"}));
        let result = handler.handle_interrupt(&interrupt).unwrap();
        assert_eq!(result, serde_json::json!({"action": "approve"}));
    }
}
