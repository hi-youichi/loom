//! DUP state: UnderstandOutput and DupState.
//!
//! DupState composes ReActState with an optional understanding output.

use serde::{Deserialize, Serialize};

use crate::state::ReActState;

/// Structured output from the Understand node (DUP phase 1–2).
///
/// Extracted from the LLM response: core goal, constraints, and context.
/// Used by downstream plan/act/observe nodes and for display in the CLI.
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

    /// Returns the most recent reasoning/thinking content from the core ReAct state.
    pub fn last_reasoning_content(&self) -> Option<String> {
        self.core.last_reasoning_content()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ReActState;
    use crate::Message;

    #[test]
    fn last_assistant_reply_delegates_to_core() {
        let mut state = DupState {
            core: ReActState::default(),
            understood: None,
        };
        assert!(state.last_assistant_reply().is_none());
        state
            .core
            .messages
            .push(Message::Assistant(crate::message::AssistantPayload {
                content: "hello".to_string(),
                tool_calls: vec![],
                reasoning_content: None,
            }));
        assert_eq!(state.last_assistant_reply().as_deref(), Some("hello"));
    }

    #[test]
    fn last_reasoning_content_delegates_to_core() {
        let state = DupState {
            core: ReActState {
                last_reasoning_content: Some("thinking".to_string()),
                ..Default::default()
            },
            understood: None,
        };
        assert_eq!(state.last_reasoning_content().as_deref(), Some("thinking"));
    }

    #[test]
    fn serialization_round_trip() {
        let state = DupState {
            core: ReActState::default(),
            understood: Some(UnderstandOutput {
                core_goal: "goal".to_string(),
                key_constraints: vec!["c1".to_string()],
                relevant_context: "ctx".to_string(),
            }),
        };
        let json = serde_json::to_string(&state).unwrap();
        let restored: DupState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.understood.as_ref().unwrap().core_goal, "goal");
    }
}
