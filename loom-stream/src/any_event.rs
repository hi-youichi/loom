//! Typed stream event for dispatching agent events.
//!
//! Currently only React is supported. DUP/ToT/GoT events are handled
//! via `agent::run::TypedAnyStreamEvent` which has real state types.

use crate::StreamEvent;
use crate::state::ReActState;

/// Typed stream event (loom-stream layer).
///
/// This is the type used by `ToolCallContext.any_stream_event_sender`.
/// `agent::run::TypedAnyStreamEvent` is a separate, richer type used
/// by the runner layer.
#[derive(Debug, Clone)]
pub enum TypedAnyStreamEvent {
    React(StreamEvent<ReActState>),
}
