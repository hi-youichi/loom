//! State graph: nodes + linear edges, compile and invoke.
//!
//! StateGraph: add nodes and edges, compile, then
//! invoke with state.

mod compile_error;
mod compiled;
mod conditional;
mod interrupt;
mod logging;
mod logging_middleware;
mod name_node;
mod next;
mod node;
mod node_middleware;
mod retry;
mod run_context;
mod runtime;
mod state_graph;
mod visualization;

pub use compile_error::CompilationError;
pub use compiled::CompiledStateGraph;
pub use conditional::{ConditionalRouter, ConditionalRouterFn, NextEntry};
pub use interrupt::{DefaultInterruptHandler, GraphInterrupt, Interrupt, InterruptHandler};
pub use logging::{
    log_graph_complete, log_graph_error, log_graph_start, log_node_complete, log_node_start,
    log_state_update,
};
pub use logging_middleware::LoggingNodeMiddleware;
pub use name_node::NameNode;
pub use next::Next;
pub use node::Node;
pub use node_middleware::NodeMiddleware;
pub use retry::RetryPolicy;
pub use run_context::RunContext;
pub use runtime::Runtime;
pub use state_graph::{StateGraph, END, START};
pub use visualization::{generate_dot, generate_text};
