//! Backtrack node: try the next candidate at the same depth when suggest_backtrack is set.
//!
//! Picks the next index not in tried_indices, applies that candidate's thought and tool_calls
//! to core (undoing the last assistant + tool-result messages), clears suggest_backtrack,
//! and returns Next::Node("act"). Emits StreamEvent::TotBacktrack.

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::{Next, RunContext};
use crate::message::Message;
use crate::stream::StreamEvent;
use crate::Node;

use super::state::TotState;

/// Backtrack node: same-layer next candidate; applies it to core and routes to act.
///
/// Reads `state.tot.candidates`, `tried_indices`, `chosen_index`; picks next index
/// not in tried_indices; undoes last assistant + tool-result messages; applies new
/// candidate; clears suggest_backtrack; returns Next::Node("act"). Emits TotBacktrack.
pub struct BacktrackNode;

impl BacktrackNode {
    /// Creates a Backtrack node.
    pub fn new() -> Self {
        Self
    }

    /// Pops the last assistant message and all consecutive user messages after it (tool results).
    fn pop_last_round_messages(messages: &mut Vec<Message>) {
        while matches!(messages.last(), Some(Message::User(_))) {
            messages.pop();
        }
        if matches!(messages.last(), Some(Message::Assistant(_))) {
            messages.pop();
        }
    }
}

impl Default for BacktrackNode {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Node<TotState> for BacktrackNode {
    fn id(&self) -> &str {
        "backtrack"
    }

    async fn run(&self, state: TotState) -> Result<(TotState, Next), AgentError> {
        let mut tot = state.tot;
        let candidates_len = tot.candidates.len();
        let next_index = (0..candidates_len)
            .find(|i| !tot.tried_indices.contains(i))
            .expect("backtrack only when there is a next candidate");

        tot.tried_indices.push(next_index);
        tot.chosen_index = Some(next_index);
        tot.suggest_backtrack = false;
        let _ = tot.path_failed_reason.take();

        let chosen = tot.candidates.get(next_index).unwrap();
        let mut core = state.core;
        Self::pop_last_round_messages(&mut core.messages);
        core.messages
            .push(Message::Assistant(chosen.thought.clone()));
        core.tool_calls = chosen.tool_calls.clone();
        core.tool_results = vec![];

        let out = TotState { core, tot };
        Ok((out, Next::Node("act".into())))
    }

    async fn run_with_context(
        &self,
        state: TotState,
        ctx: &RunContext<TotState>,
    ) -> Result<(TotState, Next), AgentError> {
        let reason = state
            .tot
            .path_failed_reason
            .clone()
            .unwrap_or_else(|| "path failed".into());
        let (out, next) = self.run(state).await?;
        if let Some(ref tx) = ctx.stream_tx {
            let to_depth = out.tot.depth;
            let _ = tx
                .send(StreamEvent::TotBacktrack { reason, to_depth })
                .await;
        }
        Ok((out, next))
    }
}
