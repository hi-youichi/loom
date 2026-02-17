//! Observe node: read tool_results, merge into state (e.g. messages), clear tool_calls and tool_results.

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::Next;
use crate::message::Message;
use crate::state::ReActState;
use crate::Node;

/// Maximum number of ReAct loop rounds (observe passes) before forcing End.
pub const MAX_REACT_TURNS: u32 = 10;

pub struct ObserveNode {
    enable_loop: bool,
}

impl ObserveNode {
    pub fn new() -> Self {
        Self { enable_loop: false }
    }

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
            message_count_after_last_think: state.message_count_after_last_think,
        };
        let next = if self.enable_loop && next_turn >= MAX_REACT_TURNS {
            Next::End
        } else if self.enable_loop && had_tool_calls {
            Next::Continue
        } else if self.enable_loop && !had_tool_calls {
            Next::End
        } else {
            Next::Continue
        };
        Ok((new_state, next))
    }
}
