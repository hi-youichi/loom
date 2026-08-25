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
//! use anureo_graph_core::RunContext;
//!
//! async fn run_with_context(&self, state: S, ctx: &RunContext<S>) -> Result<(S, crate::next::Next), crate::error::GraphError> {
//!     // Method 1: Use stream_writer() to get a StreamWriter
//!     let writer = ctx.stream_writer();
//!     writer.emit_custom(serde_json::json!({"progress": 50})).await;
//!     
//!     // Method 2: Use convenience methods directly on RunContext
//!     ctx.emit_custom(serde_json::json!({"status": "done"})).await;
//!     
//!     Ok((state, crate::next::Next::Continue))
//! }
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::managed::ManagedValue;
use checkpoint::{RunnableConfig, Store};
use stream_event::{StreamEvent, StreamMode, StreamWriter};

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
/// use anureo_graph_core::RunContext;
/// use checkpoint::RunnableConfig;
///
/// let config = RunnableConfig::default();
/// let mut ctx = RunContext::<String>::new(config);
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

    /// Cancellation token for the current run.
    pub cancellation: Option<CancellationToken>,
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
            cancellation: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed::ManagedValue;
    use checkpoint;
    use serde_json::json;
    use std::sync::Arc;
    use stream_event::StreamMode;
    use tokio::sync::mpsc;

    // Mock ManagedValue for testing
    struct TestManagedValue {
        value: serde_json::Value,
    }

    impl TestManagedValue {
        fn new(value: serde_json::Value) -> Self {
            Self { value }
        }
    }

    impl ManagedValue<serde_json::Value, String> for TestManagedValue {
        fn get(&self, _ctx: &super::RunContext<String>) -> serde_json::Value {
            self.value.clone()
        }
    }

    // Mock Store for testing
    struct MockStore;

    #[async_trait::async_trait]
    impl checkpoint::Store for MockStore {
        async fn get(
            &self,
            _namespace: &checkpoint::Namespace,
            _key: &str,
        ) -> Result<Option<serde_json::Value>, checkpoint::StoreError> {
            Ok(None)
        }

        async fn get_item(
            &self,
            _namespace: &checkpoint::Namespace,
            _key: &str,
        ) -> Result<Option<checkpoint::Item>, checkpoint::StoreError> {
            Ok(None)
        }

        async fn put(
            &self,
            _namespace: &checkpoint::Namespace,
            _key: &str,
            _value: &serde_json::Value,
        ) -> Result<(), checkpoint::StoreError> {
            Ok(())
        }

        async fn delete(
            &self,
            _namespace: &checkpoint::Namespace,
            _key: &str,
        ) -> Result<(), checkpoint::StoreError> {
            Ok(())
        }

        async fn list(
            &self,
            _namespace: &checkpoint::Namespace,
        ) -> Result<Vec<String>, checkpoint::StoreError> {
            Ok(vec![])
        }

        async fn search(
            &self,
            _namespace_prefix: &checkpoint::Namespace,
            _options: checkpoint::SearchOptions,
        ) -> Result<Vec<checkpoint::SearchItem>, checkpoint::StoreError> {
            Ok(vec![])
        }

        async fn list_namespaces(
            &self,
            _options: checkpoint::ListNamespacesOptions,
        ) -> Result<Vec<checkpoint::Namespace>, checkpoint::StoreError> {
            Ok(vec![])
        }

        async fn batch(
            &self,
            _ops: Vec<checkpoint::StoreOp>,
        ) -> Result<Vec<checkpoint::StoreOpResult>, checkpoint::StoreError> {
            Ok(vec![])
        }
    }

    #[test]
    fn test_run_context_new_creates_empty_context() {
        let ctx = RunContext::<String>::new(RunnableConfig::default());

        assert!(ctx.stream_tx.is_none());
        assert!(ctx.stream_mode.is_empty());
        assert!(ctx.managed_values.is_empty());
        assert!(ctx.store.is_none());
        assert!(ctx.previous.is_none());
        assert!(ctx.runtime_context.is_none());
        assert!(ctx.cancellation.is_none());
    }

    #[test]
    fn test_run_context_with_stream_sender_and_modes() {
        let (tx, _rx) = mpsc::channel(100);
        let mut modes = HashSet::new();
        modes.insert(StreamMode::Custom);
        modes.insert(StreamMode::Messages);

        let ctx = RunContext::<String> {
            config: RunnableConfig::default(),
            stream_tx: Some(tx),
            stream_mode: modes,
            ..Default::default()
        };

        assert!(ctx.stream_tx.is_some());
        assert!(ctx.is_streaming_mode(StreamMode::Custom));
        assert!(ctx.is_streaming_mode(StreamMode::Messages));
        assert!(!ctx.is_streaming_mode(StreamMode::Values));
    }

    #[test]
    fn test_run_context_config_accessible() {
        let config = RunnableConfig {
            thread_id: Some("test-thread".to_string()),
            ..Default::default()
        };

        let ctx = RunContext::<String>::new(config.clone());
        assert_eq!(ctx.config.thread_id, Some("test-thread".to_string()));
    }

    #[test]
    fn test_run_context_store_getter() {
        let ctx = RunContext::<String>::new(RunnableConfig::default());
        assert!(ctx.store().is_none());

        let ctx = ctx.with_store(Arc::new(MockStore));
        assert!(ctx.store().is_some());
    }

    #[test]
    fn test_run_context_previous_state_getter() {
        let ctx = RunContext::<String>::new(RunnableConfig::default());
        assert!(ctx.previous().is_none());

        let ctx = ctx.with_previous("previous_state".to_string());
        assert!(ctx.previous().is_some());
        assert_eq!(ctx.previous().unwrap(), "previous_state");
    }

    #[test]
    fn test_run_context_runtime_context_getter() {
        let ctx = RunContext::<String>::new(RunnableConfig::default());
        assert!(ctx.runtime_context().is_none());

        let custom_context = json!({"user_id": "123", "session_id": "abc"});
        let ctx = ctx.with_runtime_context(custom_context.clone());

        assert!(ctx.runtime_context().is_some());
        assert_eq!(ctx.runtime_context().unwrap(), &custom_context);
    }

    #[test]
    fn test_run_context_stream_writer_creation() {
        let (tx, _rx) = mpsc::channel(100);
        let mut modes = HashSet::new();
        modes.insert(StreamMode::Custom);

        let ctx = RunContext::<String> {
            config: RunnableConfig::default(),
            stream_tx: Some(tx),
            stream_mode: modes,
            ..Default::default()
        };

        let writer = ctx.stream_writer();
        assert!(writer.is_mode_enabled(StreamMode::Custom));
    }

    #[tokio::test]
    async fn test_run_context_emit_custom_convenience_method() {
        let (tx, mut rx) = mpsc::channel(100);
        let mut modes = HashSet::new();
        modes.insert(StreamMode::Custom);

        let ctx = RunContext::<String> {
            config: RunnableConfig::default(),
            stream_tx: Some(tx),
            stream_mode: modes,
            ..Default::default()
        };

        let custom_payload = json!({"progress": 50, "status": "processing"});
        let sent = ctx.emit_custom(custom_payload.clone()).await;

        assert!(sent);

        let received = rx.recv().await.unwrap();
        if let StreamEvent::Custom(value) = received {
            assert_eq!(value, custom_payload);
        } else {
            panic!("Expected Custom event");
        }
    }

    #[tokio::test]
    async fn test_run_context_emit_custom_no_sender() {
        let ctx = RunContext::<String>::new(RunnableConfig::default());

        let custom_payload = json!({"progress": 50});
        let sent = ctx.emit_custom(custom_payload).await;

        assert!(!sent);
    }

    #[tokio::test]
    async fn test_run_context_emit_custom_mode_disabled() {
        let (tx, _rx) = mpsc::channel(100);
        let modes = HashSet::new(); // No Custom mode

        let ctx = RunContext::<String> {
            config: RunnableConfig::default(),
            stream_tx: Some(tx),
            stream_mode: modes,
            ..Default::default()
        };

        let custom_payload = json!({"progress": 50});
        let sent = ctx.emit_custom(custom_payload).await;

        assert!(!sent);
    }

    #[tokio::test]
    async fn test_run_context_emit_message_convenience_method() {
        let (tx, mut rx) = mpsc::channel(100);
        let mut modes = HashSet::new();
        modes.insert(StreamMode::Messages);

        let ctx = RunContext::<String> {
            config: RunnableConfig::default(),
            stream_tx: Some(tx),
            stream_mode: modes,
            ..Default::default()
        };

        let sent = ctx.emit_message("Hello world", "test_node").await;

        assert!(sent);

        let received = rx.recv().await.unwrap();
        if let StreamEvent::TextDelta { content, .. } = received {
            assert_eq!(content, "Hello world");
        } else {
            panic!("Expected TextDelta event");
        }
    }

    #[tokio::test]
    async fn test_run_context_emit_message_no_sender() {
        let ctx = RunContext::<String>::new(RunnableConfig::default());

        let sent = ctx.emit_message("Hello", "node").await;

        assert!(!sent);
    }

    #[test]
    fn test_run_context_with_cancellation_token() {
        let config = RunnableConfig::default();
        let cancellation = CancellationToken::new();

        let ctx = RunContext::<String> {
            config,
            cancellation: Some(cancellation.clone()),
            ..Default::default()
        };

        assert!(ctx.cancellation.is_some());
        assert!(!ctx.cancellation.as_ref().unwrap().is_cancelled());

        cancellation.cancel();
        assert!(ctx.cancellation.as_ref().unwrap().is_cancelled());
    }

    #[test]
    fn test_run_context_managed_values_operations() {
        let managed_value = Arc::new(TestManagedValue::new(json!({"is_last": true})));

        let ctx = RunContext::<String>::new(RunnableConfig::default())
            .with_managed_value("is_last_step", managed_value);

        assert_eq!(ctx.managed_values.len(), 1);
        assert!(ctx.managed_values.contains_key("is_last_step"));
    }

    #[test]
    fn test_run_context_get_managed_value() {
        let managed_value = Arc::new(TestManagedValue::new(json!({"is_last": true})));

        let ctx = RunContext::<String>::new(RunnableConfig::default())
            .with_managed_value("is_last_step", managed_value);

        let value = ctx.get_managed_value("is_last_step");
        assert!(value.is_some());
        assert_eq!(value.unwrap(), json!({"is_last": true}));
    }

    #[test]
    fn test_run_context_get_managed_value_not_found() {
        let ctx = RunContext::<String>::new(RunnableConfig::default());

        let value = ctx.get_managed_value("non_existent");
        assert!(value.is_none());
    }

    #[test]
    fn test_run_context_edge_case_no_stream_sender() {
        let ctx = RunContext::<String>::new(RunnableConfig::default());

        assert!(ctx.stream_tx.is_none());
        assert!(!ctx.is_streaming_mode(StreamMode::Custom));
        assert!(!ctx.is_streaming_mode(StreamMode::Messages));
        assert!(!ctx.is_streaming_mode(StreamMode::Values));
        assert!(!ctx.is_streaming_mode(StreamMode::Updates));
    }

    #[test]
    fn test_run_context_edge_case_no_store() {
        let ctx = RunContext::<String>::new(RunnableConfig::default());

        assert!(ctx.store.is_none());
        assert!(ctx.store().is_none());
    }

    #[test]
    fn test_run_context_edge_case_no_previous_state() {
        let ctx = RunContext::<String>::new(RunnableConfig::default());

        assert!(ctx.previous.is_none());
        assert!(ctx.previous().is_none());
    }

    #[test]
    fn test_run_context_edge_case_no_runtime_context() {
        let ctx = RunContext::<String>::new(RunnableConfig::default());

        assert!(ctx.runtime_context.is_none());
        assert!(ctx.runtime_context().is_none());
    }

    #[test]
    fn test_run_context_method_chaining() {
        let ctx = RunContext::<String>::new(RunnableConfig::default())
            .with_store(Arc::new(MockStore))
            .with_previous("test_previous".to_string())
            .with_runtime_context(json!({"user_id": "123"}));

        assert!(ctx.store().is_some());
        assert!(ctx.previous().is_some());
        assert!(ctx.runtime_context().is_some());
    }

    #[test]
    fn test_run_context_clone() {
        let original =
            RunContext::<String>::new(RunnableConfig::default()).with_previous("state".to_string());

        let cloned = original.clone();

        assert_eq!(cloned.previous(), original.previous());
        assert!(cloned.previous().is_some());
    }

    #[tokio::test]
    async fn test_run_context_multiple_stream_modes() {
        let (tx, mut rx) = mpsc::channel(100);
        let mut modes = HashSet::new();
        modes.insert(StreamMode::Custom);
        modes.insert(StreamMode::Messages);
        modes.insert(StreamMode::Values);

        let ctx = RunContext::<String> {
            config: RunnableConfig::default(),
            stream_tx: Some(tx),
            stream_mode: modes,
            ..Default::default()
        };

        assert!(ctx.is_streaming_mode(StreamMode::Custom));
        assert!(ctx.is_streaming_mode(StreamMode::Messages));
        assert!(ctx.is_streaming_mode(StreamMode::Values));
        assert!(!ctx.is_streaming_mode(StreamMode::Updates));

        // Test multiple emissions
        ctx.emit_custom(json!({"type": "custom"})).await;
        ctx.emit_message("test", "node1").await;

        let event1 = rx.recv().await.unwrap();
        let event2 = rx.recv().await.unwrap();

        assert!(matches!(event1, StreamEvent::Custom(_)));
        assert!(matches!(event2, StreamEvent::TextDelta { .. }));
    }

    // Default implementation for tests
    impl Default for RunContext<String> {
        fn default() -> Self {
            Self::new(RunnableConfig::default())
        }
    }
}
