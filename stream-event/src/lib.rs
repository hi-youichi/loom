//! Stream event protocol (protocol_spec): type + payload + envelope.
//!
//! This crate defines the wire shape of a single stream event and envelope injection.
//! It does not depend on loom. Loom bridges `StreamEvent<S>` into `ProtocolEvent` and calls `to_json`.

pub mod codex;
pub mod envelope;
pub mod event;
pub mod sender;
pub mod stream_event;
pub mod stream_mode;
pub mod metadata;
pub mod message;
pub mod writers;

pub use codex::CodexEvent;
pub use envelope::{to_json, Envelope, EnvelopeState};
pub use event::ProtocolEvent;
pub use sender::ChunkToStreamSender;
pub use stream_event::StreamEvent;
pub use stream_mode::StreamMode;
pub use metadata::{CheckpointEvent, StreamMetadata};
pub use message::{MessageChunk, MessageChunkKind};
pub use writers::StreamWriter;
