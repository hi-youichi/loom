//! Optional configuration for run_agent and run_react_graph_stream, and its resolved form.

use std::sync::Arc;

use crate::memory::{Checkpointer, RunnableConfig, Store};
use crate::state::ReActState;
use crate::tool_source::ToolSource;
use crate::user_message::UserMessageStore;
use crate::LlmClient;

/// Optional configuration for [`super::run_agent`] and [`super::run_react_graph_stream`].
///
/// When a field is `None`, defaults are used: `llm` and `tool_source` default to
/// mock implementations (e.g. [`crate::MockLlm::first_tools_then_end`], [`crate::MockToolSource::get_time_example`])
/// so that `run_agent("What time is it?", None)` works for quick demos.
#[derive(Default)]
pub struct AgentOptions {
    /// LLM client. Defaults to a mock that returns one tool call then a final reply.
    pub llm: Option<Box<dyn LlmClient>>,
    /// Tool source. Defaults to a mock that provides `get_time`.
    pub tool_source: Option<Box<dyn ToolSource>>,
    /// Optional checkpointer for persisting/restoring conversation state.
    pub checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
    /// Optional long-term memory store (e.g. LanceDB).
    pub store: Option<Arc<dyn Store>>,
    /// Optional runtime config (thread_id, user_id, etc.).
    pub runnable_config: Option<RunnableConfig>,
    /// Optional store for user-facing messages per thread (append/list). With approach C, serve holds the store and may pass None here.
    pub user_message_store: Option<Arc<dyn UserMessageStore>>,
    /// If true, log node and state details to stderr.
    pub verbose: bool,
}

/// Resolved form of [`AgentOptions`]: optional `llm` and `tool_source` are replaced with
/// concrete instances (using mocks when not set); all other fields are passed through as-is.
/// Only used internally by [`resolve_run_agent_options`].
pub(super) struct ResolvedRunAgentOptions {
    pub llm: Box<dyn LlmClient>,
    pub tool_source: Box<dyn ToolSource>,
    pub checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
    pub store: Option<Arc<dyn Store>>,
    pub runnable_config: Option<RunnableConfig>,
    pub user_message_store: Option<Arc<dyn UserMessageStore>>,
    pub verbose: bool,
}

pub(super) fn resolve_run_agent_options(opts: AgentOptions) -> ResolvedRunAgentOptions {
    let llm = opts
        .llm
        .unwrap_or_else(|| Box::new(crate::MockLlm::first_tools_then_end()));
    let tool_source = opts
        .tool_source
        .unwrap_or_else(|| Box::new(crate::MockToolSource::get_time_example()));
    ResolvedRunAgentOptions {
        llm,
        tool_source,
        checkpointer: opts.checkpointer,
        store: opts.store,
        runnable_config: opts.runnable_config,
        user_message_store: opts.user_message_store,
        verbose: opts.verbose,
    }
}
