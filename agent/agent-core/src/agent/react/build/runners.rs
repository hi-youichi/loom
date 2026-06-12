use std::sync::Arc;

use loom_compress::CompactionConfig;
use loom_llm::error::AgentError;
use loom_llm::{LlmClient, LlmProvider};
use loom_llm::client::FixedLlmProvider;
use loom_model_spec::{ModelLimitResolver, ModelsDevResolver};
use loom_types::active_operation::RunCancellation;

use super::super::config::ReactBuildConfig;
use super::super::runner::ReactRunner;
use super::super::REACT_SYSTEM_PROMPT;
use super::checkpointer::{
    build_checkpointer, build_runnable_config,
    resolve_memory_db_path,
};
use super::context::ReactRunContext;
use super::error::BuildRunnerError;
use super::llm::{build_default_provider, resolve_title_provider};
use super::store::build_store;
use super::tool_source::build_tool_source;

pub async fn build_react_run_context(
    config: &ReactBuildConfig,
) -> Result<ReactRunContext, AgentError> {
    let db_path_owned = resolve_memory_db_path(config);
    let db_path = db_path_owned.as_str();

    tracing::debug!("build_react_run_context: checkpointer, store, runnable_config");
    let checkpointer = build_checkpointer(config, db_path)?;
    let store = build_store(config, db_path)?;
    let runnable_config = build_runnable_config(config);
    tracing::debug!("build_react_run_context: building tool_source");
    let tool_source = build_tool_source(config, &store).await?;
    tracing::debug!("build_react_run_context: tool_source ready");

    let audit_log: Option<Arc<dyn loom_llm::support::audit::LlmAuditLog>> =
        loom_llm::support::audit::LlmAuditConfig::from_env()
            .build()
            .map(|log| Arc::new(log) as Arc<dyn loom_llm::support::audit::LlmAuditLog>);

    Ok(ReactRunContext {
        checkpointer,
        store,
        runnable_config,
        tool_source,
        audit_log,
    })
}

async fn resolve_compaction_config(config: &ReactBuildConfig) -> CompactionConfig {
    if let Some(ref cfg) = config.compaction_config {
        return cfg.clone();
    }

    if let Some(ref model) = config.model {
        let resolver = ModelsDevResolver::new();

        if model.contains('/') {
            if let Some(spec) = resolver.resolve_combined(model).await {
                tracing::info!(
                    model = %model,
                    context_limit = spec.context_limit,
                    output_limit = spec.output_limit,
                    "resolved model spec from models.dev"
                );
                return CompactionConfig::with_max_context_tokens(spec.context_limit);
            }
        }

        if let Some(spec) = resolver.resolve_by_bare_model_name(model).await {
            tracing::info!(
                model = %model,
                context_limit = spec.context_limit,
                output_limit = spec.output_limit,
                "resolved model spec from models.dev by bare model name"
            );
            return CompactionConfig::with_max_context_tokens(spec.context_limit);
        }

        tracing::debug!(model = %model, "model not found in models.dev, using default config");
    }

    CompactionConfig::default()
}

pub struct BoxedLlmClient(pub Box<dyn LlmClient>);

#[async_trait::async_trait]
impl LlmClient for BoxedLlmClient {
    async fn invoke(
        &self,
        messages: &[loom_llm::message::Message],
    ) -> Result<loom_llm::LlmResponse, loom_llm::error::AgentError> {
        self.0.invoke(messages).await
    }
    async fn invoke_stream(
        &self,
        messages: &[loom_llm::message::Message],
        tx: Option<tokio::sync::mpsc::Sender<loom_stream::MessageChunk>>,
    ) -> Result<loom_llm::LlmResponse, loom_llm::error::AgentError> {
        self.0.invoke_stream(messages, tx).await
    }
}

pub async fn build_react_runner(
    config: &ReactBuildConfig,
    provider: Option<Arc<dyn LlmProvider>>,
    verbose: bool,
    cancellation: Option<RunCancellation>,
) -> Result<ReactRunner, BuildRunnerError> {
    let ctx = build_react_run_context(config).await?;
    let provider_override = provider.is_some();
    let provider = match provider {
        Some(p) => p,
        None => build_default_provider(config, ctx.tool_source.as_ref(), ctx.audit_log.clone()).await?,
    };
    let system_prompt = config
        .system_prompt
        .clone()
        .unwrap_or_else(|| REACT_SYSTEM_PROMPT.to_string());
    let compaction_config = resolve_compaction_config(config).await;
    let title_provider = if provider_override {
        None
    } else {
        resolve_title_provider(config).await
    };
    let title_headers = config
        .trace_thread_id
        .as_ref()
        .or(config.thread_id.as_ref())
        .map(|tid| loom_llm::LlmHeaders::default().with_thread_id(tid));
    let runner = ReactRunner::new(
        provider,
        ctx.tool_source,
        ctx.checkpointer,
        ctx.store,
        ctx.runnable_config,
        system_prompt,
        Some(compaction_config),
        None,
        cancellation,
        verbose,
        title_provider,
        title_headers,
    )?;
    Ok(runner)
}

pub async fn build_react_runner_with_openai(
    config: &ReactBuildConfig,
    openai_config: async_openai::config::OpenAIConfig,
    model: impl Into<String>,
    verbose: bool,
) -> Result<ReactRunner, BuildRunnerError> {
    use loom_llm::ChatOpenAI;
    let client = ChatOpenAI::with_config(openai_config, model);
    build_react_runner(config, Some(Arc::new(FixedLlmProvider {
        client: Arc::from(Box::new(client) as Box<dyn LlmClient>),
        model_id: "openai".to_string(),
    })), verbose, None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_llm::client::MockLlm;
    use loom_react_config::ReactBuildConfig;

    fn base_config() -> ReactBuildConfig {
        let mut cfg = ReactBuildConfig::from_env();
        cfg.working_folder = Some(std::env::temp_dir());
        cfg
    }

    #[tokio::test(flavor = "current_thread")]
    async fn build_react_run_context_builds_default_tool_source() {
        let ctx = build_react_run_context(&base_config()).await.unwrap();
        assert!(ctx.checkpointer.is_none());
        assert!(ctx.store.is_none());
        assert!(ctx.runnable_config.is_none());
        let tools = ctx.tool_source.list_tools().await.unwrap();
        assert!(!tools.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn exa_codesearch_off_by_default_when_exa_key_set() {
        let mut cfg = base_config();
        cfg.exa_api_key = Some("k".to_string());
        cfg.exa_codesearch_enabled = false;
        let ctx = build_react_run_context(&cfg).await.unwrap();
        let tools = ctx.tool_source.list_tools().await.unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"websearch"));
        assert!(!names.contains(&"codesearch"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn exa_codesearch_registered_when_flag_enabled() {
        let mut cfg = base_config();
        cfg.exa_api_key = Some("k".to_string());
        cfg.exa_codesearch_enabled = true;
        let ctx = build_react_run_context(&cfg).await.unwrap();
        let tools = ctx.tool_source.list_tools().await.unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"websearch"));
        assert!(names.contains(&"codesearch"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn build_react_runner_with_mock_llm_and_system_prompt_streams() {
        let mut cfg = base_config();
        cfg.system_prompt = Some("test system prompt".to_string());
        let runner = build_react_runner(
            &cfg,
            Some(Arc::new(FixedLlmProvider {
                client: Arc::new(MockLlm::with_no_tool_calls("react final")),
                model_id: "mock".to_string(),
            })),
            false,
            None,
        )
        .await
        .unwrap();
        let outcome = runner.stream_with_callback("hello", Some(|_| {} )).await.unwrap();
        match outcome {
            crate::runner_common::StreamRunOutcome::Completed(s) => {
                assert!(s.last_assistant_reply().is_some());
            }
            other => panic!("expected Completed, got {:?}", other),
        }
    }

    // Note: build_dup_tot_got_runners_with_mock_llm_stream test moved to agent-extensions
}
