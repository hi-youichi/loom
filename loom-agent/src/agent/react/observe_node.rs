//! Observe node: read tool_results, merge into state (e.g. messages), clear tool_calls and tool_results.

use async_trait::async_trait;
use tracing::{debug, warn};

use loom_llm::error::AgentError;
use loom_graph::Next;
use loom_memory::uuid6;
use loom_llm::message::{message_summary, Message};
use loom_cli_types::ReActState;
use loom_tools::tool_source::ToolCallContent;
use loom_graph::Node;

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
        let messages_before = state.messages.len();
        debug!(
            had_tool_calls,
            tool_results = state.tool_results.len(),
            messages_before,
            "observe:input"
        );
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

            let mut body = format!("Tool {} {}:\n{}", name, label, observation);

            // Add storage reference hint if available
            if let Some(ref storage_ref) = tr.storage_ref {
                body.push_str(&format!(
                    "\n\nFull output saved to: {}",
                    storage_ref.path.display()
                ));
            }

            let tool_call_id = tr
                .call_id
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    warn!(
                        tool_name = %name,
                        "observe:missing_call_id generating synthetic id"
                    );
                    format!("call_{}", uuid6())
                });

            debug!(
                call_id = %tool_call_id,
                name = %name,
                is_error = tr.is_error,
                content_len = body.len(),
                "observe:convert → Message::Tool"
            );

            messages.push(Message::Tool {
                tool_call_id,
                content: ToolCallContent::text(body),
            });
        }
        let next_turn = state.turn_count.saturating_add(1);
        let new_state = ReActState {
            messages,
            tool_calls: vec![],
            tool_results: vec![],
            turn_count: next_turn,
            ..state
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
        debug!(
            messages_before,
            messages_after = new_state.messages.len(),
            had_tool_calls,
            tool_results_consumed = state.tool_results.len(),
            turn = next_turn,
            exit_reason,
            "observe:exit"
        );
        for (i, msg) in new_state.messages.iter().enumerate().skip(messages_before.saturating_sub(2)) {
            debug!("  {}", message_summary(i, msg));
        }
        Ok((new_state, next))
    }
}

