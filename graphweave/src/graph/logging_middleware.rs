//! Logging middleware that prints node enter/exit around each node.run call.
//!
//! Used by [`WithNodeLogging`](super::WithNodeLogging) and the ReAct runner.
//! Interacts with [`NodeMiddleware`](super::NodeMiddleware).

use async_trait::async_trait;
use std::fmt::Debug;
use std::pin::Pin;

use crate::error::AgentError;
use crate::graph::Next;

use super::NodeMiddleware;

/// Middleware that logs node enter/exit around each node.run call.
///
/// Logs to stderr so that normal output (e.g. Assistant messages) can be
/// redirected separately. Generic over state type `S`; only node_id is logged.
pub struct LoggingNodeMiddleware<S> {
    _phantom: std::marker::PhantomData<S>,
}

impl<S> Default for LoggingNodeMiddleware<S> {
    fn default() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<S> NodeMiddleware<S> for LoggingNodeMiddleware<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
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
    ) -> Result<(S, Next), AgentError> {
        eprintln!("[node] enter node={}", node_id);
        let result = inner(state).await;
        match &result {
            Ok((_, ref next)) => eprintln!("[node] exit node={} next={:?}", node_id, next),
            Err(e) => eprintln!("[node] exit node={} error={}", node_id, e),
        }
        result
    }
}
