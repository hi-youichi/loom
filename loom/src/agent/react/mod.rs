//! ReAct: graph nodes (Think, Act, Observe), runner, config-driven builder.
//!
//! This module provides the three nodes and runner for the minimal ReAct chain
//! think → act → observe, plus a config-driven builder for CLIs.
//!
//! # Main types
//!
//! - **[`ThinkNode`]**: Calls the LLM with current messages; may output tool calls.
//! - **[`ActNode`]**: Executes tool_calls via ToolSource and fills tool_results.
//! - **[`ObserveNode`]**: Merges tool results into messages, clears tool_calls/tool_results.
//! - **[`ReactRunner`]**: Holds compiled graph, checkpointer, store, LLM, tool source.
//! - **[`ReactBuildConfig`]**: Configuration for building run context and runners.
//! - **[`ReactRunContext`]**: Built checkpointer, store, runnable_config, tool_source.
//!
//! # Builder API
//!
//! Use [`ReactBuildConfig::from_env`] or build config programmatically, then call
//! [`build_react_runner`] or [`build_react_run_context`].

mod act_node;
mod build;
mod config;
mod observe_node;
mod runner;
mod think_node;
mod with_node_logging;

pub use act_node::{
    ActNode, ErrorHandlerFn, HandleToolErrors, DEFAULT_EXECUTION_ERROR_TEMPLATE,
    DEFAULT_TOOL_ERROR_TEMPLATE, STEP_PROGRESS_EVENT_TYPE,
};
pub use build::{
    build_dup_runner, build_got_runner, build_react_run_context, build_react_runner,
    build_react_runner_with_openai, build_tot_runner, BuildRunnerError, ReactRunContext,
};
pub use config::{GotRunnerConfig, ReactBuildConfig, TotRunnerConfig};
pub use observe_node::ObserveNode;
pub use runner::{
    build_react_initial_state, run_agent, run_react_graph_stream, ReactRunner, AgentOptions,
    RunError,
};
pub use think_node::ThinkNode;
pub use with_node_logging::WithNodeLogging;

use crate::state::ReActState;

/// Output of the tools_condition function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolsConditionResult {
    /// Route to the tools execution node ("tools" or "act").
    Tools,
    /// Route to the end node ("__end__").
    End,
}

impl ToolsConditionResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::End => "__end__",
        }
    }
}

/// Conditional routing: if tool_calls present, route to act; else end.
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
        assert_eq!(tools_condition(&state), ToolsConditionResult::End);
        assert_eq!(tools_condition(&state).as_str(), "__end__");
    }

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
        assert_eq!(tools_condition(&state), ToolsConditionResult::Tools);
        assert_eq!(tools_condition(&state).as_str(), "tools");
    }
}

/// Default system prompt for ReAct agents.
pub const REACT_SYSTEM_PROMPT: &str = r#""#;

// pub const REACT_SYSTEM_PROMPT: &str = r#"You are an agent that follows the ReAct pattern (Reasoning + Acting).

// RULES:
// 0. LANGUAGE: Reply in the same language the user used (e.g. if they write in Chinese, reply in Chinese; if in English, reply in English).
// 1. THOUGHT first: Before any action, reason "Do I need external information?"
//    - If the question can be answered with your knowledge (math, general knowledge, reasoning) → give FINAL_ANSWER directly. Do NOT call tools.
//    - Only call tools when the user explicitly needs data you cannot know from training: current time, weather, search results, local file system content, etc.
// 2. Use ACTION: call tools only when truly needed, or give FINAL_ANSWER when you have enough.
// 3. After each tool result (OBSERVATION), reason about what you learned and decide the next step.
// 4. Be thorough but concise in your reasoning.
// 5. When using tool data, cite or summarize it clearly in your final answer.
// 6. RESEARCH/HOW-TO: For research or how-to questions (e.g. "how to do X", "best practices for Y"), you MUST use search tools at least 2–3 times with different queries or angles. Do NOT give FINAL_ANSWER after only one search. Synthesize from the gathered content, then give your final answer.

// PHASES:
// - THOUGHT: Reason about what the user needs, what you already have, and whether any tool would help.
// - ACTION: Execute one tool at a time, or give FINAL_ANSWER with your complete response.
// - OBSERVATION: After seeing tool output, analyze it and either call another tool or answer.

// Explain your reasoning clearly. Use tools only when they can help; for simple questions, answer directly. Do not make up facts; use tool results when available."#;
