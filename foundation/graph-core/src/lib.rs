//! anureo Graph Crate
//!
//! Core graph structures and types for the anureo agent framework.

pub mod agent;
pub mod cancellable;
pub mod channels;
pub mod compile_error;
pub mod compiled;
pub mod conditional;
pub mod error;
pub mod interrupt;
pub mod logging;
pub mod logging_middleware;
pub mod managed;

pub mod name_node;
pub mod next;
pub mod node;
pub mod node_middleware;
pub mod retry;
pub mod run_context;
pub mod runtime;
pub mod state_graph;
pub mod visualization;

// Re-export public types from each module
pub use cancellable::run_cancellable;
pub use channels::{
    boxed_updater, BinaryOperatorAggregate, BoxedStateUpdater, Channel, ChannelError,
    EphemeralValue, FieldBasedUpdater, LastValue, NamedBarrierUpdate, NamedBarrierValue,
    ReplaceUpdater, StateUpdater, Topic, TopicSingleWrite,
};
pub use compile_error::CompilationError;
pub use compiled::{CompiledStateGraph, GraphStream};
pub use conditional::{ConditionalRouter, ConditionalRouterFn};
pub use error::{GraphError, Interrupt};
pub use interrupt::{DefaultInterruptHandler, InterruptHandler};
pub use logging::{
    log_graph_complete, log_graph_error, log_graph_start, log_node_complete, log_node_start,
    log_node_state, log_state_update,
};
pub use logging_middleware::LoggingNodeMiddleware;
pub use managed::{IsLastStep, ManagedValue};

pub use name_node::NameNode;
pub use next::Next;
pub use node::Node;
pub use node_middleware::NodeMiddleware;
pub use retry::RetryPolicy;
pub use run_context::RunContext;
pub use runtime::Runtime;
pub use state_graph::{MetadataExtractorFn, StateGraph, END, START};
pub use visualization::{generate_dot, generate_text};

// Re-export Agent trait and AgentNode
pub use agent::{Agent, AgentNode};
