//! Streaming types for Loom runs.
//!
//! Core types are defined in the `stream-event` crate.
//! This module re-exports them and adds Loom-specific utilities.

pub mod state;
pub mod writers;

// Re-export core types from stream-event crate
pub use stream_event::{
    CheckpointEvent, MessageChunk, MessageChunkKind, StreamEvent, StreamEventSink,
    StreamMetadata, StreamMode, StreamWriter,
};

// Loom-specific types (to be removed in Phase 5)
pub mod any_event;
pub use any_event::TypedAnyStreamEvent;

// Re-export state types for convenience
pub use state::{
    ModelConfig, ReActState, ReActCheckpointMeta, ToolResult, ToolStorageRef,
    NormalizedToolOutput,
};

#[cfg(test)]
mod tests {
    pub mod integration_tests;
    pub mod stream_event_tests;
    pub mod stream_mode_tests;
}
