//! Builds checkpointer, store, runnable_config and tool_source from ReactBuildConfig.

mod context;
mod error;
mod llm;
mod store;
mod tool_source;

use std::sync::Arc;

use crate::agent::dup::{DupRunner, DupState};
use crate::agent::got::{GotRunner, GotState};
use crate::agent::tot::{TotRunner, TotState};
use crate::error::AgentError;
use crate::memory::{Checkpointer, JsonSerializer, RunnableConfig, SqliteSaver};
use crate::state::ReActState;
use crate::LlmClient;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::config::ReactBuildConfig;
use super::runner::ReactRunner;
use crate::prompts::AgentPrompts;
use llm::build_default_llm_with_tool_source;
use store::build_store;
use tool_source::build_tool_source;

pub use context::ReactRunContext;
pub use error::BuildRunnerError;

fn to_agent_error(e: impl std::fmt::Display) -> AgentError {
    AgentError::ExecutionFailed(e.to_string())
}

/// Builds an optional checkpointer for state type `S` when `config.thread_id` is set.
/// Shared by ReAct, DUP, ToT, and GoT runners to avoid duplicating SqliteSaver construction.
fn build_checkpointer_for_state<S>(
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

fn build_checkpointer(
    config: &ReactBuildConfig,
    db_path: &str,
) -> Result<Option<Arc<dyn Checkpointer<ReActState>>>, AgentError> {
    build_checkpointer_for_state::<ReActState>(config, db_path)
}

fn build_runnable_config(config: &ReactBuildConfig) -> Option<RunnableConfig> {
    if config.thread_id.is_none() && config.user_id.is_none() {
        return None;
    }
    Some(RunnableConfig {
        thread_id: config.thread_id.clone(),
        checkpoint_id: None,
        checkpoint_ns: String::new(),
        user_id: config.user_id.clone(),
        resume_from_node_id: None,
    })
}

pub async fn build_react_run_context(
    config: &ReactBuildConfig,
) -> Result<ReactRunContext, AgentError> {
    let db_path = config.db_path.as_deref().unwrap_or("memory.db");

    let checkpointer = build_checkpointer(config, db_path)?;
    let store = build_store(config, db_path)?;
    let runnable_config = build_runnable_config(config);
    let tool_source = build_tool_source(config, &store).await?;

    Ok(ReactRunContext {
        checkpointer,
        store,
        runnable_config,
        tool_source,
    })
}

pub async fn build_react_runner(
    config: &ReactBuildConfig,
    llm: Option<Box<dyn LlmClient>>,
    verbose: bool,
    agent_prompts: Option<&AgentPrompts>,
) -> Result<ReactRunner, BuildRunnerError> {
    let ctx = build_react_run_context(config).await?;
    let llm = match llm {
        Some(l) => l,
        None => build_default_llm_with_tool_source(config, ctx.tool_source.as_ref()).await?,
    };
    let system_prompt = config
        .system_prompt
        .clone()
        .or_else(|| agent_prompts.map(|p| p.react_system_prompt()));
    let runner = ReactRunner::new(
        llm,
        ctx.tool_source,
        ctx.checkpointer,
        ctx.store,
        ctx.runnable_config,
        system_prompt,
        config.approval_policy,
        config.compaction_config.clone(),
        verbose,
    )?;
    Ok(runner)
}

struct BoxedLlmClient(Box<dyn LlmClient>);

#[async_trait::async_trait]
impl LlmClient for BoxedLlmClient {
    async fn invoke(
        &self,
        messages: &[crate::message::Message],
    ) -> Result<crate::llm::LlmResponse, AgentError> {
        self.0.invoke(messages).await
    }
    async fn invoke_stream(
        &self,
        messages: &[crate::message::Message],
        tx: Option<tokio::sync::mpsc::Sender<crate::stream::MessageChunk>>,
    ) -> Result<crate::llm::LlmResponse, AgentError> {
        self.0.invoke_stream(messages, tx).await
    }
}

pub async fn build_dup_runner(
    config: &ReactBuildConfig,
    llm: Option<Box<dyn LlmClient>>,
    verbose: bool,
) -> Result<DupRunner, BuildRunnerError> {
    let ctx = build_react_run_context(config).await?;
    let llm = match llm {
        Some(l) => l,
        None => build_default_llm_with_tool_source(config, ctx.tool_source.as_ref()).await?,
    };
    let llm_arc: Arc<dyn LlmClient> = Arc::new(BoxedLlmClient(llm));

    let db_path = config.db_path.as_deref().unwrap_or("memory.db");
    let dup_checkpointer = build_checkpointer_for_state::<DupState>(config, db_path)?;

    let runner = DupRunner::new(
        llm_arc,
        ctx.tool_source,
        dup_checkpointer,
        ctx.store,
        ctx.runnable_config,
        config.system_prompt.clone(),
        config.approval_policy,
        verbose,
    )?;
    Ok(runner)
}

pub async fn build_tot_runner(
    config: &ReactBuildConfig,
    llm: Option<Box<dyn LlmClient>>,
    verbose: bool,
) -> Result<TotRunner, BuildRunnerError> {
    let ctx = build_react_run_context(config).await?;
    let llm = match llm {
        Some(l) => l,
        None => build_default_llm_with_tool_source(config, ctx.tool_source.as_ref()).await?,
    };
    let llm_arc: Arc<dyn LlmClient> = Arc::new(BoxedLlmClient(llm));

    let db_path = config.db_path.as_deref().unwrap_or("memory.db");
    let tot_checkpointer = build_checkpointer_for_state::<TotState>(config, db_path)?;

    let tot = &config.tot_config;
    let runner = TotRunner::new(
        llm_arc,
        ctx.tool_source,
        tot_checkpointer,
        ctx.store,
        ctx.runnable_config,
        config.system_prompt.clone(),
        config.approval_policy,
        verbose,
        tot.max_depth,
        tot.candidates_per_step,
        tot.research_quality_addon,
    )?;
    Ok(runner)
}

pub async fn build_got_runner(
    config: &ReactBuildConfig,
    llm: Option<Box<dyn LlmClient>>,
    verbose: bool,
) -> Result<GotRunner, BuildRunnerError> {
    let ctx = build_react_run_context(config).await?;
    let llm = match llm {
        Some(l) => l,
        None => build_default_llm_with_tool_source(config, ctx.tool_source.as_ref()).await?,
    };
    let llm_arc: Arc<dyn LlmClient> = Arc::new(BoxedLlmClient(llm));

    let db_path = config.db_path.as_deref().unwrap_or("memory.db");
    let got_checkpointer = build_checkpointer_for_state::<GotState>(config, db_path)?;

    let got = &config.got_config;
    let runner = GotRunner::new(
        llm_arc,
        ctx.tool_source,
        got_checkpointer,
        ctx.store,
        ctx.runnable_config,
        verbose,
        got.adaptive,
        got.agot_llm_complexity,
    )?;
    Ok(runner)
}

pub async fn build_react_runner_with_openai(
    config: &ReactBuildConfig,
    openai_config: async_openai::config::OpenAIConfig,
    model: impl Into<String>,
    verbose: bool,
) -> Result<ReactRunner, BuildRunnerError> {
    use crate::llm::ChatOpenAI;
    let client = ChatOpenAI::with_config(openai_config, model);
    build_react_runner(config, Some(Box::new(client)), verbose, None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::react::{GotRunnerConfig, TotRunnerConfig};
    use crate::MockLlm;

    fn base_config() -> ReactBuildConfig {
        ReactBuildConfig {
            db_path: None,
            thread_id: None,
            user_id: None,
            system_prompt: None,
            exa_api_key: None,
            twitter_api_key: None,
            mcp_exa_url: "https://mcp.exa.ai/mcp".to_string(),
            mcp_remote_cmd: "npx".to_string(),
            mcp_remote_args: "-y mcp-remote".to_string(),
            mcp_verbose: false,
            openai_api_key: None,
            openai_base_url: None,
            model: None,
            embedding_api_key: None,
            embedding_base_url: None,
            embedding_model: None,
            working_folder: None,
            approval_policy: None,
            compaction_config: None,
            tot_config: TotRunnerConfig::default(),
            got_config: GotRunnerConfig::default(),
        }
    }

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

    #[tokio::test]
    async fn build_react_run_context_builds_default_tool_source() {
        let ctx = build_react_run_context(&base_config()).await.unwrap();
        assert!(ctx.checkpointer.is_none());
        assert!(ctx.store.is_none());
        assert!(ctx.runnable_config.is_none());
        let tools = ctx.tool_source.list_tools().await.unwrap();
        assert!(!tools.is_empty());
    }

    #[tokio::test]
    async fn build_react_runner_with_mock_llm_and_prompts_invokes() {
        let cfg = base_config();
        let mut prompts = AgentPrompts::default();
        prompts.react.system_prompt = Some("test system prompt".to_string());
        let runner = build_react_runner(
            &cfg,
            Some(Box::new(MockLlm::with_no_tool_calls("react final"))),
            false,
            Some(&prompts),
        )
        .await
        .unwrap();
        let out = runner.invoke("hello").await.unwrap();
        assert!(out.last_assistant_reply().is_some());
    }

    #[tokio::test]
    async fn build_dup_tot_got_runners_with_mock_llm_invoke() {
        let cfg = base_config();

        let dup = build_dup_runner(
            &cfg,
            Some(Box::new(MockLlm::with_no_tool_calls("dup final"))),
            false,
        )
        .await
        .unwrap();
        let dup_out = dup.invoke("q").await.unwrap();
        assert!(dup_out.last_assistant_reply().is_some());

        let tot = build_tot_runner(
            &cfg,
            Some(Box::new(MockLlm::with_no_tool_calls("tot final"))),
            false,
        )
        .await
        .unwrap();
        let tot_out = tot.invoke("q").await.unwrap();
        assert!(tot_out.last_assistant_reply().is_some());

        let got = build_got_runner(
            &cfg,
            Some(Box::new(MockLlm::with_no_tool_calls("got final"))),
            false,
        )
        .await
        .unwrap();
        let got_out = got.invoke("q").await.unwrap();
        assert!(!got_out.summary_result().is_empty() || !got_out.task_graph.nodes.is_empty());
    }
}
