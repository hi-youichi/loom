//! Node middleware: wrap node.run with external async logic (around pattern).
//!
//! Set via `StateGraph::with_middleware` for fluent API, or pass to
//! `compile_with_middleware` / `compile_with_checkpointer_and_middleware`.

use async_trait::async_trait;
use std::fmt::Debug;
use std::pin::Pin;

use crate::error::AgentError;

use super::Next;

/// Async middleware that wraps node.run; implemented externally.
///
/// Can wrap `inner` calls; decide when to call, retry, modify results, etc.
#[async_trait]
pub trait NodeMiddleware<S>: Send + Sync
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Wraps node.run: wraps the inner call.
    ///
    /// - `node_id`: current node id
    /// - `state`: state passed to the node
    /// - `inner`: actual node.run logic, must be called to execute the node
    async fn around_run(
        &self,
        node_id: &str,
        state: S,
        inner: Box<
            dyn FnOnce(
                    S,
                ) -> Pin<
                    Box<dyn std::future::Future<Output = Result<(S, Next), AgentError>> + Send>,
                > + Send,
        >,
    ) -> Result<(S, Next), AgentError>;
}
