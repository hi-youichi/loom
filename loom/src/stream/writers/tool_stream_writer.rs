use serde_json::Value;
use std::fmt::Debug;
use std::sync::Arc;

/// A writer for emitting custom streaming events from tools.
///
/// This is a type-erased wrapper that doesn't require the state type `S`,
/// making it suitable for use in tools which are state-agnostic. Tools can
/// use this to emit progress updates, intermediate results, or any custom
/// JSON data during execution.
///
/// # Example
///
/// ```rust,ignore
/// use loom::stream::ToolStreamWriter;
/// use serde_json::json;
///
/// async fn my_tool(writer: &ToolStreamWriter) -> String {
///     // Emit progress updates
///     writer.emit_custom(json!({"status": "starting"}));
///     
///     // Do work...
///     
///     writer.emit_custom(json!({"status": "done", "result_count": 42}));
///     "Tool completed".to_string()
/// }
/// ```
///
/// # Thread Safety
///
/// `ToolStreamWriter` is `Clone + Send + Sync`, so it can be safely shared
/// across async tasks or threads.
#[derive(Clone)]
pub struct ToolStreamWriter {
    /// Function that emits a custom event. Returns true if sent successfully.
    emit_fn: Arc<dyn Fn(Value) -> bool + Send + Sync>,
    /// Function that emits a tool output chunk. Returns true if sent successfully.
    output_fn: Option<Arc<dyn Fn(String) -> bool + Send + Sync>>,
}

impl ToolStreamWriter {
    /// Creates a new ToolStreamWriter with the given emit function.
    ///
    /// The emit function should return `true` if the event was sent successfully,
    /// `false` otherwise (e.g., if streaming is not enabled or channel is full).
    ///
    /// # Arguments
    ///
    /// * `emit_fn` - Function that handles emitting custom events
    pub fn new(emit_fn: impl Fn(Value) -> bool + Send + Sync + 'static) -> Self {
        Self {
            emit_fn: Arc::new(emit_fn),
            output_fn: None,
        }
    }

    /// Creates a ToolStreamWriter with both custom emit and tool output callbacks.
    pub fn new_with_output(
        emit_fn: impl Fn(Value) -> bool + Send + Sync + 'static,
        output_fn: impl Fn(String) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            emit_fn: Arc::new(emit_fn),
            output_fn: Some(Arc::new(output_fn)),
        }
    }

    /// Creates a no-op ToolStreamWriter that does nothing.
    ///
    /// Useful when streaming is not enabled but code still needs a writer.
    pub fn noop() -> Self {
        Self {
            emit_fn: Arc::new(|_| false),
            output_fn: None,
        }
    }

    /// Emits a custom JSON payload.
    ///
    /// Returns `true` if the event was sent successfully, `false` otherwise.
    /// This is a non-blocking operation that uses `try_send` internally.
    ///
    /// # Arguments
    ///
    /// * `value` - The JSON value to emit
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use serde_json::json;
    ///
    /// let sent = writer.emit_custom(json!({"progress": 50}));
    /// if sent {
    ///     println!("Progress update sent");
    /// }
    /// ```
    pub fn emit_custom(&self, value: Value) -> bool {
        (self.emit_fn)(value)
    }

    /// Emits a tool output chunk (incremental text output during execution).
    ///
    /// Returns `true` if the event was sent successfully, `false` if no output
    /// callback is configured or sending failed.
    pub fn emit_output(&self, content: &str) -> bool {
        self.output_fn
            .as_ref()
            .map(|f| f(content.to_string()))
            .unwrap_or(false)
    }

    /// Checks if this writer is a no-op (always returns false).
    ///
    /// This can be used to skip expensive computations when streaming
    /// is not enabled.
    pub fn is_noop(&self) -> bool {
        // We can't truly check if it's a noop, but we can try sending
        // a null value and see if it returns false. However, this is
        // not reliable as the channel might be full. Instead, we just
        // document that users should check stream mode before expensive ops.
        false
    }

    /// Returns a clone of the emit_fn Arc for reuse when constructing per-tool writers.
    pub fn emit_fn_clone(&self) -> Arc<dyn Fn(Value) -> bool + Send + Sync> {
        self.emit_fn.clone()
    }
}

impl Debug for ToolStreamWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolStreamWriter")
            .field("emit_fn", &"<fn>")
            .finish()
    }
}

impl Default for ToolStreamWriter {
    fn default() -> Self {
        Self::noop()
    }
}
