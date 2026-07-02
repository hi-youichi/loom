//! Run context for the ReAct graph.

use std::sync::Arc;

use loom_llm::support::audit::LlmAuditLog;
use checkpoint::RunnableConfig;
use crate::state::ReActState;
use tool_core::ToolRegistryLocked;

/// Context for running the ReAct graph.
pub struct ReactRunContext {
    pub checkpointer: Option<Arc<dyn checkpoint::Checkpointer<ReActState>>>,
    pub store: Option<Arc<dyn checkpoint::Store>>,
    pub runnable_config: Option<RunnableConfig>,
    pub tool_source: Arc<ToolRegistryLocked>,
    pub audit_log: Option<Arc<dyn LlmAuditLog>>,
}