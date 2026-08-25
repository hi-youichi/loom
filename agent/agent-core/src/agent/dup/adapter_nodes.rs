//! Adapter nodes: PlanNode, ActNode, ObserveNode for DupState.
//!
//! Each wraps the corresponding react node, extracting state.core, calling the inner
//! node, and writing the result back.

use std::sync::Arc;

use async_trait::async_trait;

use crate::agent::react::{ActNode, ObserveNode, ThinkNode};
use anureo_graph_core::GraphError;
use anureo_graph_core::Next;
use anureo_graph_core::Node;
use anureo_llm::LlmProvider;
use tool_core::ToolRegistryLocked;

use super::state::DupState;

/// Plan node: adapts ThinkNode for DupState. Extracts core, runs Think, writes back.
pub struct PlanNode {
    think: ThinkNode,
}

impl PlanNode {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            think: ThinkNode::new(provider),
        }
    }
}

#[async_trait]
impl Node<DupState> for PlanNode {
    fn id(&self) -> &str {
        "plan"
    }

    async fn run(&self, state: DupState) -> Result<(DupState, Next), GraphError> {
        let (core_out, next) = self.think.run(state.core).await?;
        Ok((
            DupState {
                core: core_out,
                understood: state.understood,
            },
            next,
        ))
    }
}

/// Act node: adapts ActNode for DupState.
pub struct DupActNode {
    act: ActNode,
}

impl DupActNode {
    pub fn new(tool_source: Arc<ToolRegistryLocked>) -> Self {
        Self {
            act: ActNode::new(tool_source),
        }
    }
}

#[async_trait]
impl Node<DupState> for DupActNode {
    fn id(&self) -> &str {
        "act"
    }

    async fn run(&self, state: DupState) -> Result<(DupState, Next), GraphError> {
        let (core_out, next) = self.act.run(state.core).await?;
        Ok((
            DupState {
                core: core_out,
                understood: state.understood,
            },
            next,
        ))
    }
}

/// Observe node: adapts ObserveNode for DupState. Loops back to plan.
pub struct DupObserveNode {
    observe: ObserveNode,
}

impl DupObserveNode {
    pub fn new() -> Self {
        Self {
            observe: ObserveNode::with_loop(),
        }
    }
}

impl Default for DupObserveNode {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Node<DupState> for DupObserveNode {
    fn id(&self) -> &str {
        "observe"
    }

    async fn run(&self, state: DupState) -> Result<(DupState, Next), GraphError> {
        let (core_out, next) = self.observe.run(state.core).await?;
        // Map Next::Node("think") to Next::Node("plan") for DUP graph
        let mapped_next = match &next {
            Next::Node(id) if id == "think" => Next::Node("plan".into()),
            other => other.clone(),
        };
        Ok((
            DupState {
                core: core_out,
                understood: state.understood,
            },
            mapped_next,
        ))
    }
}
