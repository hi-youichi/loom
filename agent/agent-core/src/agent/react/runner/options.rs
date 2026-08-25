//! Optional configuration for run_react_graph_stream, and its resolved form.

use std::sync::Arc;

use crate::state::ReActState;
use checkpoint::{Checkpointer, RunnableConfig, Store};
use checkpoint_sqlite_store::user_message::UserMessageStore;
use anureo_llm::LlmProvider;
use tool_core::ToolRegistryLocked;

/// Optional configuration for [`super::run_agent`] and [`super::run_react_graph_stream`].
///
/// When a field is `None`, defaults are used: `provider` and `tool_source` default to
/// mock implementations so that `run_agent("What time is it?", None)` works for quick demos.
#[derive(Default)]
pub struct AgentOptions {
    /// LLM provider. Defaults to a mock that returns one tool call then a final reply.
    pub provider: Option<Arc<dyn LlmProvider>>,
    /// Tool source. Defaults to a mock that provides `get_time`.
    pub tool_source: Option<Arc<ToolRegistryLocked>>,
    /// Optional checkpointer for persisting/restoring conversation state.
    pub checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
    /// Optional long-term memory store (e.g. LanceDB).
    pub store: Option<Arc<dyn Store>>,
    /// Optional runtime config (thread_id, user_id, etc.).
    pub runnable_config: Option<RunnableConfig>,
    /// Optional store for user-facing messages per thread (append/list).
    pub user_message_store: Option<Arc<dyn UserMessageStore>>,
    /// If true, log node and state details to stderr.
    pub verbose: bool,
}

pub struct ResolvedRunAgentOptions {
    pub provider: Arc<dyn LlmProvider>,
    pub tool_source: Arc<ToolRegistryLocked>,
    pub checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
    pub store: Option<Arc<dyn Store>>,
    pub runnable_config: Option<RunnableConfig>,
    pub user_message_store: Option<Arc<dyn UserMessageStore>>,
    pub verbose: bool,
}

pub(super) fn resolve_run_agent_options(opts: AgentOptions) -> ResolvedRunAgentOptions {
    let provider = opts.provider.unwrap_or_else(|| {
        let llm = anureo_llm::client::MockLlm::first_tools_then_end();
        Arc::new(anureo_llm::client::FixedLlmProvider {
            client: Arc::from(Box::new(llm) as Box<dyn anureo_llm::LlmClient>),
            model_id: "mock/default".to_string(),
        })
    });
    let tool_source = opts.tool_source.unwrap_or_else(tool_core::mock_registry);
    ResolvedRunAgentOptions {
        provider,
        tool_source,
        checkpointer: opts.checkpointer,
        store: opts.store,
        runnable_config: opts.runnable_config,
        user_message_store: opts.user_message_store,
        verbose: opts.verbose,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_options_default_all_none() {
        let opts = AgentOptions::default();
        assert!(opts.provider.is_none());
        assert!(opts.tool_source.is_none());
        assert!(opts.checkpointer.is_none());
        assert!(opts.store.is_none());
        assert!(opts.runnable_config.is_none());
        assert!(opts.user_message_store.is_none());
        assert!(!opts.verbose);
    }

    #[test]
    fn resolve_with_defaults() {
        let resolved = resolve_run_agent_options(AgentOptions::default());
        assert!(!resolved.verbose);
        assert!(resolved.checkpointer.is_none());
        assert!(resolved.store.is_none());
    }

    #[test]
    fn resolve_preserves_verbose() {
        let opts = AgentOptions {
            verbose: true,
            ..Default::default()
        };
        let resolved = resolve_run_agent_options(opts);
        assert!(resolved.verbose);
    }
}
