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

#[cfg(test)]
mod tests {
    use super::super::state::{TotCandidate, TotExtension};
    use super::*;
    use crate::memory::RunnableConfig;
    use crate::state::{ReActState, ToolCall, ToolResult};
    use tokio::sync::mpsc;

    fn candidate(thought: &str, tool_name: &str) -> TotCandidate {
        TotCandidate {
            thought: thought.to_string(),
            tool_calls: vec![ToolCall {
                name: tool_name.to_string(),
                arguments: "{}".to_string(),
                id: None,
            }],
            score: Some(0.5),
        }
    }

    #[test]
    fn pop_last_round_messages_removes_assistant_and_trailing_users() {
        let mut messages = vec![
            Message::user("u1"),
            Message::Assistant("a1".into()),
            Message::user("tool result 1"),
            Message::user("tool result 2"),
        ];
        BacktrackNode::pop_last_round_messages(&mut messages);
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages.first(), Some(Message::User(s)) if s == "u1"));
    }

    #[tokio::test]
    async fn run_selects_next_candidate_and_resets_core() {
        let node = BacktrackNode::new();
        let state = TotState {
            core: ReActState {
                messages: vec![
                    Message::user("question"),
                    Message::Assistant("old plan".into()),
                    Message::user("old tool result"),
                ],
                tool_calls: vec![ToolCall {
                    name: "old_tool".to_string(),
                    arguments: "{}".to_string(),
                    id: None,
                }],
                tool_results: vec![ToolResult {
                    call_id: None,
                    name: Some("old_tool".to_string()),
                    content: "err".to_string(),
                    is_error: true,
                }],
                ..ReActState::default()
            },
            tot: TotExtension {
                depth: 2,
                candidates: vec![candidate("first", "t1"), candidate("second", "t2")],
                chosen_index: Some(0),
                tried_indices: vec![0],
                suggest_backtrack: true,
                path_failed_reason: Some("first failed".to_string()),
                ..Default::default()
            },
        };

        let (out, next) = node.run(state).await.unwrap();
        assert!(matches!(next, Next::Node(id) if id == "act"));
        assert_eq!(out.tot.chosen_index, Some(1));
        assert_eq!(out.tot.tried_indices, vec![0, 1]);
        assert!(!out.tot.suggest_backtrack);
        assert!(out.tot.path_failed_reason.is_none());
        assert!(out.core.tool_results.is_empty());
        assert_eq!(out.core.tool_calls[0].name, "t2");
        assert!(matches!(
            out.core.messages.last(),
            Some(Message::Assistant(s)) if s == "second"
        ));
    }

    #[tokio::test]
    async fn run_with_context_emits_tot_backtrack_event() {
        let node = BacktrackNode::new();
        let state = TotState {
            core: ReActState {
                messages: vec![Message::user("q"), Message::Assistant("first".into())],
                ..ReActState::default()
            },
            tot: TotExtension {
                depth: 3,
                candidates: vec![candidate("first", "t1"), candidate("second", "t2")],
                chosen_index: Some(0),
                tried_indices: vec![0],
                suggest_backtrack: true,
                path_failed_reason: Some("tool failed".to_string()),
                ..Default::default()
            },
        };

        let (tx, mut rx) = mpsc::channel(8);
        let mut ctx = RunContext::<TotState>::new(RunnableConfig::default());
        ctx.stream_tx = Some(tx);

        let (_out, _next) = node.run_with_context(state, &ctx).await.unwrap();
        match rx.recv().await {
            Some(StreamEvent::TotBacktrack { reason, to_depth }) => {
                assert_eq!(reason, "tool failed");
                assert_eq!(to_depth, 3);
            }
            other => panic!("expected TotBacktrack event, got {:?}", other),
        }
    }
}
