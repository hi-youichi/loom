//! Observe node: read tool_results, merge into state (e.g. messages), clear tool_calls and tool_results.
//!
//! ObserveNode has no external dependencies, implements `Node<ReActState>`; run reads
//! state.tool_results, appends them to state (as User messages so next Think sees context),
//! then clears tool_calls and tool_results. Linear-chain phase does not return next-hop.

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::Next;
use crate::message::Message;
use crate::state::ReActState;
use crate::Node;

/// Observe node: one ReAct step that merges tool results into state and clears tool_*.
///
/// Reads `state.tool_results`, appends each result to messages as a User message
/// (e.g. "Tool get_time returned: 12:00") so the next Think round has context;
/// then clears tool_calls and tool_results. When `enable_loop` is false (linear chain),
/// returns `Next::Continue` so the runner stops after this node if it is last. When
/// `enable_loop` is true, returns `Next::Node("think")` when this round had tool_calls
/// (ReAct loop), else `Next::End`.
///
/// Maximum number of ReAct loop rounds (observe passes) before forcing End.
pub const MAX_REACT_TURNS: u32 = 10;

/// **Interaction**: Implements `Node<ReActState>`; used by StateGraph. No external
/// deps; reads ReActState.tool_results, writes ReActState.messages and clears
/// tool_calls/tool_results.
pub struct ObserveNode {
    /// When true, return Node("think") to loop; when false, return Continue (linear chain).
    enable_loop: bool,
}

impl ObserveNode {
    /// Creates an Observe node for linear chain (one round): returns Next::Continue.
    pub fn new() -> Self {
        Self { enable_loop: false }
    }

    /// Creates an Observe node for multi-round ReAct: returns Node("think") or End.
    pub fn with_loop() -> Self {
        Self { enable_loop: true }
    }
}

impl Default for ObserveNode {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Node<ReActState> for ObserveNode {
    fn id(&self) -> &str {
        "observe"
    }

    /// Merges tool_results into messages (one User message per result), clears tool_*.
    /// Returns Next::Node("think") when this round had tool_calls (ReAct loop), else Next::End.
    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        let had_tool_calls = !state.tool_calls.is_empty();
        let mut messages = state.messages;
        for tr in &state.tool_results {
            let name = tr
                .name
                .as_deref()
                .or(tr.call_id.as_deref())
                .unwrap_or("tool");
            messages.push(Message::User(format!(
                "Tool {} returned: {}",
                name, tr.content
            )));
        }
        let next_turn = state.turn_count.saturating_add(1);
        let new_state = ReActState {
            messages,
            tool_calls: vec![],
            tool_results: vec![],
            turn_count: next_turn,
            approval_result: state.approval_result,
            usage: state.usage,
            total_usage: state.total_usage,
        };
        let next = if self.enable_loop && next_turn >= MAX_REACT_TURNS {
            Next::End
        } else if self.enable_loop && had_tool_calls {
            Next::Node("think".to_string())
        } else if self.enable_loop && !had_tool_calls {
            Next::End
        } else {
            Next::Continue
        };
        Ok((new_state, next))
    }
}
