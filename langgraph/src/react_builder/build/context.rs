//! Run context for the ReAct graph.
//!
//! This module defines [`ReactRunContext`], which holds the built persistence and tool
//! resources required to run a ReAct agent.
//!
//! # Lifecycle
//!
//! ```text
//! ReactBuildConfig
//!        │
//!        ▼
//! build_react_run_context(config)
//!        │
//!        ▼
//! ReactRunContext { checkpointer, store, runnable_config, tool_source }
//!        │
//!        ├── ctx.tool_source.as_ref()  →  used when building LLM (tool specs)
//!        │
//!        └── ctx.*  →  ReactRunner::new(llm, ctx.tool_source, ctx.checkpointer, ctx.store, ctx.runnable_config, ...)
//! ```
//!
//! # Callers
//!
//! | Caller | Usage |
//! |--------|-------|
//! | [`build_react_runner`](super::build_react_runner) | Calls [`build_react_run_context`](super::build_react_run_context) internally, consumes `ctx` to construct [`ReactRunner`](crate::react::ReactRunner). |
//!
//! # Interacting types
//!
//! - **Creates**: [`build_react_run_context`](super::build_react_run_context)
//! - **Consumes**: [`build_react_runner`](super::build_react_runner), [`ReactRunner::new`](crate::react::ReactRunner::new)
//! - **External consumers**: CLIs and other crates that build ReAct runners

use std::sync::Arc;

use crate::memory::RunnableConfig;
use crate::state::ReActState;
use crate::tool_source::ToolSource;

/// Context for running the ReAct graph: persistence (checkpointer, store, runnable_config)
/// and tool source built from config.
///
/// Produced by [`build_react_run_context`](super::build_react_run_context). All fields are
/// consumed by [`ReactRunner::new`](crate::react::ReactRunner::new) when building a runner.
/// Callers may also use `tool_source.as_ref()` when
/// constructing the LLM to pass tool specs.
pub struct ReactRunContext {
    /// Short-term memory checkpointer. `Some` when [`ReactBuildConfig::thread_id`](super::super::config::ReactBuildConfig::thread_id) is set;
    /// `None` otherwise. Interacts with [`SqliteSaver`](crate::memory::SqliteSaver) (implementation).
    pub checkpointer: Option<Arc<dyn crate::memory::Checkpointer<ReActState>>>,
    /// Long-term memory store. `Some` when embedding config is available (e.g. `user_id` + `EMBEDDING_API_KEY`);
    /// `None` otherwise. Interacts with [`InMemoryVectorStore`](crate::memory::InMemoryVectorStore) (implementation).
    pub store: Option<Arc<dyn crate::memory::Store>>,
    /// Runtime config for thread_id, checkpoint_id, user_id. `Some` when `thread_id` or `user_id`
    /// is set in config; `None` otherwise. Passed to [`run_react_graph`](crate::run_react_graph) for
    /// checkpoint resume and store namespace.
    pub runnable_config: Option<RunnableConfig>,
    /// Tool source providing tools to the agent. Always includes [`web_fetcher`](crate::tool_source::TOOL_WEB_FETCHER)
    /// (WebToolsSource). When no memory and no Exa, only web_fetcher; otherwise
    /// [`AggregateToolSource`](crate::tools::AggregateToolSource) with optional [`MemoryToolsSource`](crate::tool_source::MemoryToolsSource),
    /// MCP Exa, and web_fetcher.
    /// Callers may use `.as_ref()` when building the LLM to pass
    /// tool specs; then moved into [`ReactRunner::new`](crate::react::ReactRunner::new).
    pub tool_source: Box<dyn ToolSource>,
}
