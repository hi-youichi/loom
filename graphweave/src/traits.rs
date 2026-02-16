//! Minimal agent trait: state in, state out.
//!
//! No separate Input/Output; invoke(state) returns updated state.
//! Used by all agents (e.g. EchoAgent) and by callers that run one step per `run(state)`.
//! When `Agent::State == S`, an agent can be used as a graph `Node<S>` (see blanket impl below).

use async_trait::async_trait;
use std::fmt::Debug;

use crate::error::AgentError;
use crate::graph::{Next, Node};

/// Minimal agent: state in, state out (no Input/Output).
///
/// One step: receive state, return updated state. Equivalent to a single node
/// with fixed edge START → node → END. No streaming or tools in this minimal API.
///
/// **State is defined by the implementer**: each agent chooses its own `State` type
/// and fields (e.g. `messages` only, or `messages` + `metadata`, or a custom struct).
/// See the echo example for a minimal `AgentState` (message list) defined in the example.
///
/// **As graph node**: When the graph state type `S` equals `Agent::State`, implementors
/// automatically implement `Node<S>` so they can be used in `StateGraph::add_node`.
#[async_trait]
pub trait Agent: Send + Sync {
    /// Display name of the agent (e.g. "echo", "chat").
    fn name(&self) -> &str;

    /// State type for this agent; **defined by the implementer** (fields and shape).
    /// Must be cloneable and sendable across async boundaries.
    type State: Clone + Send + Sync + Debug + 'static;

    /// One step: receive state, return updated state.
    ///
    /// Caller puts input (e.g. user message) into state before calling;
    /// reads output (e.g. assistant message) from the returned state.
    async fn run(&self, state: Self::State) -> Result<Self::State, AgentError>;
}

/// Any agent whose state type is `S` can be used as a graph node.
///
/// Allows `StateGraph::add_node("id", Arc::new(some_agent))` when the graph
/// state type matches the agent's state. Interacts with `StateGraph`, `Node`, and `Agent`.
#[async_trait]
impl<S, A> Node<S> for A
where
    S: Clone + Send + Sync + Debug + 'static,
    A: Agent<State = S> + Send + Sync,
{
    fn id(&self) -> &str {
        self.name()
    }

    async fn run(&self, state: S) -> Result<(S, Next), AgentError> {
        Agent::run(self, state).await.map(|s| (s, Next::Continue))
    }
}
