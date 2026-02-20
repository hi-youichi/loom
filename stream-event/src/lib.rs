//! Stream event protocol (protocol_spec): type + payload + envelope.
//!
//! This crate defines the wire shape of a single stream event and envelope injection.
//! It does not depend on loom. Loom bridges `StreamEvent<S>` into `ProtocolEvent` and calls `to_json`.

pub mod envelope;
pub mod event;

pub use envelope::{to_json, Envelope, EnvelopeState};
pub use event::ProtocolEvent;
