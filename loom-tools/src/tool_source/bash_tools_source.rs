//! Bash tools source: run shell commands as one tool (`bash`).
//!
//! Uses `AggregateToolSource` internally to register [`BashTool`](crate::tools::BashTool).

use async_trait::async_trait;

use crate::tool_source::{ToolSource, ToolSourceError};
use crate::tools::{AggregateToolSource, BashTool};

/// Tool name: run a shell command.
pub const TOOL_BASH: &str = "bash";

/// Tool source that exposes bash (shell) execution as one tool: `bash`.
///
/// Uses [`AggregateToolSource`] internally to register [`BashTool`].
/// Provides a convenient way to enable shell command execution in agents.
pub struct BashToolsSource {
    _source: AggregateToolSource,
}

impl BashToolsSource {
    /// Creates a bash tools source.
    ///
    /// Returns an [`AggregateToolSource`] that you can use with [`ActNode`](crate::agent::react::ActNode).
    /// This function is async and must be awaited.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use loom_tools::tool_source::BashToolsSource;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let source = BashToolsSource::new().await;
    /// # }
    /// ```
    #[allow(clippy::new_ret_no_self)]
    pub async fn new() -> AggregateToolSource {
        let source = AggregateToolSource::new();
        source.register_async(Box::new(BashTool::new())).await;
        source
    }
}

#[async_trait]
impl ToolSource for BashToolsSource {
    /// Lists all registered tools.
    ///
    /// Delegates to [`AggregateToolSource::list_tools`].
    async fn list_tools(&self) -> Result<Vec<crate::tool_source::ToolSpec>, ToolSourceError> {
        self._source.list_tools().await
    }

    /// Calls a tool by name with given arguments.
    ///
    /// Delegates to [`AggregateToolSource::call_tool`].
    /// Use tool name `"bash"` with arguments `{ "command": "..." }`.
    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
        _ctx: Option<&crate::tool_source::ToolCallContext>,
    ) -> Result<crate::tool_source::ToolCallContent, ToolSourceError> {
        self._source.call_tool(name, arguments, _ctx).await
    }
}
