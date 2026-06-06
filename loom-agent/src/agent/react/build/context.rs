//! Run context for the ReAct graph.

use std::sync::Arc;

use loom::llm::audit::LlmAuditLog;
use loom::memory::RunnableConfig;
use loom::state::ReActState;
use loom::tool_source::ToolSource;

/// Context for running the ReAct graph.
pub struct ReactRunContext {
    pub checkpointer: Option<Arc<dyn loom::memory::Checkpointer<ReActState>>>,
    pub store: Option<Arc<dyn loom::memory::Store>>,
    pub runnable_config: Option<RunnableConfig>,
    pub tool_source: Box<dyn ToolSource>,
    pub audit_log: Option<Arc<dyn LlmAuditLog>>,
}

