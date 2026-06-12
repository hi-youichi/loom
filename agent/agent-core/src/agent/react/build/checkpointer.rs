use std::sync::Arc;

use loom_llm::error::AgentError;
use loom_memory::{Checkpointer, JsonSerializer, RunnableConfig, SqliteSaver};
use loom_cli_types::ReActState;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::agent::react::config::ReactBuildConfig;

pub(super) fn to_agent_error(e: impl std::fmt::Display) -> AgentError {
    AgentError::ExecutionFailed(e.to_string())
}

pub fn resolve_memory_db_path(config: &ReactBuildConfig) -> String {
    config.db_path.clone().unwrap_or_else(|| {
        loom_memory::default_memory_db_path()
            .to_string_lossy()
            .into_owned()
    })
}

pub fn build_checkpointer_for_state<S>(
    config: &ReactBuildConfig,
    db_path: &str,
) -> Result<Option<Arc<dyn Checkpointer<S>>>, AgentError>
where
    S: Clone + Send + Sync + 'static + Serialize + DeserializeOwned,
{
    if config.thread_id.is_none() {
        return Ok(None);
    }
    let serializer = Arc::new(JsonSerializer);
    let saver = SqliteSaver::new(db_path, serializer).map_err(to_agent_error)?;
    Ok(Some(Arc::new(saver) as Arc<dyn Checkpointer<S>>))
}

pub(super) fn build_checkpointer(
    config: &ReactBuildConfig,
    db_path: &str,
) -> Result<Option<Arc<dyn Checkpointer<ReActState>>>, AgentError> {
    build_checkpointer_for_state::<ReActState>(config, db_path)
}

pub(super) fn build_runnable_config(config: &ReactBuildConfig) -> Option<RunnableConfig> {
    if config.thread_id.is_none() && config.user_id.is_none() {
        return None;
    }
    Some(RunnableConfig {
        thread_id: config.thread_id.clone(),
        checkpoint_id: None,
        checkpoint_ns: String::new(),
        user_id: config.user_id.clone(),
        resume_from_node_id: None,
        depth: None,
        acp_session_id: None,
        resume_value: None,
        resume_values_by_namespace: Default::default(),
        resume_values_by_interrupt_id: Default::default(),
    })
}