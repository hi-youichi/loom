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
pub use tool::{
    WorkflowEventsTool, WorkflowFilesTool, WorkflowListTool, WorkflowSourceTool, WorkflowStartTool,
    WorkflowStatusTool,
};
pub use workflow_resolver::resolve_workflow;

use std::sync::Arc;
use tool_core::{Tool, ToolRegistryLocked};

pub async fn register_workflow_tools(
    registry: &ToolRegistryLocked,
    config: agent::agent::AgentConfig,
) {
    registry
        .register_async(Box::new(WorkflowStartTool::new(config.clone())))
        .await;
    registry
        .register_async(Box::new(WorkflowStatusTool::new(config.clone())))
        .await;
    registry
        .register_async(Box::new(WorkflowListTool::new(config.clone())))
        .await;
    registry
        .register_async(Box::new(WorkflowEventsTool::new(config.clone())))
        .await;
    registry
        .register_async(Box::new(WorkflowSourceTool::new(config.clone())))
        .await;
    registry
        .register_async(Box::new(WorkflowFilesTool::new(config)))
        .await;
}

pub fn default_workflow_tool_provider() -> agent::run::ExtraToolsProvider {
    Arc::new(|config: &agent::ReactBuildConfig| {
        let cfg = config.clone();
        vec![
            Arc::new(WorkflowStartTool::new(cfg.clone())) as Arc<dyn Tool>,
            Arc::new(WorkflowStatusTool::new(cfg.clone())) as Arc<dyn Tool>,
            Arc::new(WorkflowListTool::new(cfg.clone())) as Arc<dyn Tool>,
            Arc::new(WorkflowEventsTool::new(cfg.clone())) as Arc<dyn Tool>,
            Arc::new(WorkflowSourceTool::new(cfg.clone())) as Arc<dyn Tool>,
            Arc::new(WorkflowFilesTool::new(cfg)) as Arc<dyn Tool>,
        ]
    })
}
