//! Run context for the ReAct graph.

use std::sync::Arc;

use crate::memory::RunnableConfig;
use crate::state::ReActState;
use crate::tool_source::ToolSource;

/// Context for running the ReAct graph.
pub struct ReactRunContext {
    pub checkpointer: Option<Arc<dyn crate::memory::Checkpointer<ReActState>>>,
    pub store: Option<Arc<dyn crate::memory::Store>>,
    pub runnable_config: Option<RunnableConfig>,
    pub tool_source: Box<dyn ToolSource>,
}
