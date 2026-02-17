//! Builds checkpointer, store, runnable_config and tool_source from ReactBuildConfig.

mod context;
mod error;
mod llm;
mod store;
mod tool_source;

use std::sync::Arc;

use crate::dup::DupRunner;
use crate::error::AgentError;
use crate::got::GotRunner;
use crate::memory::{JsonSerializer, RunnableConfig, SqliteSaver};
use crate::state::ReActState;
use crate::tot::TotRunner;
use crate::LlmClient;

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

fn build_checkpointer(
    config: &ReactBuildConfig,
    db_path: &str,
) -> Result<Option<Arc<dyn crate::memory::Checkpointer<ReActState>>>, AgentError> {
    if config.thread_id.is_none() {
        return Ok(None);
    }
    let serializer = Arc::new(JsonSerializer);
    let saver = SqliteSaver::new(db_path, serializer).map_err(to_agent_error)?;
    Ok(Some(
        Arc::new(saver) as Arc<dyn crate::memory::Checkpointer<ReActState>>
    ))
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

    let dup_checkpointer = if ctx.checkpointer.is_some() {
        let db_path = config.db_path.as_deref().unwrap_or("memory.db");
        let serializer = Arc::new(JsonSerializer);
        let saver = SqliteSaver::new(db_path, serializer)
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;
        Some(Arc::new(saver) as Arc<dyn crate::memory::Checkpointer<crate::dup::DupState>>)
    } else {
        None
    };

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

    let tot_checkpointer = if ctx.checkpointer.is_some() {
        let db_path = config.db_path.as_deref().unwrap_or("memory.db");
        let serializer = Arc::new(JsonSerializer);
        let saver = SqliteSaver::new(db_path, serializer)
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;
        Some(Arc::new(saver) as Arc<dyn crate::memory::Checkpointer<crate::tot::TotState>>)
    } else {
        None
    };

    let max_depth = 5u32;
    let candidates_per_step = 3u32;

    let runner = TotRunner::new(
        llm_arc,
        ctx.tool_source,
        tot_checkpointer,
        ctx.store,
        ctx.runnable_config,
        config.system_prompt.clone(),
        config.approval_policy,
        verbose,
        max_depth,
        candidates_per_step,
        false,
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

    let got_checkpointer = if ctx.checkpointer.is_some() {
        let db_path = config.db_path.as_deref().unwrap_or("memory.db");
        let serializer = Arc::new(JsonSerializer);
        let saver = SqliteSaver::new(db_path, serializer)
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;
        Some(Arc::new(saver) as Arc<dyn crate::memory::Checkpointer<crate::got::GotState>>)
    } else {
        None
    };

    let runner = GotRunner::new(
        llm_arc,
        ctx.tool_source,
        got_checkpointer,
        ctx.store,
        ctx.runnable_config,
        verbose,
        config.got_adaptive,
        config.got_agot_llm_complexity,
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
