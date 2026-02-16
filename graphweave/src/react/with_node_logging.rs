//! Extension trait for fluent API: attach node logging middleware then compile.
//!
//! Interacts with [`StateGraph`](crate::graph::StateGraph), [`ReActState`](crate::state::ReActState),
//! and [`LoggingNodeMiddleware`](crate::graph::LoggingNodeMiddleware).

use std::sync::Arc;

use crate::graph::{LoggingNodeMiddleware, StateGraph};
use crate::state::ReActState;

/// Extension trait for fluent API: attach node logging middleware then compile.
///
/// Returns the same graph with `LoggingNodeMiddleware` attached. Chain with `.compile()` or
/// `.compile_with_checkpointer()`.
pub trait WithNodeLogging {
    /// Returns the same graph with node logging middleware attached.
    fn with_node_logging(self) -> Self;
}

impl WithNodeLogging for StateGraph<ReActState> {
    fn with_node_logging(self) -> Self {
        self.with_middleware(Arc::new(LoggingNodeMiddleware::<ReActState>::default()))
    }
}
