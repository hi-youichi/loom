mod backend;
mod event_bridge;
mod instance;
mod json_to_lua;
mod structured_output;
mod tool;
mod workflow_resolver;

pub use backend::LoomAgentBackend;
pub use instance::{
    build_instance_meta, write_instance_artifacts, AgentSummary, EventStats, InstanceMeta,
    PhaseSpan, ReportRef, WorkflowRef,
};
pub use structured_output::StructuredOutputTool;
pub use tool::WorkflowTool;
pub use workflow_resolver::resolve_workflow;

use std::sync::Arc;
use tool_core::Tool;

use tool_core::ToolRegistryLocked;

pub async fn register_workflow_tool(
    registry: &ToolRegistryLocked,
    config: agent::agent::AgentConfig,
) {
    registry
        .register_async(Box::new(WorkflowTool::new(config)))
        .await;
}

/// Provider for the workflow tool as a "default" extra tool that Loom
/// registers with every agent invocation.
///
/// Wire this into `RunOptions::default_extra_tools_provider` so
/// `build_react_config` registers the tool *before* assembling the skill
/// registry — the only way the workflow tool's `workflow` builtin skill
/// lands in the agent's `SkillRegistry`. Putting the provider on the tool
/// crate (rather than the `apps/cli` crate) lets every Loom front-end
/// (`cli`, `acp`, `telegram-bot`, …) reuse the same wiring without depending
/// on the CLI binary.
pub fn default_workflow_tool_provider() -> agent::run::ExtraToolsProvider {
    Arc::new(|config: &agent::ReactBuildConfig| {
        vec![Arc::new(WorkflowTool::new(config.clone())) as Arc<dyn Tool>]
    })
}
