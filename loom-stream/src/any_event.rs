//! Typed multi-pattern stream event for dispatching agent events.
//!
//! Wraps `StreamEvent<S>` for different agent patterns (ReAct, DUP, ToT, GoT)
//! into a single enum that display and bridge layers can pattern-match on.

use crate::StreamEvent;
use loom_types::cli_run::{StubDupState, StubGotState, StubTotState};
use loom_types::state::ReActState;

/// Typed multi-pattern stream event.
///
/// Unlike `loom_types::cli_run::AnyStreamEvent` (which uses `serde_json::Value`),
/// this carries the actual `StreamEvent<S>` for each agent pattern.
#[derive(Debug, Clone)]
pub enum TypedAnyStreamEvent {
    React(StreamEvent<ReActState>),
    Dup(StreamEvent<StubDupState>),
    Tot(StreamEvent<StubTotState>),
    Got(StreamEvent<StubGotState>),
}
