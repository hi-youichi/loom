mod backend;
mod event_bridge;
mod json_to_lua;
mod structured_output;
mod tool;
mod workflow_resolver;

pub use backend::LoomAgentBackend;
pub use structured_output::StructuredOutputTool;
pub use tool::LuftTool;
pub use workflow_resolver::resolve_workflow;

use tool_core::ToolRegistryLocked;

pub async fn register_luft_tool(registry: &ToolRegistryLocked, config: agent::agent::AgentConfig) {
    registry
        .register_async(Box::new(LuftTool::new(config)))
        .await;
}
