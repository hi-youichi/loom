//! AGoT adaptive extension: complexity scoring and dynamic subgraph expansion.
//!
//! When `adaptive` is enabled, after a node completes we optionally expand it
//! into sub-nodes. Only "complex" nodes are expanded (via heuristic or LLM).

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::AgentError;
use crate::message::Message;
use crate::LlmClient;

use super::dag::{append_subgraph, AppendSubgraphError};
use super::prompt::AGOT_EXPAND_SYSTEM;
use super::state::{GotState, TaskNode, TaskNodeState, TaskStatus};

/// Max total nodes to prevent runaway expansion (AGoT risk mitigation).
const MAX_TOTAL_NODES: usize = 64;

/// Prompt template for LLM-based complexity: answer with exactly "simple" or "complex".
const AGOT_COMPLEXITY_PROMPT: &str = "You are a classifier. Given a task node from a task graph, decide if it is simple (can be done in one step) or complex (should be decomposed into sub-tasks).\nReply with exactly one word: simple or complex.";

/// Complexity level for a task node. Used to decide whether to expand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplexityLevel {
    /// Simple: do not expand; node result is sufficient.
    Simple,
    /// Complex: may expand into subgraph for finer-grained execution.
    Complex,
}

/// Heuristic complexity score based on node description and context.
///
/// Returns Complex when: (1) description is long (>100 chars), or (2) contains
/// complexity keywords ("analyze", "compare", "evaluate", "prove", "derive").
/// Conservative: tends to mark as Simple to avoid over-expansion.
///
/// Uses the LLM to classify the node as simple or complex (aligns with AGoT paper).
///
/// **Interaction**: Called by ExecuteGraphNode when `agot_llm_complexity` is true; result is
/// passed as `complexity_override` to `maybe_expand` so the heuristic is not used.
pub async fn complexity_score_via_llm(
    llm: Arc<dyn LlmClient>,
    node: &TaskNode,
    _ctx: &ExpandContext<'_>,
) -> Result<ComplexityLevel, AgentError> {
    let user = format!(
        "{}\nNode id: {}\nDescription: {}",
        AGOT_COMPLEXITY_PROMPT, node.id, node.description
    );
    let messages = vec![
        Message::system("Reply with exactly one word: simple or complex.".to_string()),
        Message::user(user),
    ];
    let response = llm.invoke(&messages).await?;
    let content = response.content.trim().to_lowercase();
    if content.contains("complex") {
        Ok(ComplexityLevel::Complex)
    } else {
        Ok(ComplexityLevel::Simple)
    }
}

/// **Interaction**: Called by `maybe_expand` before deciding to call LLM expand (when no override).
pub fn complexity_score(node: &TaskNode, _context: &ExpandContext) -> ComplexityLevel {
    let desc = node.description.trim();
    if desc.len() > 100 {
        return ComplexityLevel::Complex;
    }
    let lower = desc.to_lowercase();
    let complex_keywords = [
        "analyze",
        "analyse",
        "compare",
        "evaluate",
        "prove",
        "derive",
        "compute",
        "calculate",
        "determine",
        "investigate",
        "synthesize",
    ];
    if complex_keywords.iter().any(|kw| lower.contains(kw)) {
        return ComplexityLevel::Complex;
    }
    ComplexityLevel::Simple
}

/// Context passed to expand logic: node result, sibling states, task goal.
#[allow(dead_code)] // Used by LLM expand (A4)
pub struct ExpandContext<'a> {
    pub node_id: &'a str,
    pub result: &'a str,
    pub node_states: &'a HashMap<String, TaskNodeState>,
    pub input_message: &'a str,
}

/// Result of a dynamic expand attempt.
#[derive(Debug)]
pub struct ExpandResult {
    pub nodes_added: usize,
    pub edges_added: usize,
}

/// Calls the LLM to decompose a complex node into a subgraph (nodes + edges).
///
/// Uses [`AGOT_EXPAND_SYSTEM`] prompt. The LLM returns short node ids (e.g. step1,
/// step2); we prefix them with `parent_id` to avoid collisions. Returns `None` on
/// parse failure or empty result.
///
/// **Interaction**: Called by ExecuteGraphNode before `maybe_expand` when adaptive
/// and complexity is Complex.
pub async fn expand_node_via_llm(
    llm: Arc<dyn LlmClient>,
    ctx: &ExpandContext<'_>,
    node: &TaskNode,
) -> Result<Option<(Vec<TaskNode>, Vec<(String, String)>)>, AgentError> {
    let user_content = format!(
        r#"Parent node id: {}
Parent description: {}
Parent result (so far): {}

Overall task goal: {}

Output JSON with "nodes" and "edges". Node ids must be short (e.g. step1, step2). At least one edge must be from "{}" to a new node."#,
        ctx.node_id, node.description, ctx.result, ctx.input_message, ctx.node_id
    );

    let messages = vec![
        Message::system(AGOT_EXPAND_SYSTEM.to_string()),
        Message::user(user_content),
    ];
    let response = llm.invoke(&messages).await?;
    let raw = response.content.trim();

    parse_expand_output(raw, ctx.node_id)
}

/// Parses LLM expand output into (nodes, edges) with prefixed node ids.
fn parse_expand_output(
    raw: &str,
    parent_id: &str,
) -> Result<Option<(Vec<TaskNode>, Vec<(String, String)>)>, AgentError> {
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

    let parsed: RawGraph = serde_json::from_str(raw)
        .map_err(|e| AgentError::ExecutionFailed(format!("expand parse error: {}", e)))?;

    let Some(nodes) = parsed.nodes else {
        return Ok(None);
    };
    if nodes.is_empty() {
        return Ok(None);
    }

    let mut new_nodes: Vec<TaskNode> = Vec::with_capacity(nodes.len());
    let mut raw_to_prefixed: HashMap<String, String> = HashMap::new();
    raw_to_prefixed.insert(parent_id.to_string(), parent_id.to_string());

    for (i, n) in nodes.into_iter().enumerate() {
        let raw_id =
            n.id.filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("step{}", i + 1));
        let prefixed_id = format!("{}_sub_{}", parent_id, i + 1);
        raw_to_prefixed.insert(raw_id, prefixed_id.clone());
        new_nodes.push(TaskNode {
            id: prefixed_id,
            description: n.description.unwrap_or_default(),
            tool_calls: vec![],
        });
    }

    let prefixed_ids: std::collections::HashSet<String> =
        new_nodes.iter().map(|n| n.id.clone()).collect();
    let mut new_edges: Vec<(String, String)> = Vec::new();

    for (from, to) in parsed.edges.unwrap_or_default() {
        let Some(from_prefixed) = raw_to_prefixed.get(&from).cloned() else {
            continue;
        };
        let Some(to_prefixed) = raw_to_prefixed.get(&to).cloned() else {
            continue;
        };
        if (from_prefixed == parent_id || prefixed_ids.contains(&from_prefixed))
            && prefixed_ids.contains(&to_prefixed)
        {
            new_edges.push((from_prefixed, to_prefixed));
        }
    }

    if new_edges.is_empty() {
        return Ok(None);
    }

    Ok(Some((new_nodes, new_edges)))
}

/// Attempts to expand the given node into a subgraph when adaptive and complex.
///
/// Flow: (1) if !adaptive, return None; (2) if node not Done, return None;
/// (3) if `complexity_override` is `Some(Simple)`, return None; (4) if `Some(Complex)`, skip
/// heuristic; (5) if `None`, call `complexity_score`; (6) if not Complex, return None; (7) call
/// `expand_fn`, append via append_subgraph, return ExpandResult. Honors MAX_TOTAL_NODES.
///
/// **Interaction**: Called by ExecuteGraphNode after a node transitions to Done. When
/// `agot_llm_complexity` is true, the caller passes `Some(level)` from `complexity_score_via_llm`.
pub fn maybe_expand<F>(
    state: &mut GotState,
    node_id: &str,
    result: &str,
    adaptive: bool,
    complexity_override: Option<ComplexityLevel>,
    expand_fn: F,
) -> Result<Option<ExpandResult>, AppendSubgraphError>
where
    F: FnOnce(&ExpandContext) -> Option<(Vec<TaskNode>, Vec<(String, String)>)>,
{
    if !adaptive {
        return Ok(None);
    }

    let node = match state.task_graph.nodes.iter().find(|n| n.id == node_id) {
        Some(n) => n.clone(),
        None => return Ok(None),
    };

    let node_state = state.node_states.get(node_id);
    if node_state.map(|s| s.status) != Some(TaskStatus::Done) {
        return Ok(None);
    }

    if state.task_graph.nodes.len() >= MAX_TOTAL_NODES {
        return Ok(None);
    }

    let ctx = ExpandContext {
        node_id,
        result,
        node_states: &state.node_states,
        input_message: &state.input_message,
    };

    let is_complex = match complexity_override {
        Some(ComplexityLevel::Simple) => return Ok(None),
        Some(ComplexityLevel::Complex) => true,
        None => complexity_score(&node, &ctx) == ComplexityLevel::Complex,
    };
    if !is_complex {
        return Ok(None);
    }

    let (new_nodes, new_edges) = match expand_fn(&ctx) {
        Some(t) => t,
        None => return Ok(None),
    };

    if new_nodes.is_empty() {
        return Ok(None);
    }

    let n_before = state.task_graph.nodes.len();
    let e_before = state.task_graph.edges.len();

    append_subgraph(&mut state.task_graph, new_nodes, new_edges)?;

    let nodes_added = state.task_graph.nodes.len() - n_before;
    let edges_added = state.task_graph.edges.len() - e_before;

    for n in &state.task_graph.nodes[n_before..] {
        state
            .node_states
            .insert(n.id.clone(), TaskNodeState::default());
    }

    Ok(Some(ExpandResult {
        nodes_added,
        edges_added,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, desc: &str) -> TaskNode {
        TaskNode {
            id: id.to_string(),
            description: desc.to_string(),
            tool_calls: vec![],
        }
    }

    #[test]
    fn complexity_score_simple_short() {
        let n = node("a", "Read file A");
        let ctx = ExpandContext {
            node_id: "a",
            result: "done",
            node_states: &HashMap::new(),
            input_message: "task",
        };
        assert_eq!(complexity_score(&n, &ctx), ComplexityLevel::Simple);
    }

    #[test]
    fn complexity_score_complex_keyword() {
        let n = node("a", "Analyze the results and compare with baseline");
        let ctx = ExpandContext {
            node_id: "a",
            result: "done",
            node_states: &HashMap::new(),
            input_message: "task",
        };
        assert_eq!(complexity_score(&n, &ctx), ComplexityLevel::Complex);
    }

    #[test]
    fn complexity_score_complex_long() {
        let n = node("a", &"x".repeat(150));
        let ctx = ExpandContext {
            node_id: "a",
            result: "done",
            node_states: &HashMap::new(),
            input_message: "task",
        };
        assert_eq!(complexity_score(&n, &ctx), ComplexityLevel::Complex);
    }

    #[test]
    fn maybe_expand_skips_when_not_adaptive() {
        let mut state = GotState {
            input_message: "task".into(),
            task_graph: super::super::state::TaskGraph {
                nodes: vec![node("a", "Analyze")],
                edges: vec![],
            },
            node_states: [(
                "a".into(),
                TaskNodeState {
                    status: TaskStatus::Done,
                    result: Some("r".into()),
                    error: None,
                },
            )]
            .into_iter()
            .collect(),
        };
        let r = maybe_expand(&mut state, "a", "r", false, None, |_| {
            Some((vec![node("a1", "x")], vec![("a".into(), "a1".into())]))
        })
        .unwrap();
        assert!(r.is_none());
        assert_eq!(state.task_graph.nodes.len(), 1);
    }

    #[test]
    fn maybe_expand_skips_simple_node() {
        let mut state = GotState {
            input_message: "task".into(),
            task_graph: super::super::state::TaskGraph {
                nodes: vec![node("a", "Read file")],
                edges: vec![],
            },
            node_states: [(
                "a".into(),
                TaskNodeState {
                    status: TaskStatus::Done,
                    result: Some("r".into()),
                    error: None,
                },
            )]
            .into_iter()
            .collect(),
        };
        let r = maybe_expand(&mut state, "a", "r", true, None, |_| {
            Some((vec![node("a1", "x")], vec![("a".into(), "a1".into())]))
        })
        .unwrap();
        assert!(r.is_none());
    }

    #[tokio::test]
    async fn expand_node_via_llm_parses_and_prefixes() {
        use crate::llm::MockLlm;
        let json = r#"{"nodes":[{"id":"step1","description":"First step"},{"id":"step2","description":"Second step"}],"edges":[["analyze","step1"],["step1","step2"]]}"#;
        let mock = MockLlm::with_no_tool_calls(json);
        let llm = Arc::new(mock);
        let ctx = ExpandContext {
            node_id: "analyze",
            result: "intermediate result",
            node_states: &HashMap::new(),
            input_message: "Overall task",
        };
        let node = node("analyze", "Analyze and compare");
        let out = expand_node_via_llm(llm, &ctx, &node).await.unwrap();
        let (nodes, edges) = out.unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].id, "analyze_sub_1");
        assert_eq!(nodes[0].description, "First step");
        assert_eq!(nodes[1].id, "analyze_sub_2");
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0], ("analyze".into(), "analyze_sub_1".into()));
        assert_eq!(edges[1], ("analyze_sub_1".into(), "analyze_sub_2".into()));
    }

    #[test]
    fn maybe_expand_appends_when_complex() {
        let mut state = GotState {
            input_message: "task".into(),
            task_graph: super::super::state::TaskGraph {
                nodes: vec![node("a", "Analyze and compare")],
                edges: vec![],
            },
            node_states: [(
                "a".into(),
                TaskNodeState {
                    status: TaskStatus::Done,
                    result: Some("r".into()),
                    error: None,
                },
            )]
            .into_iter()
            .collect(),
        };
        let r = maybe_expand(
            &mut state,
            "a",
            "r",
            true,
            Some(ComplexityLevel::Complex),
            |_| {
                Some((
                    vec![node("a1", "step1"), node("a2", "step2")],
                    vec![("a".into(), "a1".into()), ("a".into(), "a2".into())],
                ))
            },
        )
        .unwrap();
        let res = r.unwrap();
        assert_eq!(res.nodes_added, 2);
        assert_eq!(res.edges_added, 2);
        assert_eq!(state.task_graph.nodes.len(), 3);
        assert_eq!(state.node_states.len(), 3);
    }
}
