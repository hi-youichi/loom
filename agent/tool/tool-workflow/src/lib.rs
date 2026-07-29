mod backend;
mod common;
mod event_bridge;
mod instance;
mod runtime;
mod service;
mod structured_output;
mod tool_events;
mod tool_files;
mod tool_list;
mod tool_source;
mod tool_start;
mod tool_status;
mod tool_cancel;
mod workflow_resolver;

pub use backend::LoomAgentBackend;
pub use instance::{
    build_instance_meta, write_instance_artifacts, AgentSummary, EventStats, InstanceMeta,
    PhaseSpan, ReportRef, WorkflowRef,
};
pub use structured_output::StructuredOutputTool;
pub use tool_events::WorkflowEventsTool;
pub use tool_files::WorkflowFilesTool;
pub use tool_list::WorkflowListTool;
pub use tool_source::WorkflowSourceTool;
pub use tool_start::WorkflowStartTool;
pub use tool_status::WorkflowStatusTool;
pub use tool_cancel::WorkflowCancelTool;
pub use workflow_resolver::resolve_workflow;

pub use runtime::WorkflowRuntime;

use std::sync::Arc;
use tool_core::{Tool, ToolRegistryLocked};

pub async fn register_workflow_tools(
    registry: &ToolRegistryLocked,
    config: agent::agent::AgentConfig,
) {
    let runtime = Arc::new(WorkflowRuntime::new(config));
    registry
        .register_async(Box::new(WorkflowStartTool::new(runtime.clone())))
        .await;
    registry
        .register_async(Box::new(WorkflowCancelTool::new(runtime.clone())))
        .await;
    registry
        .register_async(Box::new(WorkflowStatusTool::new(runtime.clone())))
        .await;
    registry
        .register_async(Box::new(WorkflowListTool::new(runtime.clone())))
        .await;
    registry
        .register_async(Box::new(WorkflowEventsTool::new(runtime.clone())))
        .await;
    registry
        .register_async(Box::new(WorkflowSourceTool::new(runtime.clone())))
        .await;
    registry
        .register_async(Box::new(WorkflowFilesTool::new(runtime)))
        .await;
}

pub fn default_workflow_tool_provider() -> agent::run::ExtraToolsProvider {
    Arc::new(|config: &agent::ReactBuildConfig| {
        let runtime = Arc::new(WorkflowRuntime::new(config_to_agent_config(config)));
        vec![
            Arc::new(WorkflowStartTool::new(runtime.clone())) as Arc<dyn Tool>,
            Arc::new(WorkflowCancelTool::new(runtime.clone())) as Arc<dyn Tool>,
            Arc::new(WorkflowStatusTool::new(runtime.clone())) as Arc<dyn Tool>,
            Arc::new(WorkflowListTool::new(runtime.clone())) as Arc<dyn Tool>,
            Arc::new(WorkflowEventsTool::new(runtime.clone())) as Arc<dyn Tool>,
            Arc::new(WorkflowSourceTool::new(runtime.clone())) as Arc<dyn Tool>,
            Arc::new(WorkflowFilesTool::new(runtime.clone())) as Arc<dyn Tool>,
        ]
    })
}

fn config_to_agent_config(config: &agent::ReactBuildConfig) -> agent::agent::AgentConfig {
    config.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::agent::AgentConfig;
    use tool_core::tool_name::{
        TOOL_WORKFLOW_CANCEL, TOOL_WORKFLOW_EVENTS, TOOL_WORKFLOW_FILES, TOOL_WORKFLOW_LIST,
        TOOL_WORKFLOW_SOURCE, TOOL_WORKFLOW_START, TOOL_WORKFLOW_STATUS,
    };
    use tool_core::ToolOutputStrategy;

    #[test]
    fn seven_tool_names_and_specs_match_constants() {
        let cfg = AgentConfig::default();
        let runtime = Arc::new(WorkflowRuntime::new(cfg));
        let start = WorkflowStartTool::new(runtime.clone());
        let cancel = WorkflowCancelTool::new(runtime.clone());
        let status = WorkflowStatusTool::new(runtime.clone());
        let list = WorkflowListTool::new(runtime.clone());
        let events = WorkflowEventsTool::new(runtime.clone());
        let source = WorkflowSourceTool::new(runtime.clone());
        let files = WorkflowFilesTool::new(runtime);

        assert_eq!(start.name(), TOOL_WORKFLOW_START);
        assert_eq!(cancel.name(), TOOL_WORKFLOW_CANCEL);
        assert_eq!(status.name(), TOOL_WORKFLOW_STATUS);
        assert_eq!(list.name(), TOOL_WORKFLOW_LIST);
        assert_eq!(events.name(), TOOL_WORKFLOW_EVENTS);
        assert_eq!(source.name(), TOOL_WORKFLOW_SOURCE);
        assert_eq!(files.name(), TOOL_WORKFLOW_FILES);

        assert_eq!(start.spec().name, TOOL_WORKFLOW_START);
        assert_eq!(cancel.spec().name, TOOL_WORKFLOW_CANCEL);
        assert_eq!(status.spec().name, TOOL_WORKFLOW_STATUS);
        assert_eq!(list.spec().name, TOOL_WORKFLOW_LIST);
        assert_eq!(events.spec().name, TOOL_WORKFLOW_EVENTS);
        assert_eq!(source.spec().name, TOOL_WORKFLOW_SOURCE);
        assert_eq!(files.spec().name, TOOL_WORKFLOW_FILES);

        for tool in [
            &start as &dyn Tool,
            &cancel,
            &status,
            &list,
            &events,
            &source,
            &files,
        ] {
            let hint = tool
                .spec()
                .output_hint
                .as_ref()
                .and_then(|h| h.preferred_strategy)
                .expect("output_hint must be set");
            assert!(matches!(hint, ToolOutputStrategy::Inline));
        }
    }

    #[test]
    fn runtime_paths_match_loom_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = AgentConfig {
            working_folder: Some(tmp.path().to_path_buf()),
            ..Default::default()
        };
        let rt = Arc::new(crate::runtime::WorkflowRuntime::new(cfg));
        assert_eq!(rt.instances_root(), tmp.path().join(".loom").join("instances"));
        assert_eq!(rt.runs_root(), tmp.path().join(".luft").join("runs"));
        assert_eq!(
            rt.workflows_dir(),
            tmp.path().join(".loom").join("workflows")
        );
        assert_eq!(
            rt.loom_instance_dir("loom-instance_abc"),
            tmp.path()
                .join(".loom")
                .join("instances")
                .join("loom-instance_abc")
        );
    }
}
