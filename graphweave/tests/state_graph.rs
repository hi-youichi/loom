//! Integration tests for StateGraph: compile validation, invoke, with_store, middleware.
//!
//! Tests are split into modules under `state_graph/`:
//! - `common`: shared types (AgentState, EchoAgent)
//! - `compile_fail`: compile error cases
//! - `invoke`: invoke output
//! - `store`: with_store / store()
//! - `middleware`: compile_with_middleware and with_middleware().compile()

mod init_logging;

#[path = "state_graph/common.rs"]
mod common;

#[path = "state_graph/agent_as_node.rs"]
mod agent_as_node;

#[path = "state_graph/compile_fail.rs"]
mod compile_fail;

#[path = "state_graph/invoke.rs"]
mod invoke;

#[path = "state_graph/store.rs"]
mod store;

#[path = "state_graph/middleware.rs"]
mod middleware;

#[path = "state_graph/subgraph_as_node.rs"]
mod subgraph_as_node;
