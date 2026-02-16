//! Name node: a no-op node that only has a name.
//!
//! Implements `Node<S>` for any state type `S`. Used as a placeholder or
//! pass-through in a StateGraph. Interaction: `StateGraph::add_node`,
//! `Node::id`, `Node::run`; always returns `Next::Continue` and leaves state unchanged.

use async_trait::async_trait;
use std::fmt::Debug;

use crate::error::AgentError;

use super::Next;
use super::Node;

/// A node that does nothing except expose a name; state is passed through unchanged.
///
/// Implements `Node<S>` for any `S: Clone + Send + Sync + 'static`. Use with
/// `StateGraph::add_node(id, Arc::new(NameNode::new(id)))` and `add_edge(from, to)`
/// (use `START` and `END` for graph entry/exit) to include it in the chain.
/// `run` returns `Ok((state, Next::Continue))`.
pub struct NameNode {
    name: String,
}

impl NameNode {
    /// Creates a name node with the given id (returned by `Node::id`).
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[async_trait]
impl<S> Node<S> for NameNode
where
    S: Clone + Send + Sync + Debug + 'static,
{
    fn id(&self) -> &str {
        &self.name
    }

    /// Pass-through: returns the same state and `Next::Continue`.
    async fn run(&self, state: S) -> Result<(S, Next), AgentError> {
        Ok((state, Next::Continue))
    }
}
