//! Streaming types for Loom runs.
//!
//! Defines stream modes, events, and StreamWriter for value, update, message, and custom
//! streaming. Used by `CompiledStateGraph::stream` and nodes that emit
//! incremental results.
//!
//! # StreamWriter
//!
//! The `StreamWriter` struct provides a convenient API for nodes and tools to emit
//! custom streaming events. It encapsulates the stream sender and mode checking logic.
//!
//! ```rust,ignore
//! use loom::stream::{StreamWriter, StreamMode};
//!
//! // In a node's run_with_context method:
//! async fn run_with_context(&self, state: S, ctx: &RunContext<S>) -> Result<(S, Next), AgentError> {
//!     let writer = StreamWriter::from_context(ctx);
//!     
//!     // Send custom data (only if Custom mode is enabled)
//!     writer.emit_custom(serde_json::json!({"progress": 50})).await;
//!     
//!     // Send message chunk (only if Messages mode is enabled)
//!     writer.emit_message("Hello", "think").await;
//!     
//!     Ok((state, Next::Continue))
//! }
//! ```

use serde_json::Value;
use std::collections::HashSet;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::mpsc;

// ============================================================================
// ToolStreamWriter - Type-erased writer for tools
// ============================================================================

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
        }
    }

    /// Creates a no-op ToolStreamWriter that does nothing.
    ///
    /// Useful when streaming is not enabled but code still needs a writer.
    pub fn noop() -> Self {
        Self {
            emit_fn: Arc::new(|_| false),
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

/// Stream mode selector: which kinds of events to emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StreamMode {
    /// Emit full state after each node completes.
    Values,
    /// Emit incremental updates with node id and state.
    Updates,
    /// Emit message chunks (LLM streaming).
    Messages,
    /// Emit custom JSON payloads from nodes or tools.
    Custom,
    /// Emit checkpoint events when checkpoints are created.
    Checkpoints,
    /// Emit task start/end events for each node execution.
    Tasks,
    /// Emit both checkpoints and tasks events (debug mode).
    Debug,
}

/// Metadata attached to streamed messages.
#[derive(Clone, Debug)]
pub struct StreamMetadata {
    /// Loom node id that produced the message.
    pub loom_node: String,
}

/// Checkpoint event emitted when a checkpoint is created.
///
/// Contains the checkpoint id, metadata, and optionally the state snapshot.
/// This aligns with graph-based checkpoint streaming format.
#[derive(Clone, Debug)]
pub struct CheckpointEvent<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Unique checkpoint identifier.
    pub checkpoint_id: String,
    /// Timestamp when checkpoint was created.
    pub timestamp: String,
    /// Step number in the graph execution (-1 for input, 0+ for loop).
    pub step: i64,
    /// The state snapshot at this checkpoint.
    pub state: S,
    /// Thread ID associated with this checkpoint.
    pub thread_id: Option<String>,
    /// Checkpoint namespace (for subgraphs).
    pub checkpoint_ns: Option<String>,
}

/// One chunk of streamed message content.
#[derive(Clone, Debug)]
pub struct MessageChunk {
    pub content: String,
}

/// Adapter that converts `MessageChunk` into `StreamEvent::Messages` and sends to `stream_tx`.
///
/// Used by ThinkNode (and similar nodes) to avoid manual channel setup and forward loops.
/// Call `channel()` to get (chunk_tx, chunk_rx), pass `chunk_tx` to `invoke_stream`, then
/// `forward(chunk_rx)` alongside it with `tokio::join!` so all chunks are forwarded before return.
pub struct ChunkToStreamSender<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    stream_tx: mpsc::Sender<StreamEvent<S>>,
    node_id: String,
}

impl<S> ChunkToStreamSender<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    pub fn new(stream_tx: mpsc::Sender<StreamEvent<S>>, node_id: impl Into<String>) -> Self {
        Self {
            stream_tx,
            node_id: node_id.into(),
        }
    }

    /// Returns (chunk_tx, chunk_rx). Pass chunk_tx to `invoke_stream`, then await
    /// `forward(chunk_rx)` together with invoke_stream via `tokio::join!` so forwarding
    /// completes before the caller returns.
    pub fn channel(&self) -> (mpsc::Sender<MessageChunk>, mpsc::Receiver<MessageChunk>) {
        mpsc::channel::<MessageChunk>(128)
    }

    /// Forwards chunks from `chunk_rx` to `stream_tx` as `StreamEvent::Messages`.
    /// Completes when `chunk_rx` is closed (e.g. when invoke_stream drops its sender).
    pub async fn forward(
        &self,
        mut chunk_rx: mpsc::Receiver<MessageChunk>,
    ) {
        let stream_tx = self.stream_tx.clone();
        let node_id = self.node_id.clone();
        while let Some(chunk) = chunk_rx.recv().await {
            let event = StreamEvent::Messages {
                chunk,
                metadata: StreamMetadata {
                    loom_node: node_id.clone(),
                },
            };
            let _ = stream_tx.send(event).await;
        }
    }
}

/// Streamed event emitted while running a graph.
#[derive(Clone, Debug)]
pub enum StreamEvent<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Full state snapshot after a node finishes.
    Values(S),
    /// Incremental update with the node id and state after that node.
    Updates { node_id: String, state: S },
    /// Message chunk emitted by a node (e.g. ThinkNode streaming LLM output).
    Messages {
        chunk: MessageChunk,
        metadata: StreamMetadata,
    },
    /// Custom JSON payload for arbitrary streaming data.
    Custom(Value),
    /// Checkpoint event emitted when a checkpoint is created.
    Checkpoint(CheckpointEvent<S>),
    /// Task start event emitted when a node begins execution.
    TaskStart {
        /// Node ID that is starting execution.
        node_id: String,
    },
    /// Task end event emitted when a node finishes execution.
    TaskEnd {
        /// Node ID that finished execution.
        node_id: String,
        /// Result of the task: Ok(()) for success, Err(message) for failure.
        result: Result<(), String>,
    },
    /// ToT (Tree of Thoughts): expand node produced multiple candidates.
    TotExpand {
        /// Short summaries of each candidate thought for display.
        candidates: Vec<String>,
    },
    /// ToT: evaluate node chose one candidate and assigned scores.
    TotEvaluate {
        /// Index of the chosen candidate.
        chosen: usize,
        /// Score per candidate (same order as candidates).
        scores: Vec<f32>,
    },
    /// ToT: backtrack node is returning to a previous depth.
    TotBacktrack {
        /// Human-readable reason for backtracking.
        reason: String,
        /// Depth we are backtracking to.
        to_depth: u32,
    },
    /// GoT (Graph of Thoughts): plan_graph node produced a DAG.
    GotPlan {
        /// Number of nodes in the task graph.
        node_count: usize,
        /// Number of edges (dependencies).
        edge_count: usize,
        /// Optional summary of node ids for display.
        node_ids: Vec<String>,
    },
    /// GoT: execute_graph started executing a task node.
    GotNodeStart {
        /// Task node id.
        node_id: String,
    },
    /// GoT: execute_graph completed a task node.
    GotNodeComplete {
        /// Task node id.
        node_id: String,
        /// Short summary of result (e.g. first 200 chars).
        result_summary: String,
    },
    /// GoT: execute_graph marked a task node as failed.
    GotNodeFailed {
        /// Task node id.
        node_id: String,
        /// Error message.
        error: String,
    },
    /// AGoT: a node was expanded into a subgraph (dynamic DAG extension).
    GotExpand {
        /// Node id that triggered the expansion.
        node_id: String,
        /// Number of new nodes added.
        nodes_added: usize,
        /// Number of new edges added.
        edges_added: usize,
    },
    /// LLM token usage for the last completion (e.g. after think node).
    /// Emitted when the provider returns usage (e.g. OpenAI); consumers can print when verbose.
    Usage {
        /// Tokens in the prompt (input).
        prompt_tokens: u32,
        /// Tokens in the completion (output).
        completion_tokens: u32,
        /// Total tokens (prompt + completion).
        total_tokens: u32,
    },
}

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
                chunk: MessageChunk {
                    content: content.into(),
                },
                metadata: StreamMetadata {
                    loom_node: node_id.into(),
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
                chunk: MessageChunk {
                    content: content.into(),
                },
                metadata: StreamMetadata {
                    loom_node: node_id.into(),
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
    pub async fn emit_updates(&self, node_id: impl Into<String>, state: S) -> bool {
        if !self.modes.contains(&StreamMode::Updates) {
            return false;
        }
        if let Some(tx) = &self.tx {
            let event = StreamEvent::Updates {
                node_id: node_id.into(),
                state,
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
    pub async fn emit_task_start(&self, node_id: impl Into<String>) -> bool {
        if !self.modes.contains(&StreamMode::Tasks) && !self.modes.contains(&StreamMode::Debug) {
            return false;
        }
        if let Some(tx) = &self.tx {
            let event = StreamEvent::TaskStart {
                node_id: node_id.into(),
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
    ) -> bool {
        if !self.modes.contains(&StreamMode::Tasks) && !self.modes.contains(&StreamMode::Debug) {
            return false;
        }
        if let Some(tx) = &self.tx {
            let event = StreamEvent::TaskEnd {
                node_id: node_id.into(),
                result,
            };
            tx.send(event).await.is_ok()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use tokio::sync::mpsc;

    #[derive(Clone, Debug, PartialEq)]
    struct DummyState(i32);

    /// **Scenario**: StreamMode seven variants are distinct, Eq, and usable in HashSet.
    #[test]
    fn stream_mode_four_variants_hashset_equality() {
        let v = StreamMode::Values;
        let u = StreamMode::Updates;
        let m = StreamMode::Messages;
        let c = StreamMode::Custom;
        let cp = StreamMode::Checkpoints;
        let t = StreamMode::Tasks;
        let d = StreamMode::Debug;
        assert_eq!(v, StreamMode::Values);
        assert_ne!(v, u);
        assert_ne!(u, m);
        assert_ne!(m, c);
        assert_ne!(c, v);
        assert_ne!(cp, v);
        assert_ne!(cp, u);
        assert_ne!(cp, m);
        assert_ne!(cp, c);
        assert_ne!(t, v);
        assert_ne!(t, cp);
        assert_ne!(d, t);
        let set: HashSet<StreamMode> = [v, u, m, c, cp, t, d].into_iter().collect();
        assert_eq!(set.len(), 7, "all seven modes distinct in HashSet");
        assert!(set.contains(&StreamMode::Values));
        assert!(set.contains(&StreamMode::Custom));
        assert!(set.contains(&StreamMode::Checkpoints));
        assert!(set.contains(&StreamMode::Tasks));
        assert!(set.contains(&StreamMode::Debug));
    }

    /// **Scenario**: StreamEvent variants carry expected data.
    #[test]
    fn stream_event_variants_hold_data() {
        let values = StreamEvent::Values(DummyState(1));
        match values {
            StreamEvent::Values(DummyState(v)) => assert_eq!(v, 1),
            _ => panic!("expected Values variant"),
        }

        let updates = StreamEvent::Updates {
            node_id: "n1".into(),
            state: DummyState(2),
        };
        match updates {
            StreamEvent::Updates { node_id, state } => {
                assert_eq!(node_id, "n1");
                assert_eq!(state, DummyState(2));
            }
            _ => panic!("expected Updates variant"),
        }

        let messages: StreamEvent<DummyState> = StreamEvent::Messages {
            chunk: MessageChunk {
                content: "chunk".into(),
            },
            metadata: StreamMetadata {
                loom_node: "think".into(),
            },
        };
        match messages {
            StreamEvent::Messages { chunk, metadata } => {
                assert_eq!(chunk.content, "chunk");
                assert_eq!(metadata.loom_node, "think");
            }
            _ => panic!("expected Messages variant"),
        }

        let custom: StreamEvent<DummyState> = StreamEvent::Custom(serde_json::json!({"k": "v"}));
        match custom {
            StreamEvent::Custom(v) => assert_eq!(v["k"], "v"),
            _ => panic!("expected Custom variant"),
        }

        let checkpoint: StreamEvent<DummyState> = StreamEvent::Checkpoint(CheckpointEvent {
            checkpoint_id: "cp-123".into(),
            timestamp: "1234567890".into(),
            step: 5,
            state: DummyState(42),
            thread_id: Some("thread-1".into()),
            checkpoint_ns: None,
        });
        match checkpoint {
            StreamEvent::Checkpoint(cp) => {
                assert_eq!(cp.checkpoint_id, "cp-123");
                assert_eq!(cp.timestamp, "1234567890");
                assert_eq!(cp.step, 5);
                assert_eq!(cp.state, DummyState(42));
                assert_eq!(cp.thread_id, Some("thread-1".into()));
                assert!(cp.checkpoint_ns.is_none());
            }
            _ => panic!("expected Checkpoint variant"),
        }

        let task_start: StreamEvent<DummyState> = StreamEvent::TaskStart {
            node_id: "think".into(),
        };
        match task_start {
            StreamEvent::TaskStart { node_id } => assert_eq!(node_id, "think"),
            _ => panic!("expected TaskStart variant"),
        }

        let task_end_ok: StreamEvent<DummyState> = StreamEvent::TaskEnd {
            node_id: "act".into(),
            result: Ok(()),
        };
        match task_end_ok {
            StreamEvent::TaskEnd { node_id, result } => {
                assert_eq!(node_id, "act");
                assert!(result.is_ok());
            }
            _ => panic!("expected TaskEnd variant"),
        }

        let task_end_err: StreamEvent<DummyState> = StreamEvent::TaskEnd {
            node_id: "failing".into(),
            result: Err("execution failed".into()),
        };
        match task_end_err {
            StreamEvent::TaskEnd { node_id, result } => {
                assert_eq!(node_id, "failing");
                assert!(result.is_err());
                assert_eq!(result.unwrap_err(), "execution failed");
            }
            _ => panic!("expected TaskEnd variant"),
        }

        let tot_expand: StreamEvent<DummyState> = StreamEvent::TotExpand {
            candidates: vec!["a".into(), "b".into()],
        };
        match tot_expand {
            StreamEvent::TotExpand { candidates } => assert_eq!(candidates.len(), 2),
            _ => panic!("expected TotExpand variant"),
        }

        let tot_eval: StreamEvent<DummyState> = StreamEvent::TotEvaluate {
            chosen: 1,
            scores: vec![0.2, 0.8],
        };
        match tot_eval {
            StreamEvent::TotEvaluate { chosen, scores } => {
                assert_eq!(chosen, 1);
                assert_eq!(scores.len(), 2);
            }
            _ => panic!("expected TotEvaluate variant"),
        }

        let tot_bt: StreamEvent<DummyState> = StreamEvent::TotBacktrack {
            reason: "bad path".into(),
            to_depth: 0,
        };
        match tot_bt {
            StreamEvent::TotBacktrack { reason, to_depth } => {
                assert_eq!(reason, "bad path");
                assert_eq!(to_depth, 0);
            }
            _ => panic!("expected TotBacktrack variant"),
        }

        let got_plan: StreamEvent<DummyState> = StreamEvent::GotPlan {
            node_count: 3,
            edge_count: 2,
            node_ids: vec!["a".into(), "b".into(), "c".into()],
        };
        match got_plan {
            StreamEvent::GotPlan {
                node_count,
                edge_count,
                node_ids,
            } => {
                assert_eq!(node_count, 3);
                assert_eq!(edge_count, 2);
                assert_eq!(node_ids.len(), 3);
            }
            _ => panic!("expected GotPlan variant"),
        }

        let got_start: StreamEvent<DummyState> = StreamEvent::GotNodeStart {
            node_id: "n1".into(),
        };
        match got_start {
            StreamEvent::GotNodeStart { node_id } => assert_eq!(node_id, "n1"),
            _ => panic!("expected GotNodeStart variant"),
        }

        let got_ok: StreamEvent<DummyState> = StreamEvent::GotNodeComplete {
            node_id: "n1".into(),
            result_summary: "done".into(),
        };
        match got_ok {
            StreamEvent::GotNodeComplete {
                node_id,
                result_summary,
            } => {
                assert_eq!(node_id, "n1");
                assert_eq!(result_summary, "done");
            }
            _ => panic!("expected GotNodeComplete variant"),
        }

        let got_fail: StreamEvent<DummyState> = StreamEvent::GotNodeFailed {
            node_id: "n2".into(),
            error: "tool error".into(),
        };
        match got_fail {
            StreamEvent::GotNodeFailed { node_id, error } => {
                assert_eq!(node_id, "n2");
                assert_eq!(error, "tool error");
            }
            _ => panic!("expected GotNodeFailed variant"),
        }

        let got_expand: StreamEvent<DummyState> = StreamEvent::GotExpand {
            node_id: "n1".into(),
            nodes_added: 2,
            edges_added: 2,
        };
        match got_expand {
            StreamEvent::GotExpand {
                node_id,
                nodes_added,
                edges_added,
            } => {
                assert_eq!(node_id, "n1");
                assert_eq!(nodes_added, 2);
                assert_eq!(edges_added, 2);
            }
            _ => panic!("expected GotExpand variant"),
        }
    }

    // === StreamWriter Tests ===

    /// **Scenario**: StreamWriter::noop creates a writer that does nothing.
    #[test]
    fn stream_writer_noop_does_nothing() {
        let writer: StreamWriter<DummyState> = StreamWriter::noop();
        assert!(!writer.is_mode_enabled(StreamMode::Custom));
        assert!(!writer.is_mode_enabled(StreamMode::Messages));
        assert!(writer.sender().is_none());
    }

    /// **Scenario**: StreamWriter::emit_custom only sends when Custom mode is enabled.
    #[tokio::test]
    async fn stream_writer_emit_custom_respects_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // Without Custom mode - should not send
        let modes_without_custom = HashSet::from_iter([StreamMode::Values]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_without_custom);
        let sent = writer.emit_custom(serde_json::json!({"test": 1})).await;
        assert!(!sent, "should not send when Custom mode is disabled");

        // With Custom mode - should send
        let modes_with_custom = HashSet::from_iter([StreamMode::Custom]);
        let writer = StreamWriter::new(Some(tx), modes_with_custom);
        let sent = writer.emit_custom(serde_json::json!({"test": 2})).await;
        assert!(sent, "should send when Custom mode is enabled");

        // Verify the event
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::Custom(v) => assert_eq!(v["test"], 2),
            _ => panic!("expected Custom event"),
        }
    }

    /// **Scenario**: StreamWriter::emit_message only sends when Messages mode is enabled.
    #[tokio::test]
    async fn stream_writer_emit_message_respects_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // Without Messages mode - should not send
        let modes_without_messages = HashSet::from_iter([StreamMode::Values]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_without_messages);
        let sent = writer.emit_message("content", "node1").await;
        assert!(!sent, "should not send when Messages mode is disabled");

        // With Messages mode - should send
        let modes_with_messages = HashSet::from_iter([StreamMode::Messages]);
        let writer = StreamWriter::new(Some(tx), modes_with_messages);
        let sent = writer.emit_message("hello", "think").await;
        assert!(sent, "should send when Messages mode is enabled");

        // Verify the event
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::Messages { chunk, metadata } => {
                assert_eq!(chunk.content, "hello");
                assert_eq!(metadata.loom_node, "think");
            }
            _ => panic!("expected Messages event"),
        }
    }

    /// **Scenario**: StreamWriter::emit_values only sends when Values mode is enabled.
    #[tokio::test]
    async fn stream_writer_emit_values_respects_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // With Values mode - should send
        let modes = HashSet::from_iter([StreamMode::Values]);
        let writer = StreamWriter::new(Some(tx), modes);
        let sent = writer.emit_values(DummyState(42)).await;
        assert!(sent, "should send when Values mode is enabled");

        // Verify the event
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::Values(s) => assert_eq!(s, DummyState(42)),
            _ => panic!("expected Values event"),
        }
    }

    /// **Scenario**: StreamWriter::emit_updates only sends when Updates mode is enabled.
    #[tokio::test]
    async fn stream_writer_emit_updates_respects_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // With Updates mode - should send
        let modes = HashSet::from_iter([StreamMode::Updates]);
        let writer = StreamWriter::new(Some(tx), modes);
        let sent = writer.emit_updates("node1", DummyState(100)).await;
        assert!(sent, "should send when Updates mode is enabled");

        // Verify the event
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::Updates { node_id, state } => {
                assert_eq!(node_id, "node1");
                assert_eq!(state, DummyState(100));
            }
            _ => panic!("expected Updates event"),
        }
    }

    /// **Scenario**: StreamWriter try_emit methods work without awaiting.
    #[test]
    fn stream_writer_try_emit_non_blocking() {
        let (tx, _rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        let modes = HashSet::from_iter([StreamMode::Custom, StreamMode::Messages]);
        let writer = StreamWriter::new(Some(tx), modes);

        let sent = writer.try_emit_custom(serde_json::json!({"sync": true}));
        assert!(sent, "try_emit_custom should work");

        let sent = writer.try_emit_message("sync content", "sync_node");
        assert!(sent, "try_emit_message should work");
    }

    /// **Scenario**: StreamWriter without sender returns false for all emit methods.
    #[tokio::test]
    async fn stream_writer_no_sender_returns_false() {
        let modes = HashSet::from_iter([
            StreamMode::Custom,
            StreamMode::Messages,
            StreamMode::Values,
            StreamMode::Updates,
            StreamMode::Checkpoints,
            StreamMode::Tasks,
            StreamMode::Debug,
        ]);
        let writer: StreamWriter<DummyState> = StreamWriter::new(None, modes);

        assert!(!writer.emit_custom(serde_json::json!({})).await);
        assert!(!writer.emit_message("", "").await);
        assert!(!writer.emit_values(DummyState(0)).await);
        assert!(!writer.emit_updates("", DummyState(0)).await);
        assert!(
            !writer
                .emit_checkpoint("", "", 0, DummyState(0), None, None)
                .await
        );
        assert!(!writer.emit_task_start("").await);
        assert!(!writer.emit_task_end("", Ok(())).await);
    }

    /// **Scenario**: StreamWriter::emit_checkpoint only sends when Checkpoints mode is enabled.
    #[tokio::test]
    async fn stream_writer_emit_checkpoint_respects_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // Without Checkpoints mode - should not send
        let modes_without_checkpoints = HashSet::from_iter([StreamMode::Values]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_without_checkpoints);
        let sent = writer
            .emit_checkpoint("cp-1", "123", 1, DummyState(10), None, None)
            .await;
        assert!(!sent, "should not send when Checkpoints mode is disabled");

        // With Checkpoints mode - should send
        let modes_with_checkpoints = HashSet::from_iter([StreamMode::Checkpoints]);
        let writer = StreamWriter::new(Some(tx), modes_with_checkpoints);
        let sent = writer
            .emit_checkpoint(
                "cp-2",
                "456",
                2,
                DummyState(20),
                Some("thread-1".into()),
                Some("ns-1".into()),
            )
            .await;
        assert!(sent, "should send when Checkpoints mode is enabled");

        // Verify the event
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::Checkpoint(cp) => {
                assert_eq!(cp.checkpoint_id, "cp-2");
                assert_eq!(cp.timestamp, "456");
                assert_eq!(cp.step, 2);
                assert_eq!(cp.state, DummyState(20));
                assert_eq!(cp.thread_id, Some("thread-1".into()));
                assert_eq!(cp.checkpoint_ns, Some("ns-1".into()));
            }
            _ => panic!("expected Checkpoint event"),
        }
    }

    /// **Scenario**: StreamWriter::emit_task_start only sends when Tasks or Debug mode is enabled.
    #[tokio::test]
    async fn stream_writer_emit_task_start_respects_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // Without Tasks mode - should not send
        let modes_without_tasks = HashSet::from_iter([StreamMode::Values]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_without_tasks);
        let sent = writer.emit_task_start("node1").await;
        assert!(!sent, "should not send when Tasks mode is disabled");

        // With Tasks mode - should send
        let modes_with_tasks = HashSet::from_iter([StreamMode::Tasks]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_with_tasks);
        let sent = writer.emit_task_start("think").await;
        assert!(sent, "should send when Tasks mode is enabled");

        // Verify the event
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::TaskStart { node_id } => {
                assert_eq!(node_id, "think");
            }
            _ => panic!("expected TaskStart event"),
        }

        // With Debug mode - should also send (debug includes tasks)
        let modes_with_debug = HashSet::from_iter([StreamMode::Debug]);
        let writer = StreamWriter::new(Some(tx), modes_with_debug);
        let sent = writer.emit_task_start("act").await;
        assert!(sent, "should send when Debug mode is enabled");

        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::TaskStart { node_id } => {
                assert_eq!(node_id, "act");
            }
            _ => panic!("expected TaskStart event"),
        }
    }

    /// **Scenario**: StreamWriter::emit_task_end only sends when Tasks or Debug mode is enabled.
    #[tokio::test]
    async fn stream_writer_emit_task_end_respects_mode() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(8);

        // Without Tasks mode - should not send
        let modes_without_tasks = HashSet::from_iter([StreamMode::Values]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_without_tasks);
        let sent = writer.emit_task_end("node1", Ok(())).await;
        assert!(!sent, "should not send when Tasks mode is disabled");

        // With Tasks mode - should send success
        let modes_with_tasks = HashSet::from_iter([StreamMode::Tasks]);
        let writer = StreamWriter::new(Some(tx.clone()), modes_with_tasks);
        let sent = writer.emit_task_end("think", Ok(())).await;
        assert!(sent, "should send when Tasks mode is enabled");

        // Verify the success event
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::TaskEnd { node_id, result } => {
                assert_eq!(node_id, "think");
                assert!(result.is_ok());
            }
            _ => panic!("expected TaskEnd event"),
        }

        // With Tasks mode - should send failure
        let sent = writer
            .emit_task_end("act", Err("execution failed".into()))
            .await;
        assert!(sent, "should send failure when Tasks mode is enabled");

        // Verify the failure event
        let event = rx.recv().await.expect("should receive event");
        match event {
            StreamEvent::TaskEnd { node_id, result } => {
                assert_eq!(node_id, "act");
                assert!(result.is_err());
                assert_eq!(result.unwrap_err(), "execution failed");
            }
            _ => panic!("expected TaskEnd event"),
        }
    }

    /// **Scenario**: StreamWriter is Clone and can be used in multiple tasks.
    #[tokio::test]
    async fn stream_writer_is_clone() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<DummyState>>(16);
        let modes = HashSet::from_iter([StreamMode::Custom]);
        let writer = StreamWriter::new(Some(tx), modes);

        // Clone the writer
        let writer2 = writer.clone();

        // Use both writers in parallel
        let t1 = tokio::spawn(async move {
            writer
                .emit_custom(serde_json::json!({"from": "writer1"}))
                .await
        });
        let t2 = tokio::spawn(async move {
            writer2
                .emit_custom(serde_json::json!({"from": "writer2"}))
                .await
        });

        let (r1, r2) = tokio::join!(t1, t2);
        assert!(r1.unwrap());
        assert!(r2.unwrap());

        // Verify both events were received
        let mut events = vec![];
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        assert_eq!(
            events.len(),
            2,
            "should receive 2 events from cloned writers"
        );
    }

    /// **Scenario**: StreamWriter Debug implementation shows useful info.
    #[test]
    fn stream_writer_debug_impl() {
        let modes = HashSet::from_iter([StreamMode::Custom]);
        let writer: StreamWriter<DummyState> = StreamWriter::new(None, modes);
        let debug = format!("{:?}", writer);
        assert!(debug.contains("StreamWriter"));
        assert!(debug.contains("has_sender"));
        assert!(debug.contains("modes"));
    }

    // === ToolStreamWriter Tests ===

    /// **Scenario**: ToolStreamWriter::noop creates a writer that always returns false.
    #[test]
    fn tool_stream_writer_noop_returns_false() {
        let writer = ToolStreamWriter::noop();
        let sent = writer.emit_custom(serde_json::json!({"test": 1}));
        assert!(!sent, "noop writer should return false");
    }

    /// **Scenario**: ToolStreamWriter::new creates a working writer.
    #[test]
    fn tool_stream_writer_new_emits_via_function() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let writer = ToolStreamWriter::new(move |_value| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            true
        });

        assert!(writer.emit_custom(serde_json::json!({"a": 1})));
        assert!(writer.emit_custom(serde_json::json!({"b": 2})));
        assert!(writer.emit_custom(serde_json::json!({"c": 3})));

        assert_eq!(
            counter.load(Ordering::SeqCst),
            3,
            "emit_fn should be called 3 times"
        );
    }

    /// **Scenario**: ToolStreamWriter is Clone and can be used in multiple places.
    #[test]
    fn tool_stream_writer_is_clone() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let writer = ToolStreamWriter::new(move |_| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            true
        });

        let writer2 = writer.clone();

        writer.emit_custom(serde_json::json!(1));
        writer2.emit_custom(serde_json::json!(2));

        assert_eq!(counter.load(Ordering::SeqCst), 2, "both clones should emit");
    }

    /// **Scenario**: ToolStreamWriter Debug implementation works.
    #[test]
    fn tool_stream_writer_debug_impl() {
        let writer = ToolStreamWriter::noop();
        let debug = format!("{:?}", writer);
        assert!(debug.contains("ToolStreamWriter"));
    }

    /// **Scenario**: ToolStreamWriter Default creates a noop writer.
    #[test]
    fn tool_stream_writer_default_is_noop() {
        let writer = ToolStreamWriter::default();
        let sent = writer.emit_custom(serde_json::json!({}));
        assert!(!sent, "default writer should be noop");
    }
}
