//! Builds checkpointer, store, runnable_config and tool_source from [`ReactBuildConfig`](super::config::ReactBuildConfig).
//!
//! Used by CLI or other callers that hold a [`ReactBuildConfig`](super::config::ReactBuildConfig).
//! Requires `sqlite` and `mcp` features (SqliteSaver, SqliteStore, McpToolSource).

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
use crate::react::ReactRunner;
use crate::state::ReActState;
use crate::tot::TotRunner;
use crate::LlmClient;

use super::config::ReactBuildConfig;
use crate::prompts::AgentPrompts;
use llm::build_default_llm_with_tool_source;
use store::build_store;
use tool_source::build_tool_source;

pub use context::ReactRunContext;
pub use error::BuildRunnerError;

fn to_agent_error(e: impl std::fmt::Display) -> AgentError {
    AgentError::ExecutionFailed(e.to_string())
}

/// Builds checkpointer when thread_id is set; otherwise returns None.
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

/// Builds runnable_config when thread_id or user_id is set; otherwise returns None.
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

/// Builds checkpointer, store, runnable_config and tool_source from the given config.
///
/// Requires `sqlite` and `mcp` features. Callers build [`ReactBuildConfig`](super::config::ReactBuildConfig)
/// from their own config and pass it here.
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

/// Builds a [`ReactRunner`](crate::react::ReactRunner) from config and optional LLM.
///
/// When `llm` is `Some`, that client is used. When `llm` is `None`, the library builds a default
/// LLM from config if `openai_api_key` and `model` (or env) are set (requires `openai` feature);
/// otherwise returns [`BuildRunnerError::NoLlm`].
///
/// Uses [`build_react_run_context`](build_react_run_context) for persistence and tool source,
/// then compiles the ReAct graph with optional checkpointer. System prompt is resolved in order:
/// `config.system_prompt` → `agent_prompts.react_system_prompt()` when `agent_prompts` is `Some` →
/// in-code [`REACT_SYSTEM_PROMPT`](crate::react::REACT_SYSTEM_PROMPT).
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

/// Wraps `Box<dyn LlmClient>` so it can be stored in `Arc<dyn LlmClient>`.
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

/// Builds a [`DupRunner`](crate::dup::DupRunner) from config and optional LLM.
///
/// Same as [`build_react_runner`] but returns a DUP runner (understand → plan → act → observe).
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

/// Builds a [`TotRunner`](crate::tot::TotRunner) from config and optional LLM.
///
/// Same as [`build_react_runner`] but returns a ToT runner (think_expand → think_evaluate → act | end).
/// Uses default max_depth=5 and candidates_per_step=3 when not configured.
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
        false, // research_quality_addon: opt-in via config when needed
    )?;
    Ok(runner)
}

/// Builds a [`GotRunner`](crate::got::GotRunner) from config and optional LLM.
///
/// Same as [`build_react_runner`] but returns a GoT runner (plan_graph → execute_graph).
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

/// Builds a [`ReactRunner`](crate::react::ReactRunner) with an OpenAI client from explicit config and model.
///
/// Convenience when you already have an [`OpenAIConfig`](async_openai::config::OpenAIConfig).
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
