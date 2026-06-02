//! Loom Graph Crate
//!
//! Core graph structures and types for the Loom agent framework.

pub mod cancellable;
pub mod channels;
pub mod compile_error;
pub mod compiled;
pub mod conditional;
pub mod interrupt;
pub mod logging;
pub mod logging_middleware;
pub mod managed;
pub mod memory;
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
pub use channels::{Channel, ChannelError, BinaryOperatorAggregate, EphemeralValue, LastValue, Topic, TopicSingleWrite, NamedBarrierValue, NamedBarrierUpdate, StateUpdater, BoxedStateUpdater, ReplaceUpdater, FieldBasedUpdater, boxed_updater};
pub use compile_error::CompilationError;
pub use compiled::{CompiledStateGraph, GraphStream};
pub use conditional::{ConditionalRouter, ConditionalRouterFn};
pub use interrupt::{InterruptHandler, DefaultInterruptHandler, Interrupt};
pub use logging::{log_node_start, log_node_state, log_node_complete, log_state_update, log_graph_start, log_graph_complete, log_graph_error};
pub use logging_middleware::LoggingNodeMiddleware;
pub use managed::{ManagedValue, IsLastStep};
pub use memory::{
    Checkpoint, CheckpointMetadata, CheckpointListItem, CheckpointTuple,
    CheckpointUserMeta, KernelMetadata, CheckpointSource,
    Checkpointer, CheckpointError,
    ChannelVersions, PendingWrite,
    RunnableConfig,
    Store, StoreOp, StoreError, StoreOpResult, StoreSearchHit,
    Item, SearchItem, Namespace,
    FilterOp, ListNamespacesOptions, MatchCondition, NamespaceMatchType, SearchOptions,
    uuid6, uuid6_with_params, Uuid6,
    CHECKPOINT_VERSION, ERROR, SCHEDULED, INTERRUPT, RESUME, writes_idx_map,
};
pub use name_node::NameNode;
pub use next::Next;
pub use node::Node;
pub use node_middleware::NodeMiddleware;
pub use retry::RetryPolicy;
pub use run_context::RunContext;
pub use runtime::Runtime;
pub use state_graph::{StateGraph, START, END, MetadataExtractorFn};
pub use visualization::{generate_dot, generate_text};