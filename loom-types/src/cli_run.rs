//! CLI run types shared between loom and loom-tools.
//!
//! These types are used for agent execution configuration and cancellation tracking.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Active operation kind for cancellation tracking.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActiveOperationKind {
    /// LLM request
    LlmRequest,
    /// MCP request
    McpRequest,
    /// Bash execution
    BashExecution,
    /// Child process
    ChildProcess,
    /// File operation
    FileOperation,
    /// Other operation
    Other(String),
}

/// Trait for cancelling active operations.
pub trait ActiveOperationCanceller: Send + Sync {
    /// Cancel the operation.
    fn cancel(&self);
}

/// Active operation with cancellation support.
#[derive(Clone)]
pub struct ActiveOperation {
    pub kind: ActiveOperationKind,
    pub canceller: Option<Arc<dyn ActiveOperationCanceller>>,
}

impl std::fmt::Debug for ActiveOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveOperation")
            .field("kind", &self.kind)
            .field("canceller", &self.canceller.as_ref().map(|_| "..."))
            .finish()
    }
}

impl ActiveOperation {
    pub fn new(kind: ActiveOperationKind, canceller: Arc<dyn ActiveOperationCanceller>) -> Self {
        Self {
            kind,
            canceller: Some(canceller),
        }
    }

    pub fn cancel(&self) {
        if let Some(canceller) = &self.canceller {
            canceller.cancel();
        }
    }
}

/// Active operation cancellation handle.
#[derive(Clone, Debug)]
pub struct RunCancellation {
    inner: Arc<std::sync::Mutex<RunCancellationInner>>,
}

#[derive(Clone, Debug)]
struct RunCancellationInner {
    cancelled: bool,
    abort_handle: Option<futures_util::future::AbortHandle>,
    active_operation: Option<ActiveOperation>,
}

impl RunCancellation {
    pub fn new(depth: u32) -> Self {
        Self {
            inner: Arc::new(std::sync::Mutex::new(RunCancellationInner {
                cancelled: false,
                abort_handle: None,
                active_operation: None,
            })),
        }
    }

    pub fn cancel(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.cancelled = true;
        if let Some(operation) = &inner.active_operation {
            operation.cancel();
        }
        if let Some(handle) = &inner.abort_handle {
            handle.abort();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.lock().unwrap().cancelled
    }

    pub fn set_abortable_operation(&self, kind: ActiveOperationKind, handle: futures_util::future::AbortHandle) {
        let mut inner = self.inner.lock().unwrap();
        inner.abort_handle = Some(handle);
    }

    pub fn set_active_operation(&self, operation: ActiveOperation) {
        let mut inner = self.inner.lock().unwrap();
        inner.active_operation = Some(operation);
    }

    pub fn active_operation_kind(&self) -> Option<ActiveOperationKind> {
        self.inner.lock().unwrap().active_operation.as_ref().map(|op| op.kind.clone())
    }
}

/// Stream event from agent runs - this is a placeholder type.
/// The actual implementation in loom defines variants for different agent types.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AnyStreamEvent {
    React(serde_json::Value),
    Dup(serde_json::Value),
    Tot(serde_json::Value),
    Got(serde_json::Value),
}
