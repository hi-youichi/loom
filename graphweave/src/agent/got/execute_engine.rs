//! ExecuteGraph node: run task nodes in DAG order; each sub-task uses ReAct.
//!
//! Computes ready nodes, runs one (or more) per step, writes node_states.
//! Emits GotNodeStart, GotNodeComplete, GotNodeFailed.

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::{Next, RunContext};
use crate::message::Message;
use crate::agent::react::{ActNode, HandleToolErrors, ObserveNode, ThinkNode};
use crate::state::ReActState;
use crate::stream::{StreamEvent, StreamMode};
use crate::tool_source::ToolSource;
use crate::LlmClient;
use crate::Node;

use super::adaptive::{
    complexity_score, complexity_score_via_llm, expand_node_via_llm, maybe_expand, ExpandContext,
};
use super::dag::{predecessors, ready_nodes};
use super::state::{GotState, TaskNodeState, TaskStatus};

/// Max ReAct turns per sub-task to avoid runaway loops.
const MAX_SUB_TASK_TURNS: u32 = 10;

/// Max characters per predecessor result when building sub-task user message (avoids huge context).
const MAX_PREDECESSOR_RESULT_LEN: usize = 500;

/// System prompt for sub-task execution (one node in the DAG).
const SUB_TASK_SYSTEM: &str =
    "You are an assistant. Complete the following sub-task. Use tools if needed. Be concise.";

/// Truncates `s` to at most `max_len` characters, at a UTF-8 boundary, with "..." if truncated.
fn truncate_result(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let end = (0..=max_len.saturating_sub(3))
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0);
    format!("{}...", &s[..end])
}

/// Builds the full user message for a sub-task: task goal, predecessor results (truncated), and this node's description.
///
/// Successor nodes receive predecessor outputs so they can aggregate or build on them.
/// Called by ExecuteGraphNode before [`ExecuteGraphNode::run_sub_task`].
pub(crate) fn build_sub_task_user_message(
    state: &GotState,
    node_id: &str,
    description: &str,
) -> String {
    let pred_ids = predecessors(&state.task_graph, node_id);
    let mut parts = Vec::new();

    if !state.input_message.is_empty() {
        parts.push(format!("Task: {}", state.input_message));
    }

    if !pred_ids.is_empty() {
        let mut pred_results = Vec::new();
        for pred_id in &pred_ids {
            if let Some(ts) = state.node_states.get(pred_id) {
                if ts.status == TaskStatus::Done {
                    if let Some(ref r) = ts.result {
                        let truncated = truncate_result(r, MAX_PREDECESSOR_RESULT_LEN);
                        pred_results.push(format!("{}: {}", pred_id, truncated));
                    }
                }
            }
        }
        if !pred_results.is_empty() {
            parts.push(format!(
                "Predecessor results:\n{}",
                pred_results.join("\n\n")
            ));
        }
    }

    parts.push(format!("Sub-task: {}", description));
    parts.join("\n\n")
}

/// Wraps Arc<dyn LlmClient> for use as Box<dyn LlmClient> in ThinkNode.
struct SharedLlm(Arc<dyn LlmClient>);

#[async_trait::async_trait]
impl LlmClient for SharedLlm {
    async fn invoke(
        &self,
        messages: &[crate::message::Message],
    ) -> Result<crate::llm::LlmResponse, AgentError> {
        self.0.invoke(messages).await
    }
    async fn invoke_stream(
        &self,
        messages: &[crate::message::Message],
        tx: Option<tokio::sync::mpsc::Sender<crate::stream::MessageChunk>>,
    ) -> Result<crate::llm::LlmResponse, AgentError> {
        self.0.invoke_stream(messages, tx).await
    }
}

/// ExecuteGraph node: runs ready DAG nodes one at a time; each node runs as a ReAct sub-task.
///
/// Holds LLM and ToolSource to run Think → Act → Observe for each task node.
/// Emits GotNodeStart / GotNodeComplete / GotNodeFailed when Custom mode is enabled.
/// When `adaptive` is true (AGoT), may expand complex nodes into subgraphs after completion.
/// When `agot_llm_complexity` is true, complexity is decided by LLM instead of heuristic.
pub struct ExecuteGraphNode {
    think: ThinkNode,
    act: ActNode,
    observe: ObserveNode,
    adaptive: bool,
    agot_llm_complexity: bool,
    llm: Arc<dyn LlmClient>,
}

impl ExecuteGraphNode {
    /// Creates an ExecuteGraph node with the given LLM and tool source.
    ///
    /// When `adaptive` is true, complex nodes may be expanded into subgraphs (AGoT)
    /// via LLM call after completion. When `agot_llm_complexity` is true, use LLM to
    /// classify simple vs complex instead of the heuristic.
    pub fn new(
        llm: Arc<dyn LlmClient>,
        tool_source: Box<dyn ToolSource>,
        adaptive: bool,
        agot_llm_complexity: bool,
    ) -> Self {
        let think = ThinkNode::new(Arc::new(SharedLlm(Arc::clone(&llm))));
        let act = ActNode::new(tool_source).with_handle_tool_errors(HandleToolErrors::Always(None));
        let observe = ObserveNode::with_loop();
        Self {
            think,
            act,
            observe,
            adaptive,
            agot_llm_complexity,
            llm,
        }
    }

    /// Runs one sub-task (ReAct loop until no tool_calls or max turns).
    ///
    /// `user_message` is the full user content for the sub-task (task goal, predecessor
    /// results, and this node's description). Built by [`build_sub_task_user_message`].
    async fn run_sub_task(&self, user_message: &str) -> Result<String, AgentError> {
        let mut state = ReActState {
            messages: vec![
                Message::system(SUB_TASK_SYSTEM),
                Message::user(user_message.to_string()),
            ],
            tool_calls: vec![],
            tool_results: vec![],
            turn_count: 0,
            approval_result: None,
            usage: None,
            total_usage: None,
            message_count_after_last_think: None,
        };

        for _ in 0..MAX_SUB_TASK_TURNS {
            let (s1, _) = self.think.run(state).await?;
            if s1.tool_calls.is_empty() {
                return Ok(s1.last_assistant_reply().unwrap_or_default());
            }
            let (s2, _) = self.act.run(s1).await?;
            let (s3, _) = self.observe.run(s2).await?;
            state = s3;
        }

        Ok(state.last_assistant_reply().unwrap_or_default())
    }
}

#[async_trait]
impl Node<GotState> for ExecuteGraphNode {
    fn id(&self) -> &str {
        "execute_graph"
    }

    async fn run(&self, state: GotState) -> Result<(GotState, Next), AgentError> {
        let ctx = RunContext::new(crate::memory::RunnableConfig::default());
        self.run_with_context(state, &ctx).await
    }

    async fn run_with_context(
        &self,
        state: GotState,
        ctx: &RunContext<GotState>,
    ) -> Result<(GotState, Next), AgentError> {
        let ready = ready_nodes(&state.task_graph, &state.node_states);
        if ready.is_empty() {
            return Ok((state, Next::End));
        }

        let node_id = ready.into_iter().next().unwrap();
        let description = state
            .task_graph
            .nodes
            .iter()
            .find(|n| n.id == node_id)
            .map(|n| n.description.clone())
            .ok_or_else(|| AgentError::ExecutionFailed("node not found".to_string()))?;

        let user_message = build_sub_task_user_message(&state, &node_id, &description);

        if ctx.stream_mode.contains(&StreamMode::Custom) {
            if let Some(tx) = &ctx.stream_tx {
                let _ = tx
                    .send(StreamEvent::GotNodeStart {
                        node_id: node_id.clone(),
                    })
                    .await;
            }
        }

        let mut node_states = state.node_states;
        node_states.insert(
            node_id.clone(),
            TaskNodeState {
                status: TaskStatus::Running,
                result: None,
                error: None,
            },
        );
        let mut state = GotState {
            input_message: state.input_message,
            task_graph: state.task_graph,
            node_states,
        };

        match self.run_sub_task(&user_message).await {
            Ok(result) => {
                let summary = if result.len() > 200 {
                    let end = (0..=200)
                        .rev()
                        .find(|&i| result.is_char_boundary(i))
                        .unwrap_or(0);
                    format!("{}...", &result[..end])
                } else {
                    result.clone()
                };
                if ctx.stream_mode.contains(&StreamMode::Custom) {
                    if let Some(tx) = &ctx.stream_tx {
                        let _ = tx
                            .send(StreamEvent::GotNodeComplete {
                                node_id: node_id.clone(),
                                result_summary: summary.clone(),
                            })
                            .await;
                    }
                }
                state.node_states.insert(
                    node_id.clone(),
                    TaskNodeState {
                        status: TaskStatus::Done,
                        result: Some(result.clone()),
                        error: None,
                    },
                );

                if self.adaptive {
                    let node = state
                        .task_graph
                        .nodes
                        .iter()
                        .find(|n| n.id == node_id)
                        .cloned();
                    let expand_ctx = ExpandContext {
                        node_id: &node_id,
                        result: &result,
                        node_states: &state.node_states,
                        input_message: &state.input_message,
                    };
                    let complexity_override = if let Some(ref n) = node {
                        if self.agot_llm_complexity {
                            complexity_score_via_llm(Arc::clone(&self.llm), n, &expand_ctx)
                                .await
                                .ok()
                        } else {
                            Some(complexity_score(n, &expand_ctx))
                        }
                    } else {
                        None
                    };
                    let subgraph = if let Some(ref n) = node {
                        expand_node_via_llm(Arc::clone(&self.llm), &expand_ctx, n)
                            .await
                            .ok()
                            .flatten()
                    } else {
                        None
                    };
                    if let Ok(Some(expand_res)) = maybe_expand(
                        &mut state,
                        &node_id,
                        &result,
                        true,
                        complexity_override,
                        |_| subgraph,
                    )
                    {
                        if ctx.stream_mode.contains(&StreamMode::Custom) {
                            if let Some(tx) = &ctx.stream_tx {
                                let _ = tx
                                    .send(StreamEvent::GotExpand {
                                        node_id: node_id.clone(),
                                        nodes_added: expand_res.nodes_added,
                                        edges_added: expand_res.edges_added,
                                    })
                                    .await;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                let err_msg = e.to_string();
                if ctx.stream_mode.contains(&StreamMode::Custom) {
                    if let Some(tx) = &ctx.stream_tx {
                        let _ = tx
                            .send(StreamEvent::GotNodeFailed {
                                node_id: node_id.clone(),
                                error: err_msg.clone(),
                            })
                            .await;
                    }
                }
                state.node_states.insert(
                    node_id,
                    TaskNodeState {
                        status: TaskStatus::Failed,
                        result: None,
                        error: Some(err_msg),
                    },
                );
                // Plan: on first failure we stop (return Next::End).
                return Ok((state, Next::End));
            }
        }

        let still_ready = ready_nodes(&state.task_graph, &state.node_states);
        if still_ready.is_empty() {
            Ok((state, Next::End))
        } else {
            Ok((state, Next::Node("execute_graph".into())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::got::state::{TaskGraph, TaskNode};

    fn node(id: &str, desc: &str) -> TaskNode {
        TaskNode {
            id: id.to_string(),
            description: desc.to_string(),
            tool_calls: vec![],
        }
    }

    /// **Scenario**: Sub-task user message for a node with one predecessor includes that predecessor's result.
    #[test]
    fn build_sub_task_user_message_includes_predecessor_result() {
        let state = GotState {
            input_message: "Overall task".to_string(),
            task_graph: TaskGraph {
                nodes: vec![node("a", "Step A"), node("b", "Step B")],
                edges: vec![("a".into(), "b".into())],
            },
            node_states: [
                (
                    "a".to_string(),
                    TaskNodeState {
                        status: TaskStatus::Done,
                        result: Some("result from A".to_string()),
                        error: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        };
        let msg = build_sub_task_user_message(&state, "b", "Step B");
        assert!(msg.contains("Predecessor results:"), "message should have predecessor section");
        assert!(msg.contains("result from A"), "message should include predecessor result");
        assert!(msg.contains("Sub-task: Step B"));
        assert!(msg.contains("Overall task"));
    }

    /// **Scenario**: Sub-task user message for a node with no predecessors has no predecessor section.
    #[test]
    fn build_sub_task_user_message_no_predecessors() {
        let state = GotState {
            input_message: "Task".to_string(),
            task_graph: TaskGraph {
                nodes: vec![node("a", "Step A")],
                edges: vec![],
            },
            node_states: std::collections::HashMap::new(),
        };
        let msg = build_sub_task_user_message(&state, "a", "Step A");
        assert!(!msg.contains("Predecessor results:"), "no predecessor section when no preds");
        assert!(msg.contains("Sub-task: Step A"));
    }
}
