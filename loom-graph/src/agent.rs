//! Minimal agent trait: state in, state out.
//!
//! No separate Input/Output; invoke(state) returns updated state.
//! Used by all agents (e.g. EchoAgent) and by callers that run one step per `run(state)`.
//! When `Agent::State == S`, an agent can be used as a graph `Node<S>` (see blanket impl below).

use std::sync::Arc;

use loom_llm::error::AgentError;

use crate::{Next, Node};

/// Minimal agent trait: state in, state out.
///
/// No separate Input/Output; invoke(state) returns updated state.
/// Used by all agents (e.g. EchoAgent) and by callers that run one step per `run(state)`.
/// When `Agent::State == S`, an agent can be used as a graph `Node<S>` (see blanket impl below).
///
/// Minimal agent: state in, state out (no Input/Output).
///
/// One step: receive state, return updated state. Equivalent to a single node
/// with fixed edge START → node → END. No streaming or tools in this minimal API.
///
/// **State is defined by the implementer**: each agent chooses its own `State` type
/// and fields (e.g. `messages` only, or `messages` + `metadata`, or a custom struct).
/// See the echo example for a minimal `AgentState` (message list) defined in the example.
///
/// **As graph node**: Wrap in [`AgentNode`] to use in `StateGraph::add_node`.
#[async_trait::async_trait]
pub trait Agent: Send + Sync {
    /// Display name of the agent (e.g. "echo", "chat").
    fn name(&self) -> &str;

    /// State type for this agent; **defined by the implementer** (fields and shape).
    /// Must be cloneable and sendable across async boundaries.
    type State: Clone + Send + Sync + std::fmt::Debug + 'static;

    /// One step: receive state, return updated state.
    ///
    /// Caller puts input (e.g. user message) into state before calling;
    /// reads output (e.g. assistant message) from the returned state.
    async fn run(&self, state: Self::State) -> Result<Self::State, AgentError>;
}

/// Wrapper that adapts an [`Agent`] into a [`Node<S>`] for use in `StateGraph::add_node`.
///
/// Since `Node` is defined in the external `loom_graph` crate, we cannot provide
/// a blanket `impl<A: Agent> Node<A::State> for A`. Instead, wrap the agent:
///
/// ```rust,ignore
/// let agent = EchoAgent::new();
/// graph.add_node("echo", Arc::new(AgentNode::new(agent)));
/// ```
pub struct AgentNode<A: Agent> {
    inner: Arc<A>,
}

impl<A: Agent> AgentNode<A> {
    pub fn new(agent: A) -> Self {
        Self { inner: Arc::new(agent) }
    }

    pub fn from_arc(agent: Arc<A>) -> Self {
        Self { inner: agent }
    }
}

#[async_trait::async_trait]
impl<A: Agent> Node<A::State> for AgentNode<A> {
    fn id(&self) -> &str {
        self.inner.name()
    }

    async fn run(&self, state: A::State) -> Result<(A::State, Next), AgentError> {
        self.inner.run(state).await.map(|s| (s, Next::Continue))
    }
}
