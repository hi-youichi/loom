//! Unified error types for Loom and loom-llm.
//!
//! This module provides the shared error type used by both crates.

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

/// Backward compatibility alias.
#[deprecated(since = "0.1.0", note = "Use Interrupt directly")]
pub type GraphInterrupt = Interrupt;

/// Unified error type for Loom agent operations.
///
/// This error is used by:
/// - `loom`: All agent operations (LLM calls, tool execution, graph execution)
/// - `loom-llm`: LLM-specific operations
///
/// # Error Hierarchy
///
/// - `ExecutionFailed`: Generic execution errors (LLM call failed, tool error, etc.)
/// - `Cancelled`: Run was cancelled by runtime
/// - `Interrupted`: Graph execution was interrupted for human-in-the-loop
/// - `EmptyLlmResponse`: LLM returned empty response after all retries
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum AgentError {
    /// Execution failed with a message (e.g. LLM call failed, tool error).
    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    /// Run was cancelled by runtime.
    #[error("run cancelled")]
    Cancelled,

    /// Graph execution was interrupted for human-in-the-loop scenarios.
    ///
    /// Contains the interrupt value with optional ID for identifying specific interrupts.
    #[error("graph interrupted: {0}")]
    Interrupted(Interrupt),

    /// LLM returned empty response after all retries exhausted.
    #[error("LLM returned empty response after {retries} retries")]
    EmptyLlmResponse { retries: u32 },
}

impl AgentError {
    /// Returns true if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            AgentError::ExecutionFailed(_) | AgentError::EmptyLlmResponse { .. }
        )
    }

    /// Returns true if this error indicates an interrupt.
    pub fn is_interrupt(&self) -> bool {
        matches!(self, AgentError::Interrupted(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_error_display_execution_failed() {
        let err = AgentError::ExecutionFailed("msg".to_string());
        let s = err.to_string();
        assert!(s.contains("execution failed"), "Display should contain 'execution failed': {}", s);
        assert!(s.contains("msg"), "Display should contain message: {}", s);
    }

    #[test]
    fn agent_error_display_cancelled() {
        let err = AgentError::Cancelled;
        let s = err.to_string();
        assert!(s.contains("run cancelled"), "Display: {}", s);
    }

    #[test]
    fn agent_error_display_interrupted() {
        let interrupt = Interrupt::new(serde_json::json!({"action": "approve"}));
        let err = AgentError::Interrupted(interrupt);
        let s = err.to_string();
        assert!(s.contains("interrupt"), "Display: {}", s);
    }

    #[test]
    fn agent_error_display_empty_llm_response() {
        let err = AgentError::EmptyLlmResponse { retries: 3 };
        let s = err.to_string();
        assert!(s.contains("empty response"), "Display: {}", s);
        assert!(s.contains("3"), "Display should contain retry count: {}", s);
    }

    #[test]
    fn agent_error_is_retryable() {
        assert!(AgentError::ExecutionFailed("test".into()).is_retryable());
        assert!(AgentError::EmptyLlmResponse { retries: 1 }.is_retryable());
        assert!(!AgentError::Cancelled.is_retryable());
        assert!(!AgentError::Interrupted(Interrupt::new(serde_json::json!(null))).is_retryable());
    }

    #[test]
    fn agent_error_is_interrupt() {
        assert!(AgentError::Interrupted(Interrupt::new(serde_json::json!(null))).is_interrupt());
        assert!(!AgentError::Cancelled.is_interrupt());
        assert!(!AgentError::ExecutionFailed("test".into()).is_interrupt());
    }

    #[test]
    fn agent_error_serialize_deserialize() {
        let interrupt = Interrupt::with_id(
            serde_json::json!({"action": "approve"}),
            "int_123".to_string(),
        );
        let err = AgentError::Interrupted(interrupt);
        let json = serde_json::to_string(&err).unwrap();
        let back: AgentError = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, AgentError::Interrupted(_)));
    }

    #[test]
    fn interrupt_display() {
        let i = Interrupt::new(serde_json::json!({"x": 1}));
        assert!(i.to_string().contains("interrupt"));

        let i2 = Interrupt::with_id(serde_json::json!(null), "abc".into());
        assert!(i2.to_string().contains("abc"));
    }
}
