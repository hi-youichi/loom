//! PlanGraph node: LLM produces a DAG of sub-tasks from the user message.
//!
//! Reads `state.input_message`, calls LLM with GOT prompt, parses JSON into
//! `state.task_graph`, and emits `StreamEvent::GotPlan`.

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::{Next, RunContext};
use crate::llm::LlmClient;
use crate::message::Message;
use crate::stream::{StreamEvent, StreamMode};
use crate::Node;

use super::prompt::GOT_PLAN_SYSTEM;
use super::state::{GotState, TaskGraph, TaskNode};

/// PlanGraph node: turns user message into a task DAG via LLM.
///
/// Implements `Node<GotState>`. Writes `state.task_graph` and initializes
/// `node_states` for each node to Pending. Emits GotPlan when Custom mode is enabled.
pub struct PlanGraphNode {
    llm: Box<dyn LlmClient>,
}

impl PlanGraphNode {
    pub fn new(llm: Box<dyn LlmClient>) -> Self {
        Self { llm }
    }
}

/// Parses LLM response into TaskGraph. Fallback: single node with full message.
fn parse_task_graph(raw: &str, input_message: &str) -> TaskGraph {
    #[derive(serde::Deserialize)]
    struct RawNode {
        id: Option<String>,
        description: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct RawGraph {
        nodes: Option<Vec<RawNode>>,
        edges: Option<Vec<(String, String)>>,
    }

    if let Ok(parsed) = serde_json::from_str::<RawGraph>(raw) {
        if let Some(nodes) = parsed.nodes {
            let graph_nodes: Vec<TaskNode> = nodes
                .into_iter()
                .filter_map(|n| {
                    let id =
                        n.id.filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "task_1".to_string());
                    let description = n.description.unwrap_or_else(|| input_message.to_string());
                    Some(TaskNode {
                        id,
                        description,
                        tool_calls: vec![],
                    })
                })
                .collect();
            if !graph_nodes.is_empty() {
                let ids: std::collections::HashSet<String> =
                    graph_nodes.iter().map(|n| n.id.clone()).collect();
                let edges = parsed
                    .edges
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|(f, t)| ids.contains(f) && ids.contains(t))
                    .collect();
                return TaskGraph {
                    nodes: graph_nodes,
                    edges,
                };
            }
        }
    }

    // Fallback: single node
    TaskGraph {
        nodes: vec![TaskNode {
            id: "task_1".to_string(),
            description: input_message.to_string(),
            tool_calls: vec![],
        }],
        edges: vec![],
    }
}

#[async_trait]
impl Node<GotState> for PlanGraphNode {
    fn id(&self) -> &str {
        "plan_graph"
    }

    async fn run(&self, state: GotState) -> Result<(GotState, Next), AgentError> {
        let ctx = crate::graph::RunContext::new(crate::memory::RunnableConfig::default());
        self.run_with_context(state, &ctx).await
    }

    async fn run_with_context(
        &self,
        state: GotState,
        ctx: &RunContext<GotState>,
    ) -> Result<(GotState, Next), AgentError> {
        let messages = vec![
            Message::system(GOT_PLAN_SYSTEM),
            Message::user(state.input_message.clone()),
        ];
        let response = self.llm.invoke(&messages).await?;
        let task_graph = parse_task_graph(response.content.trim(), &state.input_message);

        let node_ids: Vec<String> = task_graph.nodes.iter().map(|n| n.id.clone()).collect();
        let node_count = task_graph.nodes.len();
        let edge_count = task_graph.edges.len();

        if ctx.stream_mode.contains(&StreamMode::Custom) {
            if let Some(tx) = &ctx.stream_tx {
                let _ = tx
                    .send(StreamEvent::GotPlan {
                        node_count,
                        edge_count,
                        node_ids: node_ids.clone(),
                    })
                    .await;
            }
        }

        let node_states = task_graph
            .nodes
            .iter()
            .map(|n| (n.id.clone(), super::state::TaskNodeState::default()))
            .collect();

        let new_state = GotState {
            input_message: state.input_message,
            task_graph,
            node_states,
        };

        Ok((new_state, Next::Continue))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockLlm;
    use crate::memory::RunnableConfig;
    use tokio::sync::mpsc;

    #[test]
    fn parse_task_graph_filters_invalid_edges() {
        let raw = r#"{
            "nodes":[
                {"id":"a","description":"step a"},
                {"id":"b","description":"step b"}
            ],
            "edges":[["a","b"],["a","missing"],["missing","b"]]
        }"#;
        let graph = parse_task_graph(raw, "fallback");
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges, vec![("a".to_string(), "b".to_string())]);
    }

    #[test]
    fn parse_task_graph_fallbacks_to_single_node() {
        let graph = parse_task_graph("not json", "hello world");
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].id, "task_1");
        assert_eq!(graph.nodes[0].description, "hello world");
        assert!(graph.edges.is_empty());
    }

    #[tokio::test]
    async fn run_with_context_sets_task_graph_and_emits_event() {
        let llm = MockLlm::with_no_tool_calls(
            r#"{
                "nodes":[
                    {"id":"collect","description":"collect info"},
                    {"id":"answer","description":"answer user"}
                ],
                "edges":[["collect","answer"]]
            }"#,
        );
        let node = PlanGraphNode::new(Box::new(llm));
        let state = GotState {
            input_message: "how to learn rust".to_string(),
            ..GotState::default()
        };

        let (tx, mut rx) = mpsc::channel(8);
        let mut ctx = RunContext::<GotState>::new(RunnableConfig::default());
        ctx.stream_mode.insert(StreamMode::Custom);
        ctx.stream_tx = Some(tx);

        let (out, next) = node.run_with_context(state, &ctx).await.unwrap();
        assert!(matches!(next, Next::Continue));
        assert_eq!(out.task_graph.nodes.len(), 2);
        assert_eq!(out.task_graph.edges.len(), 1);
        assert_eq!(out.node_states.len(), 2);
        assert!(out.node_states.contains_key("collect"));
        assert!(out.node_states.contains_key("answer"));

        match rx.recv().await {
            Some(StreamEvent::GotPlan {
                node_count,
                edge_count,
                node_ids,
            }) => {
                assert_eq!(node_count, 2);
                assert_eq!(edge_count, 1);
                assert_eq!(node_ids, vec!["collect".to_string(), "answer".to_string()]);
            }
            other => panic!("expected GotPlan event, got {:?}", other),
        }
    }
}
