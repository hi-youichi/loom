use std::sync::Arc;

use loom_llm::error::AgentError;
use loom_memory::{Checkpointer, JsonSerializer, RunnableConfig, SqliteSaver};
use loom_types::state::ReActState;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::super::config::ReactBuildConfig;

pub(super) fn to_agent_error(e: impl std::fmt::Display) -> AgentError {
    AgentError::ExecutionFailed(e.to_string())
}

pub(super) fn resolve_memory_db_path(config: &ReactBuildConfig) -> String {
    config.db_path.clone().unwrap_or_else(|| {
        loom_memory::default_memory_db_path()
            .to_string_lossy()
            .into_owned()
    })
}

pub(super) fn build_checkpointer_for_state<S>(
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
        acp_session_id: config.acp_session_id.clone(),
        resume_value: None,
        resume_values_by_namespace: Default::default(),
        resume_values_by_interrupt_id: Default::default(),
    })
}

#[cfg(test)]
pub(super) fn base_config() -> crate::agent::react::config::ReactBuildConfig {
    use crate::agent::react::{GotRunnerConfig, TotRunnerConfig};
    crate::agent::react::config::ReactBuildConfig {
        db_path: None,
        thread_id: None,
        trace_thread_id: None,
        user_id: None,
        system_prompt: None,
        exa_api_key: None,
        exa_codesearch_enabled: false,
        twitter_api_key: None,
        mcp_exa_url: "https://mcp.exa.ai/mcp".to_string(),
        mcp_remote_cmd: "npx".to_string(),
        mcp_remote_args: "-y mcp-remote".to_string(),
        github_token: None,
        mcp_github_cmd: "npx".to_string(),
        mcp_github_args: vec![
            "-y".to_string(),
            "@modelcontextprotocol/server-github".to_string(),
        ],
        mcp_github_url: None,
        mcp_verbose: false,
        openai_api_key: None,
        openai_base_url: None,
        model: None,
        model_tier: None,
        parent_model_hint: None,
        llm_provider: None,
        llm_provider_name: None,
        openai_temperature: None,
        embedding_api_key: None,
        embedding_base_url: None,
        embedding_model: None,
        working_folder: None,
        approval_policy: None,
        compaction_config: None,
        tot_config: TotRunnerConfig::default(),
        got_config: GotRunnerConfig::default(),
        mcp_servers: None,
        skill_registry: None,
        max_sub_agent_depth: None,
        dry_run: false,
        builtin_tool_filter: None,
        bash_executor: None,
        extra_tools: None,
        acp_session_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::state::ReActState;

    #[test]
    fn build_runnable_config_handles_none_and_some_fields() {
        assert!(build_runnable_config(&base_config()).is_none());

        let mut with_thread = base_config();
        with_thread.thread_id = Some("thread-1".to_string());
        let rc = build_runnable_config(&with_thread).unwrap();
        assert_eq!(rc.thread_id.as_deref(), Some("thread-1"));

        let mut with_user = base_config();
        with_user.user_id = Some("user-1".to_string());
        let rc2 = build_runnable_config(&with_user).unwrap();
        assert_eq!(rc2.user_id.as_deref(), Some("user-1"));
    }

    #[test]
    fn build_checkpointer_for_state_returns_none_without_thread() {
        let cp = build_checkpointer_for_state::<ReActState>(&base_config(), "memory.db").unwrap();
        assert!(cp.is_none());
    }

    #[test]
    fn build_checkpointer_for_state_returns_some_with_thread() {
        let mut cfg = base_config();
        cfg.thread_id = Some("thread-1".to_string());
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("cp.db");
        let cp = build_checkpointer_for_state::<ReActState>(&cfg, db.to_str().unwrap()).unwrap();
        assert!(cp.is_some());
    }
}
