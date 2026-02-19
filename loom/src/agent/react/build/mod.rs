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
