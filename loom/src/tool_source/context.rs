//! Context passed into tool calls for the current step.
//!
//! Used by short-term memory tools (e.g. `get_recent_messages`) that need access to
//! the current conversation. ActNode sets this via `ToolSource::set_call_context` before
//! executing tool calls.
//!
//! # Streaming Support
//!
//! `ToolCallContext` includes an optional `stream_writer` field that enables tools
//! to emit custom streaming events (e.g., progress updates, intermediate results)
//! during execution. The writer is provided by `ActNode` when streaming is enabled.
//!
//! ```rust,ignore
//! use loom::tool_source::ToolCallContext;
//! use serde_json::json;
//!
//! async fn my_tool(ctx: Option<&ToolCallContext>) -> String {
//!     if let Some(ctx) = ctx {
//!         if let Some(writer) = &ctx.stream_writer {
//!             writer.emit_custom(json!({"status": "starting"}));
//!         }
//!     }
//!     // Do work...
//!     "Result".to_string()
//! }
//! ```

use crate::message::Message;
use crate::stream::ToolStreamWriter;

/// Per-step context available to tools during execution.
///
/// Injected by ActNode before calling tools; implementations that need current
/// messages (e.g. ShortTermMemoryToolSource) read it in `call_tool`. Other
/// ToolSource implementations ignore it (default `set_call_context` is no-op).
///
/// **When tools get this context**: All tools registered on the graph's
/// [`ToolSource`](crate::tool_source::ToolSource) (e.g. via ActNode) receive
/// this context when the graph is run. The compiled graph passes a
/// [`RunContext`](crate::graph::RunContext) on both `invoke` and
/// `invoke_with_context`, so ActNode always has `thread_id`/`user_id` from
/// config available to fill into this struct.
///
/// # Fields
///
/// - `recent_messages`: Current conversation messages from state
/// - `stream_writer`: Optional writer for emitting custom streaming events
/// - `thread_id`: Optional thread/session id from [`RunnableConfig`](crate::memory::RunnableConfig); set by ActNode when running with RunContext. Use for session-scoped storage (e.g. todo per thread).
/// - `user_id`: Optional user id from RunnableConfig; use for multi-tenant or store namespace.
///
/// # Streaming
///
/// When streaming is enabled and `StreamMode::Custom` is active, `ActNode` provides
/// a `ToolStreamWriter` that tools can use to emit progress updates or intermediate
/// results. This enables real-time feedback during long-running tool operations.
///
/// **Interaction**: Set by ActNode via `ToolSource::set_call_context`; read by
/// `ShortTermMemoryToolSource::call_tool` to return recent messages; `stream_writer`
/// used by any tool that wants to emit custom streaming events.
#[derive(Debug, Clone, Default)]
pub struct ToolCallContext {
    /// Recent messages in the current conversation (current step's state.messages).
    pub recent_messages: Vec<Message>,

    /// Optional writer for emitting custom streaming events.
    ///
    /// This is provided by `ActNode` when streaming is enabled with `StreamMode::Custom`.
    /// Tools can use this to emit progress updates, intermediate results, or any
    /// custom JSON data during execution.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(writer) = &ctx.stream_writer {
    ///     writer.emit_custom(serde_json::json!({"progress": 50}));
    /// }
    /// ```
    pub stream_writer: Option<ToolStreamWriter>,

    /// Optional thread/session id for the current run.
    ///
    /// Injected by ActNode from `RunContext::config` when `run_with_context` is used.
    /// When set, tools can use it as a session key (e.g. per-thread todo storage).
    pub thread_id: Option<String>,

    /// Optional user id for the current run.
    ///
    /// Injected by ActNode from `RunContext::config` when `run_with_context` is used.
    /// Use for multi-tenant or store namespace. See RunnableConfig::user_id.
    pub user_id: Option<String>,
}

impl ToolCallContext {
    /// Creates a new ToolCallContext with the given messages.
    ///
    /// `stream_writer`, `thread_id`, and `user_id` are set to `None`.
    pub fn new(recent_messages: Vec<Message>) -> Self {
        Self {
            recent_messages,
            stream_writer: None,
            thread_id: None,
            user_id: None,
        }
    }

    /// Creates a new ToolCallContext with messages and a stream writer.
    ///
    /// `thread_id` and `user_id` are set to `None`. When running with RunContext,
    /// ActNode builds the context with thread_id/user_id from config.
    pub fn with_stream_writer(
        recent_messages: Vec<Message>,
        stream_writer: ToolStreamWriter,
    ) -> Self {
        Self {
            recent_messages,
            stream_writer: Some(stream_writer),
            thread_id: None,
            user_id: None,
        }
    }

    /// Emits a custom streaming event if a writer is available.
    ///
    /// This is a convenience method that checks if `stream_writer` is present
    /// and calls `emit_custom` on it. Returns `true` if the event was sent,
    /// `false` if no writer is available or sending failed.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let sent = ctx.emit_custom(serde_json::json!({"status": "processing"}));
    /// ```
    pub fn emit_custom(&self, value: serde_json::Value) -> bool {
        self.stream_writer
            .as_ref()
            .map(|w| w.emit_custom(value))
            .unwrap_or(false)
    }
}
