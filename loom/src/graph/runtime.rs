//! Runtime context for graph execution.
//!
//! Provides access to run-scoped context, store, stream writer, and previous state.
//! Runtime bundles context and runtime utilities for graph nodes.

use std::fmt::Debug;
use std::sync::Arc;

use crate::memory::{RunnableConfig, Store};
use crate::stream::StreamEvent;

/// Runtime context that bundles run-scoped context and other runtime utilities.
///
/// This struct is designed to be injected into graph nodes and middleware.
/// It provides access to `context`, `store`, `stream_writer`, and `previous`.
///
/// # Note on Config
///
/// `Runtime` does not include `config` directly. To access `RunnableConfig`,
/// you can inject it directly by adding a `config: RunnableConfig` parameter
/// to your node function (recommended), or access it via the `RunContext`.
///
/// # Example
///
/// ```rust,ignore
/// use loom::graph::Runtime;
/// use loom::memory::{RunnableConfig, InMemoryStore};
/// use std::sync::Arc;
///
/// let config = RunnableConfig::default();
/// let store = Arc::new(InMemoryStore::new());
///
/// let runtime: Runtime<String, String> = Runtime::new(config)
///     .with_store(store);
///
/// // Use runtime in node execution
/// ```
pub struct Runtime<C, S>
where
    C: Clone + Send + Sync + Debug + 'static,
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Static context for the graph run, like `user_id`, `db_conn`, etc.
    ///
    /// Can also be thought of as 'run dependencies'.
    pub context: Option<C>,

    /// Store for the graph run, enabling persistence and memory.
    pub store: Option<Arc<dyn Store>>,

    /// Function that writes to the custom stream.
    ///
    /// This is a no-op by default. Set it to enable custom streaming behavior.
    pub stream_writer: Option<Box<dyn Fn(StreamEvent<S>) + Send + Sync>>,

    /// The previous return value for the given thread.
    ///
    /// Only available with the functional API when a checkpointer is provided.
    pub previous: Option<S>,

    /// Config for the current run (thread_id, checkpoint, user_id, etc.).
    pub config: RunnableConfig,
}

impl<C, S> Runtime<C, S>
where
    C: Clone + Send + Sync + Debug + 'static,
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Creates a new Runtime with the given config.
    pub fn new(config: RunnableConfig) -> Self {
        Self {
            context: None,
            store: None,
            stream_writer: None,
            previous: None,
            config,
        }
    }

    /// Sets the runtime context.
    ///
    /// Returns `Self` for method chaining.
    pub fn with_context(mut self, context: C) -> Self {
        self.context = Some(context);
        self
    }

    /// Sets the store for the runtime.
    ///
    /// Returns `Self` for method chaining.
    pub fn with_store(mut self, store: Arc<dyn Store>) -> Self {
        self.store = Some(store);
        self
    }

    /// Sets the stream writer function.
    ///
    /// Returns `Self` for method chaining.
    pub fn with_stream_writer<F>(mut self, writer: F) -> Self
    where
        F: Fn(StreamEvent<S>) + Send + Sync + 'static,
    {
        self.stream_writer = Some(Box::new(writer));
        self
    }

    /// Sets the previous state value.
    ///
    /// Returns `Self` for method chaining.
    pub fn with_previous(mut self, previous: S) -> Self {
        self.previous = Some(previous);
        self
    }

    /// Merges two runtimes together.
    ///
    /// If a value is not provided in the other runtime, the value from
    /// the current runtime is used.
    pub fn merge(mut self, other: Runtime<C, S>) -> Self {
        if other.context.is_some() {
            self.context = other.context;
        }
        if other.store.is_some() {
            self.store = other.store;
        }
        if other.stream_writer.is_some() {
            self.stream_writer = other.stream_writer;
        }
        if other.previous.is_some() {
            self.previous = other.previous;
        }
        // Config is always taken from other (most recent)
        self.config = other.config;
        self
    }
}

impl<C, S> Clone for Runtime<C, S>
where
    C: Clone + Send + Sync + Debug + 'static,
    S: Clone + Send + Sync + Debug + 'static,
{
    fn clone(&self) -> Self {
        Self {
            context: self.context.clone(),
            store: self.store.clone(),
            stream_writer: None, // Cannot clone Fn, so set to None
            previous: self.previous.clone(),
            config: self.config.clone(),
        }
    }
}

impl<C, S> Debug for Runtime<C, S>
where
    C: Clone + Send + Sync + Debug + 'static,
    S: Clone + Send + Sync + Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("context", &self.context)
            .field("store", &self.store.is_some())
            .field("stream_writer", &self.stream_writer.is_some())
            .field("previous", &self.previous)
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::InMemoryStore;

    #[test]
    fn test_runtime_new() {
        let config = RunnableConfig::default();
        let runtime = Runtime::<String, String>::new(config);
        assert!(runtime.context.is_none());
        assert!(runtime.store.is_none());
        assert!(runtime.stream_writer.is_none());
        assert!(runtime.previous.is_none());
    }

    #[test]
    fn test_runtime_with_context() {
        let config = RunnableConfig::default();
        let runtime: Runtime<String, String> =
            Runtime::new(config).with_context("user_123".to_string());
        assert_eq!(runtime.context, Some("user_123".to_string()));
    }

    #[test]
    fn test_runtime_with_store() {
        let config = RunnableConfig::default();
        let store = Arc::new(InMemoryStore::new());
        let runtime: Runtime<String, String> = Runtime::new(config).with_store(store);
        assert!(runtime.store.is_some());
    }

    #[test]
    fn test_runtime_merge() {
        let config1 = RunnableConfig::default();
        let config2 = RunnableConfig::default();

        let runtime1: Runtime<String, String> = Runtime::new(config1)
            .with_context("user_123".to_string())
            .with_previous("state1".to_string());

        let store = Arc::new(InMemoryStore::new());
        let runtime2: Runtime<String, String> = Runtime::new(config2)
            .with_store(store)
            .with_previous("state2".to_string());

        let merged = runtime1.merge(runtime2);
        assert_eq!(merged.context, Some("user_123".to_string()));
        assert!(merged.store.is_some());
        assert_eq!(merged.previous, Some("state2".to_string()));
    }
}
