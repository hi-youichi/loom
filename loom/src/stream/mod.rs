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

pub mod message;
pub mod metadata;
pub mod sender;
pub mod stream_event;
pub mod stream_mode;
pub mod writers;

pub use message::{MessageChunk, MessageChunkKind};
pub use metadata::{CheckpointEvent, StreamMetadata};
pub use sender::ChunkToStreamSender;
pub use stream_event::StreamEvent;
pub use stream_mode::StreamMode;
pub use writers::{StreamWriter, ToolStreamWriter};

// Test modules
#[cfg(test)]
mod tests {
    pub mod integration_tests;
    pub mod stream_event_tests;
    pub mod stream_mode_tests;
    pub mod writer_tests;
}
