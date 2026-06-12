//! Run context for the ReAct graph.

use std::sync::Arc;

use loom_llm::support::audit::LlmAuditLog;
use loom_memory::RunnableConfig;
use loom_cli_types::ReActState;
use loom_tools::tool_source::ToolSource;

/// Context for running the ReAct graph.
pub struct ReactRunContext {
    pub checkpointer: Option<Arc<dyn loom_memory::Checkpointer<ReActState>>>,
    pub store: Option<Arc<dyn loom_memory::Store>>,
    pub runnable_config: Option<RunnableConfig>,
    pub tool_source: Box<dyn ToolSource>,
    pub audit_log: Option<Arc<dyn LlmAuditLog>>,
}