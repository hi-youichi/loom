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
mod completion_check_node;
mod config;
mod observe_node;
mod runner;
mod title_node;
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
pub(crate) use build::resolve_tier_for_config;
pub use completion_check_node::CompletionCheckNode;
pub use config::{GotRunnerConfig, ReactBuildConfig, TotRunnerConfig};
pub use observe_node::ObserveNode;
pub use runner::{
    build_react_initial_state, run_agent, run_react_graph_stream, AgentOptions, ReactRunner,
    RunError,
};
pub use title_node::{is_first_think, TitleNode};
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

/// Default ReAct **base** system prompt when no `react.yaml` / `REACT_SYSTEM_PROMPT` override.
///
/// The previous RULES/PHASES block (THOUGHT / FINAL_ANSWER / "do not call tools" for in-knowledge
/// questions) is **disabled**: it conflicts with `tool_choice: required` and with tasks that need
/// real workspace listing without hallucination.
///
/// **Restore:** copy `system_prompt` from `loom/prompts/experimental/react.yaml` into
/// `loom/prompts/react.yaml`, or set env `REACT_SYSTEM_PROMPT`. Role / AGENTS.md / Helve sections
/// still apply on top of this empty base.
pub const REACT_SYSTEM_PROMPT: &str = "";

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
