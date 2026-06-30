//! Erased stream event types for agent runs.
//!
//! These types use `serde_json::Value` for pattern-agnostic event dispatching.

use serde::{Deserialize, Serialize};

use crate::state::ReActState;

/// Stream event from agent runs - this is a placeholder type.
/// The actual implementation in loom defines variants for different agent types.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AnyStreamEvent {
    React(serde_json::Value),
    Dup(serde_json::Value),
    Tot(serde_json::Value),
    Got(serde_json::Value),
}

// ── Stub agent state types for multi-pattern event dispatching ──────
// These are simple wrappers that allow AnyStreamEvent to carry state for
// different agent patterns (DUP, ToT, GoT) while keeping the core
// ReAct state accessible via a `.core` field.

/// Stub state for the DUP (Duplicate/Refine) agent pattern.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StubDupState {
    pub core: ReActState,
}

/// Stub state for the ToT (Tree of Thought) agent pattern.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StubTotState {
    pub core: ReActState,
}

/// Stub state for the GoT (Graph of Thought) agent pattern.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StubGotState {
    pub input_message: String,
}
