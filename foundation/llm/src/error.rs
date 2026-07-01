//! Error types for loom-llm.
//!
//! This module provides:
//! - `LlmError`: LLM-specific error type (invoke failures, empty responses, cancellation)
//! - Re-exports of `GraphError` and `Interrupt` from `loom-graph-core` for backward compatibility
//! - `From<LlmError> for GraphError` conversion for seamless `?` propagation

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use loom_graph_core::{GraphError, Interrupt};

/// LLM client error type.
///
/// Contains only LLM-specific error semantics. Converted to `GraphError`
/// automatically via `From<LlmError> for GraphError` when propagating
/// through graph execution (e.g., in ThinkNode).
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum LlmError {
    /// LLM invocation failed (network error, API error, parse failure, etc.).
    #[error("LLM invoke failed: {0}")]
    InvokeFailed(String),

    /// LLM returned empty response after all retries exhausted.
    #[error("LLM returned empty response after {retries} retries")]
    EmptyResponse { retries: u32 },

    /// LLM call was cancelled.
    #[error("LLM call cancelled")]
    Cancelled,
}

impl LlmError {
    /// Returns true if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            LlmError::InvokeFailed(_) | LlmError::EmptyResponse { .. }
        )
    }
}

/// Converts LLM errors to graph execution errors.
///
/// This allows `?` to automatically propagate `LlmError` as `GraphError`
/// in any function that returns `Result<_, GraphError>`.
impl From<LlmError> for GraphError {
    fn from(e: LlmError) -> Self {
        match e {
            LlmError::InvokeFailed(msg) => GraphError::ExecutionFailed(msg),
            LlmError::EmptyResponse { retries } => GraphError::ExecutionFailed(format!(
                "LLM returned empty response after {retries} retries"
            )),
            LlmError::Cancelled => GraphError::Cancelled,
        }
    }
}

/// Converts `reqwest::Error` to `LlmError` for client implementations
/// that return `Result<_, LlmError>` from `invoke()`.
///
/// Conversion chain for `?` propagation through graph nodes:
/// `reqwest::Error → LlmError → GraphError` (two `?` steps).
impl From<reqwest::Error> for LlmError {
    fn from(e: reqwest::Error) -> Self {
        LlmError::InvokeFailed(e.to_string())
    }
}

/// Deprecated alias for backward compatibility.
///
/// Use `loom_graph_core::GraphError` or `loom_llm::LlmError` instead.
#[deprecated(note = "split into loom_graph_core::GraphError and loom_llm::LlmError")]
pub type AgentError = GraphError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_error_invoke_failed_display() {
        let err = LlmError::InvokeFailed("timeout".to_string());
        let s = err.to_string();
        assert!(s.contains("invoke failed"), "Display: {}", s);
        assert!(s.contains("timeout"), "Display: {}", s);
    }

    #[test]
    fn llm_error_empty_response_display() {
        let err = LlmError::EmptyResponse { retries: 3 };
        let s = err.to_string();
        assert!(s.contains("empty response"), "Display: {}", s);
        assert!(s.contains("3"), "Display: {}", s);
    }

    #[test]
    fn llm_error_cancelled_display() {
        let err = LlmError::Cancelled;
        let s = err.to_string();
        assert!(s.contains("cancelled"), "Display: {}", s);
    }

    #[test]
    fn llm_error_is_retryable() {
        assert!(LlmError::InvokeFailed("x".into()).is_retryable());
        assert!(LlmError::EmptyResponse { retries: 0 }.is_retryable());
        assert!(!LlmError::Cancelled.is_retryable());
    }

    #[test]
    fn from_llm_error_to_graph_error() {
        let llm_err = LlmError::InvokeFailed("network".into());
        let graph_err = GraphError::from(llm_err);
        match graph_err {
            GraphError::ExecutionFailed(msg) => assert!(msg.contains("network")),
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }

        let llm_err = LlmError::EmptyResponse { retries: 5 };
        let graph_err = GraphError::from(llm_err);
        match graph_err {
            GraphError::ExecutionFailed(msg) => assert!(msg.contains("5 retries")),
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }

        let llm_err = LlmError::Cancelled;
        let graph_err = GraphError::from(llm_err);
        assert!(matches!(graph_err, GraphError::Cancelled));
    }
}
