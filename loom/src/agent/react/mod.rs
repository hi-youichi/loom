//! ReAct: graph nodes (Think, Act, Observe), runner, config-driven builder.
//!
//! This module packages Loom's default ReAct loop as a small graph:
//!
//! 1. [`ThinkNode`] asks the model for the next action.
//! 2. [`tools_condition`] decides whether the turn should end or route to tools.
//! 3. [`ActNode`] executes tool calls through a [`crate::ToolSource`].
//! 4. [`ObserveNode`] converts tool results back into messages so the next think
//!    step can continue.
//!
//! You can use it at two levels:
//!
//! - Low level: wire [`ThinkNode`], [`ActNode`], and [`ObserveNode`] into a
//!   graph yourself.
//! - High level: use [`ReactBuildConfig`], [`build_react_runner`], or
//!   [`build_react_run_context`] to construct a ready-to-run CLI/runtime setup.
//!
//! # Main types
//!
//! - [`ThinkNode`]: calls the LLM with the current conversation state.
//! - [`ActNode`]: executes tool calls and records tool results.
//! - [`ObserveNode`]: appends tool results to the message list and clears the
//!   tool buffers for the next turn.
//! - [`ReactRunner`]: owns the compiled graph plus the services needed to run it.
//! - [`ReactBuildConfig`]: configuration for building runners from env or files.
//! - [`ReactRunContext`]: resolved checkpointer, store, tool source, and run config.

mod act_node;
mod build;
mod config;
mod observe_node;
mod runner;
mod summarize_node;
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
    build_react_initial_state, run_agent, run_react_graph_stream, AgentOptions, ReactRunner,
    RunError,
};
pub use summarize_node::{is_first_think, SummarizeNode};
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
    /// Returns the edge label used by the compiled graph.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::End => "__end__",
        }
    }
}

/// Chooses the next ReAct edge after a think step.
///
/// Returns [`ToolsConditionResult::Tools`] when the model produced at least one
/// tool call; otherwise returns [`ToolsConditionResult::End`] to finish the turn.
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
            ..Default::default()
        };
        assert_eq!(tools_condition(&state), ToolsConditionResult::End);
        assert_eq!(tools_condition(&state).as_str(), "__end__");
    }

    #[test]
    fn tools_condition_returns_tools_when_tool_calls_present() {
        let state = ReActState {
            messages: vec![Message::User("hello".into())],
            tool_calls: vec![ToolCall {
                name: "test".into(),
                arguments: "{}".into(),
                id: None,
            }],
            ..Default::default()
        };
        assert_eq!(tools_condition(&state), ToolsConditionResult::Tools);
        assert_eq!(tools_condition(&state).as_str(), "tools");
    }
}

/// Default system prompt for ReAct agents. Used as a fallback when no
/// `config.system_prompt` or agent prompt file is provided.
pub const REACT_SYSTEM_PROMPT: &str = r#"You are an agent that follows the ReAct pattern (Reasoning + Acting).

RULES:
0. LANGUAGE: Reply in the same language the user used (e.g. if they write in Chinese, reply in Chinese; if in English, reply in English).
1. THOUGHT first: Before any action, reason "Do I need external information?"
   - If the question can be answered with your knowledge (math, general knowledge, reasoning) → give FINAL_ANSWER directly. Do NOT call tools.
   - Only call tools when the user explicitly needs data you cannot know from training: current time, weather, search results, local file system content, etc.
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
