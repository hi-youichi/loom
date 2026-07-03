//! Middleware that assembles the session title from a shared slot into state.
//!
//! Used together with [`TitleNode`] to make title generation non-blocking:
//! the title LLM call runs in a spawned task that writes its result into a
//! [`OnceCell`]. This middleware checks the lock after every node's `run`;
//! if the title is ready and `state.summary` is still `None`, it fills it in.
//!
//! Optionally composes with [`LoggingNodeMiddleware`] so that the graph compile
//! API (which accepts only a single middleware) gets both behaviors.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::OnceCell;

use loom_graph_core::{GraphError, LoggingNodeMiddleware, Next, NodeMiddleware};

use crate::state::ReActState;

pub struct TitleAssemblyMiddleware {
    slot: Arc<OnceCell<String>>,
    logger: Option<Arc<LoggingNodeMiddleware<ReActState>>>,
}

impl TitleAssemblyMiddleware {
    pub fn new(
        slot: Arc<OnceCell<String>>,
        logger: Option<Arc<LoggingNodeMiddleware<ReActState>>>,
    ) -> Self {
        Self { slot, logger }
    }
}

#[async_trait]
impl NodeMiddleware<ReActState> for TitleAssemblyMiddleware {
    async fn around_run(
        &self,
        node_id: &str,
        state: ReActState,
        inner: Box<
            dyn FnOnce(
                    ReActState,
                ) -> Pin<
                    Box<
                        dyn std::future::Future<
                                Output = Result<(ReActState, Next), GraphError>,
                            > + Send,
                    >,
                > + Send,
        >,
    ) -> Result<(ReActState, Next), GraphError> {
        let result = if let Some(ref logger) = self.logger {
            logger.around_run(node_id, state, inner).await
        } else {
            inner(state).await
        };

        let (mut state, next) = result?;

        if state.summary.is_none() {
            if let Some(title) = self.slot.get() {
                state.summary = Some(title.to_string());
            }
        }

        Ok((state, next))
    }
}
