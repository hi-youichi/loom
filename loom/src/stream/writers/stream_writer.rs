use super::super::{CheckpointEvent, MessageChunk, StreamEvent, StreamMetadata, StreamMode};
use serde_json::Value;
use std::collections::HashSet;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::mpsc;

/// A writer for emitting streaming events from nodes and tools.
///
/// `StreamWriter` encapsulates the stream sender and mode checking, providing
/// a convenient API for nodes and tools to emit custom events without manually
/// checking `stream_mode` and handling the sender.
///
/// # Usage
///
/// Nodes should create a `StreamWriter` from the `RunContext` and use it to
/// emit events during execution:
///
/// ```rust,ignore
/// use loom::stream::StreamWriter;
///
/// async fn run_with_context(&self, state: S, ctx: &RunContext<S>) -> Result<(S, Next), AgentError> {
///     let writer = StreamWriter::from_context(ctx);
///     
///     // Emit progress (only sent if Custom mode is enabled)
///     writer.emit_custom(serde_json::json!({"status": "processing"})).await;
///     
///     // Do work...
///     
///     writer.emit_custom(serde_json::json!({"status": "done"})).await;
///     Ok((state, Next::Continue))
/// }
/// ```
///
/// # Thread Safety
///
/// `StreamWriter` is `Clone + Send + Sync`, so it can be safely shared across
/// async tasks or threads. Multiple writers can emit events concurrently.
#[derive(Clone)]
pub struct StreamWriter<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// The sender for stream events (None if streaming is not active).
    tx: Option<mpsc::Sender<StreamEvent<S>>>,
    /// The enabled stream modes.
    modes: Arc<HashSet<StreamMode>>,
}

impl<S> StreamWriter<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Creates a new StreamWriter with the given sender and modes.
    ///
    /// # Arguments
    ///
    /// * `tx` - Optional sender for stream events
    /// * `modes` - Set of enabled stream modes
    pub fn new(tx: Option<mpsc::Sender<StreamEvent<S>>>, modes: HashSet<StreamMode>) -> Self {
        Self {
            tx,
            modes: Arc::new(modes),
        }
    }

    /// Creates a StreamWriter that does nothing (no-op writer).
    ///
    /// Useful when streaming is not enabled but code still needs a writer.
    pub fn noop() -> Self {
        Self {
            tx: None,
            modes: Arc::new(HashSet::new()),
        }
    }

    /// Checks if a specific stream mode is enabled.
    pub fn is_mode_enabled(&self, mode: StreamMode) -> bool {
        self.modes.contains(&mode)
    }

    /// Emits a custom JSON payload.
    ///
    /// Only sends if `StreamMode::Custom` is enabled and a sender is available.
    /// Returns `true` if the event was sent, `false` otherwise.
    ///
    /// # Arguments
    ///
    /// * `value` - The JSON value to emit
    pub async fn emit_custom(&self, value: Value) -> bool {
        if !self.modes.contains(&StreamMode::Custom) {
            return false;
        }
        if let Some(tx) = &self.tx {
            tx.send(StreamEvent::Custom(value)).await.is_ok()
        } else {
            false
        }
    }

    /// Emits a custom JSON payload (non-blocking version).
    ///
    /// Uses `try_send` instead of `send`, which does not await.
    /// Useful in sync contexts or when you don't want to block.
    ///
    /// Returns `true` if the event was sent, `false` otherwise.
    pub fn try_emit_custom(&self, value: Value) -> bool {
        if !self.modes.contains(&StreamMode::Custom) {
            return false;
        }
        if let Some(tx) = &self.tx {
            tx.try_send(StreamEvent::Custom(value)).is_ok()
        } else {
            false
        }
    }

    /// Emits a message chunk (LLM token).
    ///
    /// Only sends if `StreamMode::Messages` is enabled and a sender is available.
    /// Returns `true` if the event was sent, `false` otherwise.
    ///
    /// # Arguments
    ///
    /// * `content` - The message chunk content
    /// * `node_id` - The node ID that produced this message
    pub async fn emit_message(
        &self,
        content: impl Into<String>,
        node_id: impl Into<String>,
    ) -> bool {
        if !self.modes.contains(&StreamMode::Messages) {
            return false;
        }
        if let Some(tx) = &self.tx {
            let event = StreamEvent::Messages {
                chunk: MessageChunk::message(content.into()),
                metadata: StreamMetadata {
                    loom_node: node_id.into(),
                    namespace: None,
                },
            };
            tx.send(event).await.is_ok()
        } else {
            false
        }
    }

    /// Emits a message chunk (non-blocking version).
    ///
    /// Uses `try_send` instead of `send`.
    pub fn try_emit_message(&self, content: impl Into<String>, node_id: impl Into<String>) -> bool {
        if !self.modes.contains(&StreamMode::Messages) {
            return false;
        }
        if let Some(tx) = &self.tx {
            let event = StreamEvent::Messages {
                chunk: MessageChunk::message(content.into()),
                metadata: StreamMetadata {
                    loom_node: node_id.into(),
                    namespace: None,
                },
            };
            tx.try_send(event).is_ok()
        } else {
            false
        }
    }

    /// Emits a full state value.
    ///
    /// Only sends if `StreamMode::Values` is enabled and a sender is available.
    /// Returns `true` if the event was sent, `false` otherwise.
    ///
    /// Note: This is typically used by the graph execution loop, not by nodes directly.
    pub async fn emit_values(&self, state: S) -> bool {
        if !self.modes.contains(&StreamMode::Values) {
            return false;
        }
        if let Some(tx) = &self.tx {
            tx.send(StreamEvent::Values(state)).await.is_ok()
        } else {
            false
        }
    }

    /// Emits an incremental update.
    ///
    /// Only sends if `StreamMode::Updates` is enabled and a sender is available.
    /// Returns `true` if the event was sent, `false` otherwise.
    ///
    /// Note: This is typically used by the graph execution loop, not by nodes directly.
    pub async fn emit_updates(
        &self,
        node_id: impl Into<String>,
        state: S,
        namespace: Option<String>,
    ) -> bool {
        if !self.modes.contains(&StreamMode::Updates) {
            return false;
        }
        if let Some(tx) = &self.tx {
            let event = StreamEvent::Updates {
                node_id: node_id.into(),
                state,
                namespace,
            };
            tx.send(event).await.is_ok()
        } else {
            false
        }
    }

    /// Emits a checkpoint event.
    ///
    /// Only sends if `StreamMode::Checkpoints` or `StreamMode::Debug` is enabled and a sender is available.
    /// Returns `true` if the event was sent, `false` otherwise.
    ///
    /// Note: This is typically used by the graph execution loop after saving a checkpoint.
    ///
    /// # Arguments
    ///
    /// * `checkpoint_id` - Unique identifier for this checkpoint
    /// * `timestamp` - Timestamp when checkpoint was created
    /// * `step` - Step number in the graph execution (-1 for input, 0+ for loop)
    /// * `state` - The state snapshot at this checkpoint
    /// * `thread_id` - Optional thread ID
    /// * `checkpoint_ns` - Optional checkpoint namespace (for subgraphs)
    pub async fn emit_checkpoint(
        &self,
        checkpoint_id: impl Into<String>,
        timestamp: impl Into<String>,
        step: i64,
        state: S,
        thread_id: Option<String>,
        checkpoint_ns: Option<String>,
    ) -> bool {
        if !self.modes.contains(&StreamMode::Checkpoints)
            && !self.modes.contains(&StreamMode::Debug)
        {
            return false;
        }
        if let Some(tx) = &self.tx {
            let event = StreamEvent::Checkpoint(CheckpointEvent {
                checkpoint_id: checkpoint_id.into(),
                timestamp: timestamp.into(),
                step,
                state,
                thread_id,
                checkpoint_ns,
            });
            tx.send(event).await.is_ok()
        } else {
            false
        }
    }

    /// Emits a task start event.
    ///
    /// Only sends if `StreamMode::Tasks` or `StreamMode::Debug` is enabled and a sender is available.
    /// Returns `true` if the event was sent, `false` otherwise.
    ///
    /// Note: This is typically used by the graph execution loop before running a node.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The ID of the node that is starting execution
    pub async fn emit_task_start(
        &self,
        node_id: impl Into<String>,
        namespace: Option<String>,
    ) -> bool {
        if !self.modes.contains(&StreamMode::Tasks) && !self.modes.contains(&StreamMode::Debug) {
            return false;
        }
        if let Some(tx) = &self.tx {
            let event = StreamEvent::TaskStart {
                node_id: node_id.into(),
                namespace,
            };
            tx.send(event).await.is_ok()
        } else {
            false
        }
    }

    /// Emits a task end event.
    ///
    /// Only sends if `StreamMode::Tasks` or `StreamMode::Debug` is enabled and a sender is available.
    /// Returns `true` if the event was sent, `false` otherwise.
    ///
    /// Note: This is typically used by the graph execution loop after running a node.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The ID of the node that finished execution
    /// * `result` - Ok(()) for success, Err(message) for failure
    pub async fn emit_task_end(
        &self,
        node_id: impl Into<String>,
        result: Result<(), String>,
        namespace: Option<String>,
    ) -> bool {
        if !self.modes.contains(&StreamMode::Tasks) && !self.modes.contains(&StreamMode::Debug) {
            return false;
        }
        if let Some(tx) = &self.tx {
            let event = StreamEvent::TaskEnd {
                node_id: node_id.into(),
                result,
                namespace,
            };
            tx.send(event).await.is_ok()
        } else {
            false
        }
    }

    fn is_tools_enabled(&self) -> bool {
        self.modes.contains(&StreamMode::Tools) || self.modes.contains(&StreamMode::Debug)
    }

    pub async fn emit_tool_call_chunk(
        &self,
        call_id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    ) -> bool {
        if !self.is_tools_enabled() {
            return false;
        }
        if let Some(tx) = &self.tx {
            tx.send(StreamEvent::ToolCallChunk {
                call_id,
                name,
                arguments_delta,
            })
            .await
            .is_ok()
        } else {
            false
        }
    }

    pub async fn emit_tool_call(
        &self,
        call_id: Option<String>,
        name: String,
        arguments: Value,
    ) -> bool {
        if !self.is_tools_enabled() {
            return false;
        }
        if let Some(tx) = &self.tx {
            tx.send(StreamEvent::ToolCall {
                call_id,
                name,
                arguments,
            })
            .await
            .is_ok()
        } else {
            false
        }
    }

    pub async fn emit_tool_start(&self, call_id: Option<String>, name: String) -> bool {
        if !self.is_tools_enabled() {
            return false;
        }
        if let Some(tx) = &self.tx {
            tx.send(StreamEvent::ToolStart { call_id, name })
                .await
                .is_ok()
        } else {
            false
        }
    }

    pub async fn emit_tool_output(
        &self,
        call_id: Option<String>,
        name: String,
        content: String,
    ) -> bool {
        if !self.is_tools_enabled() {
            return false;
        }
        if let Some(tx) = &self.tx {
            tx.send(StreamEvent::ToolOutput {
                call_id,
                name,
                content,
            })
            .await
            .is_ok()
        } else {
            false
        }
    }

    pub async fn emit_tool_end(
        &self,
        call_id: Option<String>,
        name: String,
        result: String,
        is_error: bool,
        raw_result: Option<String>,
    ) -> bool {
        if !self.is_tools_enabled() {
            return false;
        }
        if let Some(tx) = &self.tx {
            tx.send(StreamEvent::ToolEnd {
                call_id,
                name,
                result,
                is_error,
                raw_result,
            })
            .await
            .is_ok()
        } else {
            false
        }
    }

    pub async fn emit_tool_approval(
        &self,
        call_id: Option<String>,
        name: String,
        arguments: Value,
    ) -> bool {
        if !self.is_tools_enabled() {
            return false;
        }
        if let Some(tx) = &self.tx {
            tx.send(StreamEvent::ToolApproval {
                call_id,
                name,
                arguments,
            })
            .await
            .is_ok()
        } else {
            false
        }
    }

    /// Returns the raw sender if available.
    ///
    /// This allows advanced use cases where direct access to the sender is needed.
    pub fn sender(&self) -> Option<&mpsc::Sender<StreamEvent<S>>> {
        self.tx.as_ref()
    }

    /// Returns a reference to the enabled modes.
    pub fn modes(&self) -> &HashSet<StreamMode> {
        &self.modes
    }
}

impl<S> Debug for StreamWriter<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamWriter")
            .field("has_sender", &self.tx.is_some())
            .field("modes", &self.modes)
            .finish()
    }
}
