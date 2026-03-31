//! Extension trait for fluent API: attach node logging middleware then compile.

use std::sync::Arc;

use crate::graph::{LoggingNodeMiddleware, StateGraph};
use crate::state::ReActState;

pub trait WithNodeLogging {
    fn with_node_logging(self) -> Self;
}

impl WithNodeLogging for StateGraph<ReActState> {
    fn with_node_logging(self) -> Self {
        self.with_middleware(Arc::new(LoggingNodeMiddleware::<ReActState>::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_node_logging_returns_state_graph() {
        let graph: StateGraph<ReActState> = StateGraph::new();
        let _result = graph.with_node_logging();
    }
}
