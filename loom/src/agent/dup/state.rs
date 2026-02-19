//! DUP state: UnderstandOutput and DupState.
//!
//! DupState composes ReActState with an optional understanding output.

use serde::{Deserialize, Serialize};

use crate::state::ReActState;

/// Structured output from the Understand node (DUP phase 1–2).
///
/// Extracted from the LLM response: core goal, constraints, and context.
/// Used by downstream plan/act/observe nodes and for display in TUI/CLI.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnderstandOutput {
    /// One sentence describing what the user wants to achieve.
    pub core_goal: String,
    /// Key constraints (e.g. working folder, approval policy).
    pub key_constraints: Vec<String>,
    /// Brief summary of workspace, files, or context that matters.
    pub relevant_context: String,
}

/// State for the DUP graph: core execution state plus optional understanding.
///
/// Composes `ReActState` (as `core`) with `UnderstandOutput` (as `understood`).
/// The understand node writes `understood`; plan/act/observe operate on `core`.
///
/// **Interaction**: Flows through `StateGraph<DupState>`; understand node sets
/// `understood`; PlanNode/ActNode/ObserveNode read and write `core`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DupState {
    /// Core execution state (messages, tool_calls, tool_results). Reused from ReAct.
    pub core: ReActState,
    /// Understanding output from the understand node. Set after understand runs.
    #[serde(default)]
    pub understood: Option<UnderstandOutput>,
}

impl DupState {
    /// Returns the last assistant reply from `core.messages`, if any.
    pub fn last_assistant_reply(&self) -> Option<String> {
        self.core.last_assistant_reply()
    }
}
