//! Bash tools registration: run shell commands as one tool (`bash`).

use tool_core::{ToolRegistryLocked};
use crate::BashTool;

/// Tool name: run a shell command.
pub use loom_types::tools::tool_name::TOOL_BASH;

/// Register bash tools with the given registry.
///
/// This function registers the [`BashTool`] with the provided tool registry.
///
/// # Examples
///
/// ```no_run
/// use tool_basic::register_bash_tools;
/// use tool_core::ToolRegistryLocked;
/// # #[tokio::main]
/// # async fn main() {
/// let registry = ToolRegistryLocked::new();
/// register_bash_tools(&registry).await;
/// # }
/// ```
pub async fn register_bash_tools(registry: &ToolRegistryLocked) {
    registry.register_async(Box::new(BashTool::new())).await;
}