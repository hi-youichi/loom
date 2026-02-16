//! ToT adapter nodes: Act and Observe that operate on TotState.
//!
//! Observe sets `suggest_backtrack` when tool results look bad and another candidate can be tried.

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::Next;
use crate::react::{ActNode, HandleToolErrors, ObserveNode};
use crate::Node;
use crate::{helve::ApprovalPolicy, tool_source::ToolSource};

use super::state::TotState;

/// Min total tool result content length below which we consider the path weak (suggest backtrack).
const MIN_TOOL_RESULT_CONTENT_LEN: usize = 20;

/// ToT Act node: adapts ActNode for TotState. Uses core (chosen candidate already applied).
pub struct TotActNode {
    act: ActNode,
}

impl TotActNode {
    pub fn new(tool_source: Box<dyn ToolSource>) -> Self {
        Self {
            act: ActNode::new(tool_source).with_handle_tool_errors(HandleToolErrors::Always(None)),
        }
    }

    pub fn with_approval_policy(mut self, policy: Option<ApprovalPolicy>) -> Self {
        self.act = self.act.with_approval_policy(policy);
        self
    }
}

#[async_trait]
impl Node<TotState> for TotActNode {
    fn id(&self) -> &str {
        "act"
    }

    async fn run(&self, state: TotState) -> Result<(TotState, Next), AgentError> {
        let (core_out, next) = self.act.run(state.core).await?;
        Ok((
            TotState {
                core: core_out,
                tot: state.tot,
            },
            next,
        ))
    }
}

/// ToT Observe node: adapts ObserveNode for TotState; loops back to think_expand.
pub struct TotObserveNode {
    observe: ObserveNode,
}

impl TotObserveNode {
    pub fn new() -> Self {
        Self {
            observe: ObserveNode::with_loop(),
        }
    }
}

impl Default for TotObserveNode {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Node<TotState> for TotObserveNode {
    fn id(&self) -> &str {
        "observe"
    }

    async fn run(&self, state: TotState) -> Result<(TotState, Next), AgentError> {
        let mut tot = state.tot;
        let results = &state.core.tool_results;
        let can_try_another =
            tot.candidates.len() > 1 && tot.tried_indices.len() < tot.candidates.len();
        let has_error = results.iter().any(|r| {
            let c = r.content.to_lowercase();
            c.contains("error") || c.contains("failed")
        });
        let total_len: usize = results.iter().map(|r| r.content.len()).sum();
        let too_short = total_len < MIN_TOOL_RESULT_CONTENT_LEN;

        let (core_out, next) = self.observe.run(state.core).await?;

        if can_try_another && (has_error || too_short) {
            tot.suggest_backtrack = true;
            tot.path_failed_reason = Some(if has_error {
                "tool error or failure".into()
            } else {
                "tool results too short".into()
            });
        }
        let mapped_next = match &next {
            Next::Node(id) if id == "think" => Next::Continue,
            other => other.clone(),
        };
        Ok((
            TotState {
                core: core_out,
                tot,
            },
            mapped_next,
        ))
    }
}
