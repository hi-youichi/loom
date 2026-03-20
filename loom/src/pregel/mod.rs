//! Pregel-style bulk-synchronous parallel runtime building blocks.
//!
//! This module exposes Loom's low-level graph runtime inspired by Google's Pregel
//! and LangGraph's bulk-synchronous parallel (BSP) execution model. It provides
//! fine-grained control over graph execution, state management, and checkpointing
//! for building complex, stateful agent workflows.
//!
//! # Core Concepts
//!
//! ## Bulk-Synchronous Parallel (BSP) Model
//!
//! Pregel executes graphs in discrete supersteps (also called "steps"). Within each
//! superstep:
//!
//! 1. **Barrier synchronization**: All scheduled nodes receive their inputs from
//!    channels simultaneously.
//! 2. **Parallel execution**: Nodes execute independently and concurrently.
//! 3. **Write aggregation**: All outputs are collected and applied to channels.
//! 4. **Channel update**: Channels are updated, potentially triggering new nodes.
//!
//! This model ensures deterministic behavior even with concurrent execution,
//! making it ideal for debugging, testing, and checkpointing.
//!
//! ## Channels
//!
//! Channels are named communication buffers that hold state between supersteps.
//! Each channel has a specific aggregation strategy:
//!
//! - [`LastValueChannel`]: Keeps only the most recent value (overwrites previous).
//! - [`TopicChannel`]: Accumulates values across steps, useful for message history.
//! - [`BinaryAggregateChannel`]: Uses a custom reducer function to combine values.
//!
//! Nodes subscribe to channels (triggered when the channel updates) and read from
//! channels (accessing values without triggering). See [`ChannelSpec`] for
//! configuring channel subscriptions.
//!
//! ## Nodes
//!
//! [`PregelNode`] represents a computation unit in the graph. Each node:
//!
//! - **Subscribes** to one or more trigger channels that schedule its execution.
//! - **Reads** from additional channels to access state without triggering.
//! - **Writes** to channels via [`PregelNodeOutput`], producing new values.
//!
//! Nodes are pure functions: given the same input state, they produce the same
//! output. Side effects (like I/O) should be managed through the
//! [`PregelNodeContext`] when needed.
//!
//! # Execution Flow
//!
//! A typical Pregel execution proceeds as follows:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     PregelRuntime                          │
//! │  ┌─────────────┐    ┌─────────────┐    ┌───────────────┐   │
//! │  │   Input     │───▶│  Superstep  │───▶│   Channel     │   │
//! │  │  Channels   │    │   (step N)  │    │   Update      │   │
//! │  └─────────────┘    └──────┬──────┘    └───────┬───────┘   │
//! │                            │                   │           │
//! │                            ▼                   │           │
//! │  ┌─────────────────────────────────────────┐   │           │
//! │  │              Node Execution              │◀──┘           │
//! │  │  ┌───────┐  ┌───────┐  ┌───────┐       │               │
//! │  │  │Node A │  │Node B │  │Node C │  ...  │               │
//! │  │  └───┬───┘  └───┬───┘  └───┬───┘       │               │
//! │  │      │          │          │           │               │
//! │  │      └──────────┼──────────┘           │               │
//! │  │                 ▼                      │               │
//! │  │         Output Aggregation             │               │
//! │  └─────────────────────────────────────────┘               │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! 1. **Initialization**: Input values are written to designated channels.
//! 2. **Scheduling**: Channels with new values trigger subscribed nodes.
//! 3. **Task Preparation**: [`prepare_next_tasks`] creates executable tasks.
//! 4. **Parallel Execution**: All tasks for the current step execute concurrently.
//! 5. **Write Application**: [`apply_writes`] updates channels with outputs.
//! 6. **Loop Detection**: If no new tasks are scheduled, execution terminates.
//!
//! # Architecture
//!
//! The module is organized around four core types:
//!
//! ## [`PregelGraph`] — Static Graph Definition
//!
//! An immutable description of the computation graph, including:
//! - Channel specifications and their types
//! - Node definitions with subscribe/read channel lists
//! - Subgraph compositions for hierarchical graphs
//!
//! ## [`PregelNode`] — Node Declaration
//!
//! Defines a single computation node with:
//! - Trigger channels (scheduling)
//! - Read channels (state access)
//! - The async function implementing the node logic
//!
//! ## [`PregelRuntime`] — Execution Driver
//!
//! The main entry point for running graphs. Provides:
//! - Graph validation before execution
//! - Invocation (single result) and streaming modes
//! - Checkpoint persistence and state history
//! - Task caching for deterministic replay
//!
//! ## [`PregelLoop`] — Per-Run State Machine
//!
//! Internal mutable state tracking:
//! - Current step number
//! - Active channel snapshots
//! - Pending writes and sends
//! - Interrupt/resume state
//!
//! # API Layers
//!
//! The public API is organized into four layers:
//!
//! ## Definition and Validation
//!
//! Build and validate graph definitions:
//! - [`PregelGraph`] — Graph builder with fluent API
//! - [`ChannelSpec`] — Configure channel subscriptions
//! - [`PregelRuntime::validate`] — Verify graph integrity before execution
//!
//! ## Introspection
//!
//! Query graph structure and runtime state:
//! - [`PregelGraphView`] — Read-only view of graph topology
//! - [`PregelRuntime::get_graph`] — Access the underlying graph
//! - [`PregelRuntime::get_subgraphs`] — List nested subgraphs
//!
//! ## Execution
//!
//! Run the graph in different modes:
//! - [`PregelRuntime::invoke`] — Execute to completion, return final state
//! - [`PregelRuntime::stream`] — Stream intermediate states and events
//! - [`PregelRuntime::invoke_subgraph`] — Execute a nested subgraph
//!
//! ## Persistence and Replay
//!
//! Manage state across runs:
//! - [`PregelRuntime::get_state`] — Retrieve current checkpoint state
//! - [`PregelRuntime::get_state_history`] — Access historical checkpoints
//! - [`PregelRuntime::replay`] — Replay execution from a checkpoint
//!
//! # Example
//!
//! ```ignore
//! use loom::pregel::{PregelGraph, PregelNode, PregelRuntime, ChannelSpec, LastValueChannel};
//!
//! // Define channels
//! let mut graph = PregelGraph::new("my_graph");
//! graph.add_channel("input", Box::new(LastValueChannel::new()));
//! graph.add_channel("output", Box::new(LastValueChannel::new()));
//!
//! // Define a node that transforms input to output
//! let node = PregelNode::new("transform")
//!     .subscribe("input")      // Triggered when 'input' changes
//!     .read("input")           // Read the current input value
//!     .write("output")         // Write to 'output' channel
//!     .with_function(|ctx, input| async move {
//!         // Process input and produce output
//!         let value = input.trigger_values.get("input").cloned();
//!         Ok(PregelNodeOutput {
//!             writes: vec![("output".into(), value.unwrap_or_default())],
//!             ..Default::default()
//!         })
//!     });
//!
//! graph.add_node(node);
//!
//! // Create and run the runtime
//! let runtime = PregelRuntime::new(graph);
//! runtime.validate()?;
//!
//! let result = runtime.invoke(
//!     vec![("input".into(), "hello".into())].into_iter().collect(),
//!     Default::default(),
//! ).await?;
//! ```
//!
//! # Relationship to Higher-Level APIs
//!
//! **Note**: This module provides low-level primitives for maximum control.
//! For most applications, consider using the higher-level [`StateGraph`] API
//! instead, which is built on top of Pregel and provides:
//!
//! - Simpler node definition with typed state
//! - Automatic edge routing via [`Next`] return values
//! - Built-in message accumulation for chat agents
//! - Easier testing and debugging
//!
//! Use this `pregel` module when you need:
//! - Fine-grained control over channel semantics
//! - Custom checkpoint or replay strategies
//! - Direct access to the BSP execution model
//! - Building new higher-level abstractions
//!
//! [`StateGraph`]: crate::graph::StateGraph
//! [`Next`]: crate::graph::Next
//! [`PregelRuntime::validate`]: PregelRuntime::validate
//! [`PregelRuntime::invoke`]: PregelRuntime::invoke
//! [`PregelRuntime::stream`]: PregelRuntime::stream
//! [`PregelRuntime::invoke_subgraph`]: PregelRuntime::invoke_subgraph
//! [`PregelRuntime::get_graph`]: PregelRuntime::get_graph
//! [`PregelRuntime::get_subgraphs`]: PregelRuntime::get_subgraphs
//! [`PregelRuntime::get_state`]: PregelRuntime::get_state
//! [`PregelRuntime::get_state_history`]: PregelRuntime::get_state_history
//! [`PregelRuntime::replay`]: PregelRuntime::replay

mod algo;
mod cache;
mod channel;
mod config;
mod graph_view;
mod loop_state;
mod node;
mod replay;
mod runner;
mod runtime;
mod state;
mod subgraph;
mod types;
mod validate;

pub use algo::{
    apply_writes, finish_channels, normalize_pending_sends, normalize_pending_writes,
    prepare_next_tasks, restore_channels_from_checkpoint, snapshot_channels, task_id_for,
    ExecutableTask, PreparedTask, TaskOutcome,
};
pub use cache::{CachedTaskWrites, InMemoryPregelTaskCache, PregelTaskCache, TaskCacheKey};
pub use channel::{
    BinaryAggregateChannel, BoxedChannel, Channel, ChannelKind, ChannelSpec, LastValueChannel,
    ReducerFn, TopicChannel,
};
pub use config::{PregelConfig, PregelDurability};
pub use graph_view::{
    PregelGraphChannelView, PregelGraphEdgeKind, PregelGraphEdgeView, PregelGraphNodeView,
    PregelGraphView, PregelNamedGraphView,
};
pub use loop_state::{InterruptState, PregelLoop};
pub use node::{PregelGraph, PregelNode, PregelNodeContext, PregelNodeInput, PregelNodeOutput};
pub use replay::{ReplayMode, ReplayRequest, ReplayResult};
pub use runner::PregelRunner;
pub use runtime::{PregelRuntime, PregelStream};
pub use state::{BulkStateUpdateRequest, PregelStateSnapshot, StateUpdateRequest};
pub use subgraph::{
    CheckpointNamespace, PregelSubgraph, PregelSubgraphEntry, SubgraphInvocation, SubgraphResult,
};
pub use types::{
    ChannelName, ChannelValue, ChannelVersion, InterruptRecord, LoopStatus, ManagedValues,
    NodeName, PendingWrite, PregelScratchpad, ReservedWrite, ResumeMap, SendPacket, TaskId,
    TaskKind, TASKS_CHANNEL,
};
