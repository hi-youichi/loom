//! Streaming types for Loom runs.
//!
//! Core types are defined in the `stream-event` crate.
//! This module re-exports them and adds Loom-specific utilities.

pub mod openai_sse;
pub mod sender;
pub mod writers;

// Re-export core types from stream-event crate
pub use stream_event::{
    CheckpointEvent, MessageChunk, MessageChunkKind, StreamEvent, StreamMetadata, StreamMode,
    StreamWriter,
};

// Loom-specific types
pub use sender::ChunkToStreamSender;
pub use writers::tool_stream_writer::ToolStreamWriter;

#[cfg(test)]
mod tests {
    pub mod integration_tests;
    pub mod stream_event_tests;
    pub mod stream_mode_tests;
    pub mod writer_tests;
}
