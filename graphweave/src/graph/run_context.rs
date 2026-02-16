//! Run context passed into nodes for streaming-aware execution.
//!
//! Holds runnable config, optional stream sender, selected stream modes, and runtime context.
//! This module integrates the Runtime functionality for a unified execution context.
//!
//! # StreamWriter Integration
//!
//! `RunContext` provides methods to create a `StreamWriter` and emit events directly:
//!
//! ```rust,ignore
//! use graphweave::graph::RunContext;
//!
//! async fn run_with_context(&self, state: S, ctx: &RunContext<S>) -> Result<(S, Next), AgentError> {
//!     // Method 1: Use stream_writer() to get a StreamWriter
//!     let writer = ctx.stream_writer();
//!     writer.emit_custom(serde_json::json!({"progress": 50})).await;
//!     
//!     // Method 2: Use convenience methods directly on RunContext
//!     ctx.emit_custom(serde_json::json!({"status": "done"})).await;
//!     
//!     Ok((state, Next::Continue))
//! }
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::managed::ManagedValue;
use crate::memory::{RunnableConfig, Store};
use crate::stream::{StreamEvent, StreamMode, StreamWriter};

/// Run context passed into nodes for streaming-aware execution.
///
/// Holds runnable config, optional stream sender, selected stream modes, managed values,
/// and runtime context (store, previous state, custom context).
///
/// # Runtime Integration
///
/// The `RunContext` integrates the functionality of `Runtime` to provide a unified
/// execution context. It includes:
/// - `store`: Long-term memory store for cross-thread data
/// - `previous`: The previous state value (for functional API with checkpointer)
/// - `runtime_context`: Custom context data (user_id, db_conn, etc.)
///
/// # Example
///
/// ```rust,no_run
/// use graphweave::graph::RunContext;
/// use graphweave::memory::{RunnableConfig, InMemoryStore};
/// use std::sync::Arc;
///
/// let config = RunnableConfig::default();
/// let mut ctx = RunContext::<String>::new(config);
///
/// // Add store
/// let store = Arc::new(InMemoryStore::new());
/// ctx = ctx.with_store(store);
///
/// // Add custom context
/// ctx = ctx.with_runtime_context(serde_json::json!({"user_id": "123"}));
/// ```
#[derive(Clone)]
pub struct RunContext<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Config for the current run (thread_id, checkpoint, user_id, etc.).
    pub config: RunnableConfig,
    /// Optional sender for streaming events.
    pub stream_tx: Option<mpsc::Sender<StreamEvent<S>>>,
    /// Enabled stream modes (Values, Updates, Messages, Custom).
    pub stream_mode: HashSet<StreamMode>,
    /// Managed values accessible during node execution.
    ///
    /// Managed values provide runtime information computed by the graph execution system,
    /// such as `IsLastStep` which indicates whether the current step is the last one.
    pub managed_values: HashMap<String, Arc<dyn ManagedValue<serde_json::Value, S>>>,

    // === Runtime Integration Fields ===
    /// Store for the graph run, enabling persistence and long-term memory.
    ///
    /// When set, nodes can use it for cross-thread memory (e.g., namespace from `config.user_id`).
    pub store: Option<Arc<dyn Store>>,

    /// The previous return value for the given thread.
    ///
    /// Only available when a checkpointer is provided and there is a previous state.
    pub previous: Option<S>,

    /// Custom runtime context (user_id, db_conn, etc.).
    ///
    /// This is a JSON value to support arbitrary context data without requiring
    /// additional type parameters.
    pub runtime_context: Option<serde_json::Value>,
}

impl<S> RunContext<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Creates a new RunContext with default values.
    pub fn new(config: RunnableConfig) -> Self {
        Self {
            config,
            stream_tx: None,
            stream_mode: HashSet::new(),
            managed_values: HashMap::new(),
            store: None,
            previous: None,
            runtime_context: None,
        }
    }

    /// Gets a managed value by name.
    ///
    /// Returns `None` if the managed value is not registered.
    /// The value is returned as a JSON value for type erasure.
    pub fn get_managed_value(&self, name: &str) -> Option<serde_json::Value> {
        self.managed_values.get(name).map(|mv| mv.get(self))
    }

    /// Registers a managed value.
    ///
    /// Returns `Self` for method chaining.
    pub fn with_managed_value(
        mut self,
        name: impl Into<String>,
        value: Arc<dyn ManagedValue<serde_json::Value, S>>,
    ) -> Self {
        self.managed_values.insert(name.into(), value);
        self
    }

    // === Runtime Integration Methods ===

    /// Sets the store for long-term memory.
    ///
    /// When set, nodes can use it for cross-thread memory (e.g., namespace from `config.user_id`).
    ///
    /// Returns `Self` for method chaining.
    pub fn with_store(mut self, store: Arc<dyn Store>) -> Self {
        self.store = Some(store);
        self
    }

    /// Sets the previous state value.
    ///
    /// This is typically set when resuming from a checkpoint.
    ///
    /// Returns `Self` for method chaining.
    pub fn with_previous(mut self, previous: S) -> Self {
        self.previous = Some(previous);
        self
    }

    /// Sets the custom runtime context.
    ///
    /// This can be used to pass arbitrary context data to nodes (user_id, db_conn, etc.).
    ///
    /// Returns `Self` for method chaining.
    pub fn with_runtime_context(mut self, context: serde_json::Value) -> Self {
        self.runtime_context = Some(context);
        self
    }

    /// Gets the store if available.
    pub fn store(&self) -> Option<&Arc<dyn Store>> {
        self.store.as_ref()
    }

    /// Gets the previous state if available.
    pub fn previous(&self) -> Option<&S> {
        self.previous.as_ref()
    }

    /// Gets the runtime context if available.
    pub fn runtime_context(&self) -> Option<&serde_json::Value> {
        self.runtime_context.as_ref()
    }

    // === StreamWriter Integration ===

    /// Creates a StreamWriter from this context.
    ///
    /// The StreamWriter encapsulates the stream sender and mode checking,
    /// providing a convenient API for emitting events.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let writer = ctx.stream_writer();
    /// writer.emit_custom(serde_json::json!({"progress": 50})).await;
    /// writer.emit_message("Hello", "node_id").await;
    /// ```
    pub fn stream_writer(&self) -> StreamWriter<S> {
        StreamWriter::new(self.stream_tx.clone(), self.stream_mode.clone())
    }

    /// Emits a custom JSON payload directly from the context.
    ///
    /// This is a convenience method that creates a StreamWriter and calls emit_custom.
    /// Only sends if `StreamMode::Custom` is enabled.
    ///
    /// Returns `true` if the event was sent, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// ctx.emit_custom(serde_json::json!({"status": "processing"})).await;
    /// ```
    pub async fn emit_custom(&self, value: Value) -> bool {
        self.stream_writer().emit_custom(value).await
    }

    /// Emits a message chunk directly from the context.
    ///
    /// This is a convenience method that creates a StreamWriter and calls emit_message.
    /// Only sends if `StreamMode::Messages` is enabled.
    ///
    /// Returns `true` if the event was sent, `false` otherwise.
    ///
    /// # Arguments
    ///
    /// * `content` - The message content
    /// * `node_id` - The node ID that produced this message
    pub async fn emit_message(
        &self,
        content: impl Into<String>,
        node_id: impl Into<String>,
    ) -> bool {
        self.stream_writer().emit_message(content, node_id).await
    }

    /// Checks if a specific stream mode is enabled.
    ///
    /// Useful for nodes that want to conditionally perform expensive operations
    /// only when the corresponding stream mode is enabled.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if ctx.is_streaming_mode(StreamMode::Custom) {
    ///     // Perform expensive computation for progress reporting
    ///     ctx.emit_custom(serde_json::json!({"progress": compute_progress()})).await;
    /// }
    /// ```
    pub fn is_streaming_mode(&self, mode: StreamMode) -> bool {
        self.stream_mode.contains(&mode)
    }
}
