//! Observe node: read tool_results, merge into state (e.g. messages), clear tool_calls and tool_results.

use async_trait::async_trait;
use tracing::info;

use crate::error::AgentError;
use crate::graph::Next;
use crate::message::Message;
use crate::state::ReActState;
use crate::Node;

pub struct ObserveNode {
    enable_loop: bool,
    /// When `Some(n)`, end loop after n observe rounds. When `None` (default for with_loop), no limit.
    max_turns: Option<u32>,
}

impl ObserveNode {
    pub fn new() -> Self {
        Self {
            enable_loop: false,
            max_turns: None,
        }
    }

    /// ReAct loop: observe can continue back to think. No turn limit by default.
    pub fn with_loop() -> Self {
        Self {
            enable_loop: true,
            max_turns: None,
        }
    }

    /// ReAct loop with a maximum number of observe rounds; after this, exit with max_turns_reached.
    pub fn with_loop_max_turns(max_turns: u32) -> Self {
        Self {
            enable_loop: true,
            max_turns: Some(max_turns),
        }
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
            let label = if tr.is_error { "error" } else { "result" };

            // Observe only consumes the normalized observation view.
            let observation = tr.observation();

            let mut msg = format!("Tool {} {}:\n{}", name, label, observation);

            // Add storage reference hint if available
            if let Some(ref storage_ref) = tr.storage_ref {
                msg.push_str(&format!(
                    "\n\nFull output saved to: {}",
                    storage_ref.path.display()
                ));
            }

            messages.push(Message::User(msg));
        }
        let next_turn = state.turn_count.saturating_add(1);
        let new_state = ReActState {
            messages,
            last_reasoning_content: state.last_reasoning_content,
            tool_calls: vec![],
            tool_results: vec![],
            turn_count: next_turn,
            approval_result: state.approval_result,
            usage: state.usage,
            total_usage: state.total_usage,
            message_count_after_last_think: state.message_count_after_last_think,
        };
        let max_turns_reached = self.max_turns.map_or(false, |m| next_turn >= m);
        let (next, exit_reason) = if self.enable_loop && max_turns_reached {
            (Next::End, "max_turns_reached")
        } else if self.enable_loop && had_tool_calls {
            (Next::Continue, "loop_back_to_think")
        } else if self.enable_loop && !had_tool_calls {
            (Next::End, "no_tool_calls_final_answer")
        } else {
            (Next::Continue, "linear_next")
        };
        info!(
            observe_exit = exit_reason,
            next = ?next,
            turn = next_turn,
            had_tool_calls = had_tool_calls,
            "observe exit"
        );
        Ok((new_state, next))
    }
}
