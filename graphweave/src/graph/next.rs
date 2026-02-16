//! Next-step result from a graph node: continue linear chain, jump to a node, or end.
//!
//! The graph runner uses this to decide the next node or to stop.

/// Next step after running a node.
///
/// - **Continue**: follow the linear edge order (next node in chain, or END if last).
/// - **Node(id)**: jump to the given node (e.g. observe → think for ReAct loop).
/// - **End**: stop; return current state as final result.
///
/// **Interaction**: Returned by `Node::run`; consumed by `CompiledStateGraph::invoke`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Next {
    /// Follow linear edge order; if current node is last, equivalent to End.
    Continue,
    /// Run the node with the given id next.
    Node(String),
    /// Stop and return the current state.
    End,
}
