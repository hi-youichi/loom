//! ReAct graph nodes: Think, Act, Observe, and routing utilities.
//!
//! This module provides the three nodes and runner for the minimal ReAct chain
//! think → act → observe. Each node implements [`Node`](crate::graph::Node) with state type [`ReActState`].
//!
//! # Main types
//!
//! - **[`ThinkNode`]**: Calls the LLM with current messages; may output tool calls. Add after
//!   [`ObserveNode`] in the graph so the cycle is observe → think → (condition) → act or end.
//! - **[`ActNode`]**: Executes [`state.tool_calls`](crate::state::ReActState::tool_calls) via
//!   [`ToolSource`](crate::tool_source::ToolSource) and fills `tool_results`. Use
//!   [`HandleToolErrors`] to customize error handling.
//! - **[`ObserveNode`]**: Merges tool results into messages and clears `tool_calls`/`tool_results`;
//!   increments turn count. Typically the last node before looping back to think or ending.
//! - **[`ReactRunner`]**: Holds compiled graph, checkpointer, store, LLM, and tool source. Use
//!   [`run_react_graph`] or [`run_react_graph_stream`] to run; build state with
//!   [`build_react_initial_state`].
//! - **[`tools_condition`]**: Conditional routing: if there are tool calls, go to act; else end.
//!   Returns [`ToolsConditionResult`]; use [`.as_str()`](ToolsConditionResult::as_str) for node IDs.
//!
//! # Routing
//!
//! Use [`tools_condition`] with [`StateGraph::add_conditional_edges`](crate::graph::StateGraph::add_conditional_edges) from the think node:
//!
//! ```rust,ignore
//! use std::collections::HashMap;
//! use std::sync::Arc;
//!
//! let path_map: HashMap<String, String> = [
//!     ("tools".into(), "act".into()),
//!     (graphweave::graph::END.into(), graphweave::graph::END.into()),
//! ].into_iter().collect();
//! graph.add_conditional_edges(
//!     "think",
//!     Arc::new(|state: &ReActState| tools_condition(state).as_str().to_string()),
//!     Some(path_map),
//! );
//! ```

mod act_node;
mod observe_node;
mod runner;
mod think_node;
mod with_node_logging;

pub use act_node::{
    ActNode, ErrorHandlerFn, HandleToolErrors, DEFAULT_EXECUTION_ERROR_TEMPLATE,
    DEFAULT_TOOL_ERROR_TEMPLATE, STEP_PROGRESS_EVENT_TYPE,
};
pub use observe_node::ObserveNode;
pub use runner::{
    build_react_initial_state, run_react_graph, run_react_graph_stream, ReactRunner, RunError,
};
pub use think_node::ThinkNode;
pub use with_node_logging::WithNodeLogging;

use crate::state::ReActState;

/// Output of the tools_condition function.
///
/// - `Tools` - Route to the tools/act node
/// - `End` - Route to the end node
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolsConditionResult {
    /// Route to the tools execution node ("tools" or "act").
    Tools,
    /// Route to the end node ("__end__").
    End,
}

impl ToolsConditionResult {
    /// Returns the node ID string for this routing result.
    ///
    /// - `Tools` -> `"tools"`
    /// - `End` -> `"__end__"`
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::End => "__end__",
        }
    }
}

/// Conditional routing function for ReAct-style tool-calling workflows.
///
/// This utility function implements the standard conditional logic for ReAct-style
/// agents: if the state contains tool calls, route to the tool execution node;
/// otherwise, end the workflow.
///
/// # Arguments
///
/// * `state` - The current ReActState to examine for tool calls
///
/// # Returns
///
/// * `ToolsConditionResult::Tools` - If `state.tool_calls` is not empty
/// * `ToolsConditionResult::End` - If `state.tool_calls` is empty
///
/// # Example
///
/// ```rust,ignore
/// use graphweave::react::tools_condition;
/// use graphweave::graph::StateGraph;
/// use std::collections::HashMap;
/// use std::sync::Arc;
///
/// let mut graph = StateGraph::new();
/// graph.add_node("think", think_node);
/// graph.add_node("act", act_node);
///
/// let path_map: HashMap<String, String> = [
///     ("tools".into(), "act".into()),
///     (graphweave::graph::END.into(), graphweave::graph::END.into()),
/// ].into_iter().collect();
/// graph.add_conditional_edges(
///     "think",
///     Arc::new(|state| tools_condition(state).as_str().to_string()),
///     Some(path_map),
/// );
/// ```
///
/// # Notes
///
/// - This function only examines `state.tool_calls`, not the messages
/// - Tool calls are typically populated by `ThinkNode` when the LLM decides to call tools
/// - If your state structure differs, you may need to implement a custom condition function
pub fn tools_condition(state: &ReActState) -> ToolsConditionResult {
    if state.tool_calls.is_empty() {
        ToolsConditionResult::End
    } else {
        ToolsConditionResult::Tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ToolCall;
    use crate::Message;

    /// **Scenario**: tools_condition returns End when no tool calls.
    #[test]
    fn tools_condition_returns_end_when_no_tool_calls() {
        let state = ReActState {
            messages: vec![Message::User("hello".into())],
            tool_calls: vec![],
            tool_results: vec![],
            turn_count: 0,
            approval_result: None,
            usage: None,
            total_usage: None,
            message_count_after_last_think: None,
        };

        let result = tools_condition(&state);
        assert_eq!(result, ToolsConditionResult::End);
        assert_eq!(result.as_str(), "__end__");
    }

    /// **Scenario**: tools_condition returns Tools when tool calls present.
    #[test]
    fn tools_condition_returns_tools_when_tool_calls_present() {
        let state = ReActState {
            messages: vec![Message::User("search".into())],
            tool_calls: vec![ToolCall {
                id: Some("tc1".into()),
                name: "search".into(),
                arguments: "{}".into(),
            }],
            tool_results: vec![],
            turn_count: 0,
            approval_result: None,
            usage: None,
            total_usage: None,
            message_count_after_last_think: None,
        };

        let result = tools_condition(&state);
        assert_eq!(result, ToolsConditionResult::Tools);
        assert_eq!(result.as_str(), "tools");
    }

    /// **Scenario**: ToolsConditionResult as_str returns correct values.
    #[test]
    fn tools_condition_result_as_str() {
        assert_eq!(ToolsConditionResult::Tools.as_str(), "tools");
        assert_eq!(ToolsConditionResult::End.as_str(), "__end__");
    }
}

/// Default system prompt for ReAct agents.
///
/// Follows the Thought → Action → Observation pattern. Prepend as the first
/// message in `ReActState::messages` when building state so the LLM reasons
/// before acting and analyzes tool results. Callers can use a custom system
/// message instead; ThinkNode does not inject this automatically.
pub const REACT_SYSTEM_PROMPT: &str = r#"You are an agent that follows the ReAct pattern (Reasoning + Acting).

RULES:
0. LANGUAGE: Reply in the same language the user used (e.g. if they write in Chinese, reply in Chinese; if in English, reply in English).
1. THOUGHT first: Before any action, reason "Do I need external information?"
   - If the question can be answered with your knowledge (math, general knowledge, reasoning) → give FINAL_ANSWER directly. Do NOT call tools.
   - Only call tools when the user explicitly needs data you cannot know: current time, weather, search, etc.
2. Use ACTION: call tools only when truly needed, or give FINAL_ANSWER when you have enough.
3. After each tool result (OBSERVATION), reason about what you learned and decide the next step.
4. Be thorough but concise in your reasoning.
5. When using tool data, cite or summarize it clearly in your final answer.
6. RESEARCH/HOW-TO: For research or how-to questions (e.g. "how to do X", "best practices for Y"), you MUST use search tools at least 2–3 times with different queries or angles. Do NOT give FINAL_ANSWER after only one search. Synthesize from the gathered content, then give your final answer.

PHASES:
- THOUGHT: Reason about what the user needs, what you already have, and whether any tool would help.
- ACTION: Execute one tool at a time, or give FINAL_ANSWER with your complete response.
- OBSERVATION: After seeing tool output, analyze it and either call another tool or answer.

Explain your reasoning clearly. Use tools only when they can help; for simple questions, answer directly. Do not make up facts; use tool results when available."#;
