//! Shared cancellation and operation tracking types.
//!
//! These types are used across multiple modules in loom and were originally
//! defined in cli_run/agent.rs. They are now in a separate module to avoid
//! circular dependencies when agent functionality moves to loom-agent.

use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Trait for cancelling active operations.
pub trait ActiveOperationCanceller: Send + Sync {
    /// Cancel the operation.
    fn cancel(&self);
}

/// Kind of active operation being tracked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveOperationKind {
    /// LLM request in progress.
    Llm,
    /// Background task/tool execution.
    ToolTask,
    /// MCP request in progress.
    McpRequest,
    /// Child process execution.
    ChildProcess,
}

/// An active operation that can be cancelled.
#[derive(Clone)]
pub struct ActiveOperation {
    kind: ActiveOperationKind,
    canceller: Arc<dyn ActiveOperationCanceller>,
}

impl ActiveOperation {
    /// Create a new active operation.
    pub fn new(kind: ActiveOperationKind, canceller: Arc<dyn ActiveOperationCanceller>) -> Self {
        Self { kind, canceller }
    }

    /// Get the operation kind.
    pub fn kind(&self) -> ActiveOperationKind {
        self.kind
    }

    /// Cancel the operation.
    pub fn cancel(&self) {
        self.canceller.cancel();
    }
}

impl fmt::Debug for ActiveOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActiveOperation")
            .field("kind", &self.kind)
            .finish()
    }
}

/// Internal state for cancellation tracking.
#[derive(Debug)]
struct CancellationState {
    active_operation: Mutex<Option<ActiveOperation>>,
}

/// Canceller based on a futures AbortHandle.
#[derive(Debug)]
struct AbortHandleCanceller {
    handle: futures_util::future::AbortHandle,
}

impl ActiveOperationCanceller for AbortHandleCanceller {
    fn cancel(&self) {
        self.handle.abort();
    }
}

/// Runtime cancellation handle for one run generation.
#[derive(Debug, Clone)]
pub struct RunCancellation {
    generation: u64,
    token: CancellationToken,
    state: Arc<CancellationState>,
}

impl RunCancellation {
    /// Create a new cancellation handle.
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            token: CancellationToken::new(),
            state: Arc::new(CancellationState {
                active_operation: Mutex::new(None),
            }),
        }
    }

    /// Get the generation number.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Get the cancellation token.
    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    /// Cancel the operation.
    pub fn cancel(&self) {
        self.token.cancel();
        self.cancel_active_operation();
    }

    /// Set the active operation being tracked.
    pub fn set_active_operation(&self, operation: ActiveOperation) {
        if let Ok(mut active_operation) = self.state.active_operation.lock() {
            *active_operation = Some(operation);
        }
    }

    /// Set an abortable operation.
    pub fn set_abortable_operation(
        &self,
        kind: ActiveOperationKind,
        handle: futures_util::future::AbortHandle,
    ) {
        self.set_active_operation(ActiveOperation::new(
            kind,
            Arc::new(AbortHandleCanceller { handle }),
        ));
    }

    /// Clear the active operation.
    pub fn clear_active_operation(&self) {
        if let Ok(mut active_operation) = self.state.active_operation.lock() {
            *active_operation = None;
        }
    }

    /// Get the kind of the active operation.
    pub fn active_operation_kind(&self) -> Option<ActiveOperationKind> {
        self.state
            .active_operation
            .lock()
            .ok()
            .and_then(|active_operation| active_operation.as_ref().map(ActiveOperation::kind))
    }

    /// Cancel the active operation.
    pub fn cancel_active_operation(&self) {
        let active_operation = self
            .state
            .active_operation
            .lock()
            .ok()
            .and_then(|active_operation| active_operation.clone());
        if let Some(active_operation) = active_operation {
            active_operation.cancel();
            self.clear_active_operation();
        }
    }

    /// Returns true if this run has been cancelled.
    ///
    /// Tools can check this during long-running operations to bail out early
    /// when the user presses Ctrl+C.
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}
