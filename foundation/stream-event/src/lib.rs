//! Stream event protocol (protocol_spec): type + payload + envelope.
//!
//! This crate defines the wire shape of a single stream event and envelope injection.
//! It does not depend on loom. Loom bridges `StreamEvent<S>` into `ProtocolEvent` and calls `to_json`.

pub mod codex;
pub mod sink;
pub mod types;
pub mod wire;

pub use codex::CodexEvent;
pub use sink::event_sink::StreamEventSink;
pub use sink::stream_writer::StreamWriter;
pub use types::message::{MessageChunk, MessageChunkKind, StreamSink};
pub use types::metadata::{CheckpointEvent, StreamMetadata};
pub use types::stream_event::StreamEvent;
pub use types::stream_mode::StreamMode;
pub use wire::convert::{
    stream_event_to_format_a, stream_event_to_protocol_envelope, ProtocolEventEnvelope,
};
pub use wire::envelope::{to_json, Envelope, EnvelopeState};
pub use wire::protocol::ProtocolEvent;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::protocol::ProtocolEvent;

    #[test]
    fn re_exports_compile() {
        let _ = std::mem::size_of::<Envelope>();
        let _ = std::mem::size_of::<EnvelopeState>();
        let _ = std::mem::size_of::<ProtocolEvent>();
        let _ = std::mem::size_of::<StreamMode>();
        let _ = std::mem::size_of::<CheckpointEvent<String>>();
        let _ = std::mem::size_of::<MessageChunk>();
        let _ = std::mem::size_of::<MessageChunkKind>();
        let _ = std::mem::size_of::<StreamWriter<String>>();
    }

    #[test]
    fn stream_mode_variants_unique() {
        assert_ne!(StreamMode::Values, StreamMode::Updates);
        assert_ne!(StreamMode::Messages, StreamMode::Custom);
        assert_ne!(StreamMode::Checkpoints, StreamMode::Tasks);
        assert_ne!(StreamMode::Tools, StreamMode::Debug);
    }

    #[test]
    fn stream_mode_clone_eq() {
        let modes = [
            StreamMode::Values,
            StreamMode::Updates,
            StreamMode::Messages,
            StreamMode::Custom,
            StreamMode::Checkpoints,
            StreamMode::Tasks,
            StreamMode::Tools,
            StreamMode::Debug,
        ];
        for mode in modes {
            assert_eq!(mode, mode.clone());
        }
    }

    #[test]
    fn stream_mode_serialization() {
        let modes = [
            StreamMode::Values,
            StreamMode::Updates,
            StreamMode::Messages,
            StreamMode::Custom,
            StreamMode::Checkpoints,
            StreamMode::Tasks,
            StreamMode::Tools,
            StreamMode::Debug,
        ];
        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let back: StreamMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
    }

    #[test]
    fn stream_mode_debug_format() {
        let modes = [StreamMode::Values, StreamMode::Debug];
        for mode in modes {
            let s = format!("{:?}", mode);
            assert!(!s.is_empty());
        }
    }
}
