//! ToT (Tree of Thoughts) state: TotCandidate, TotExtension, TotState.
//!
//! Composes `ReActState` with ToT-specific fields for multi-candidate expansion
//! and selection.

use serde::{Deserialize, Serialize};

use crate::state::{ReActState, ToolCall};

/// One candidate produced by ThinkExpand: a thought and optional tool calls.
///
/// Written by [`ThinkExpandNode`](crate::agent::tot::ThinkExpandNode); scored by
/// [`ThinkEvaluateNode`](crate::agent::tot::ThinkEvaluateNode). The chosen candidate's
/// `tool_calls` are applied to `TotState::core` before Act.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotCandidate {
    /// Reasoning text for this candidate.
    pub thought: String,
    /// Tool invocations for this candidate (same shape as `ReActState::tool_calls`).
    pub tool_calls: Vec<ToolCall>,
    /// Score assigned by ThinkEvaluate (None until evaluate runs).
    #[serde(default)]
    pub score: Option<f32>,
}

/// ToT-specific state: depth, current candidates, and selection tracking.
///
/// Used by ThinkExpand (writes `candidates`), ThinkEvaluate (writes `scores`,
/// `chosen_index`), and select condition / Backtrack (read `chosen_index`,
/// `tried_indices`). Interacts with [`TotState`] and nodes in `crate::agent::tot`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TotExtension {
    /// Current depth in the thought tree (incremented each expand round).
    #[serde(default)]
    pub depth: u32,
    /// Candidates produced by the last ThinkExpand (2–3 items).
    #[serde(default)]
    pub candidates: Vec<TotCandidate>,
    /// Path summaries for logging (e.g. list of thought snippets per depth).
    #[serde(default)]
    pub explored_paths: Vec<Vec<String>>,
    /// Index of the chosen candidate at current depth (set by ThinkEvaluate).
    #[serde(default)]
    pub chosen_index: Option<usize>,
    /// Indices already tried at current depth when backtracking.
    #[serde(default)]
    pub tried_indices: Vec<usize>,
    /// When true, Observe suggests trying the next candidate (same layer).
    #[serde(default)]
    pub suggest_backtrack: bool,
    /// Optional reason for path failure (e.g. tool error, empty result).
    #[serde(default)]
    pub path_failed_reason: Option<String>,
}

/// State for the ToT graph: core ReAct state plus ToT extension.
///
/// Composes `ReActState` (as `core`) with `TotExtension` (as `tot`). ThinkExpand
/// and ThinkEvaluate write `tot`; adapter nodes read/write `core` (and preserve `tot`).
/// Checkpointer serializes the full `TotState`.
///
/// **Interaction**: Flows through `StateGraph<TotState>`; see `crate::agent::tot::runner`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotState {
    /// Core execution state (messages, tool_calls, tool_results). Reused from ReAct.
    pub core: ReActState,
    /// ToT extension: candidates, scores, chosen index, backtrack tracking.
    #[serde(default)]
    pub tot: TotExtension,
}

impl TotState {
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
        let mut state = TotState {
            core: ReActState::default(),
            tot: TotExtension::default(),
        };
        assert!(state.last_assistant_reply().is_none());
        state.core.messages.push(Message::Assistant(
            crate::message::AssistantPayload {
                content: "reply".to_string(),
                tool_calls: vec![],
                reasoning_content: None,
            },
        ));
        assert_eq!(state.last_assistant_reply().as_deref(), Some("reply"));
    }

    #[test]
    fn last_reasoning_content_delegates_to_core() {
        let state = TotState {
            core: ReActState {
                last_reasoning_content: Some("reasoning".to_string()),
                ..Default::default()
            },
            tot: TotExtension::default(),
        };
        assert_eq!(state.last_reasoning_content().as_deref(), Some("reasoning"));
    }

    #[test]
    fn tot_extension_default() {
        let ext = TotExtension::default();
        assert_eq!(ext.depth, 0);
        assert!(ext.candidates.is_empty());
        assert!(ext.chosen_index.is_none());
        assert!(!ext.suggest_backtrack);
        assert!(ext.path_failed_reason.is_none());
    }

    #[test]
    fn serialization_round_trip() {
        let state = TotState {
            core: ReActState::default(),
            tot: TotExtension {
                depth: 3,
                candidates: vec![TotCandidate {
                    thought: "idea".to_string(),
                    tool_calls: vec![],
                    score: Some(0.9),
                }],
                ..Default::default()
            },
        };
        let json = serde_json::to_string(&state).unwrap();
        let restored: TotState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.tot.depth, 3);
        assert_eq!(restored.tot.candidates.len(), 1);
    }
}
