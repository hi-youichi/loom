//! Typed multi-pattern stream event for dispatching agent events.
//!
//! Wraps `StreamEvent<S>` for different agent patterns (ReAct, DUP, ToT, GoT)
//! into a single enum that display and bridge layers can pattern-match on.

use crate::StreamEvent;
use crate::{StubDupState, StubGotState, StubTotState};
use crate::state::ReActState;

/// Typed multi-pattern stream event.
///
/// Unlike `crate::AnyStreamEvent` (which uses `serde_json::Value`),
/// this carries the actual `StreamEvent<S>` for each agent pattern.
#[derive(Debug, Clone)]
pub enum TypedAnyStreamEvent {
    React(StreamEvent<ReActState>),
    Dup(StreamEvent<StubDupState>),
    Tot(StreamEvent<StubTotState>),
    Got(StreamEvent<StubGotState>),
}
