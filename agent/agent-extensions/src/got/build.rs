//! GotRunner builder — extracted from react/build/runners.rs for co-location.

use std::sync::Arc;

use loom_llm::LlmClient;
use loom_react_config::ReactBuildConfig;

use crate::got::{GotRunner, GotState};
use agent::agent::react::build::{
    build_react_run_context, build_default_llm_with_tool_source, resolve_memory_db_path,
    build_checkpointer_for_state, BuildRunnerError,
    BoxedLlmClient,
};

pub async fn build_got_runner(
    config: &ReactBuildConfig,
    llm: Option<Box<dyn LlmClient>>,
    verbose: bool,
) -> Result<GotRunner, BuildRunnerError> {
    let ctx = build_react_run_context(config).await?;
    let llm = match llm {
        Some(l) => l,
        None => build_default_llm_with_tool_source(config, ctx.tool_source.as_ref(), ctx.audit_log.clone()).await?,
    };
    let llm_arc: Arc<dyn LlmClient> = Arc::new(BoxedLlmClient(llm));

    let db_path_owned = resolve_memory_db_path(config);
    let db_path = db_path_owned.as_str();
    let got_checkpointer = build_checkpointer_for_state::<GotState>(config, db_path)?;

    let got = &config.got_config;
    let runner = GotRunner::new(
        llm_arc,
        ctx.tool_source,
        got_checkpointer,
        ctx.store,
        ctx.runnable_config,
        None,
        verbose,
        got.adaptive,
        got.agot_llm_complexity,
    )?;
    Ok(runner)
}
