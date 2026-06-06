//! Loom Tools: Tool implementations and sources for Loom agents
//!
//! This crate provides tool implementations and tool source abstractions used by Loom agents.
//! It includes file tools, bash execution, web fetching, memory tools, MCP integration, and more.

pub mod tool_source;
pub mod tools;
pub mod http_retry;

#[cfg(test)]
mod test_util;

// Re-export commonly used types from dependencies
pub use loom_llm::tool::{ToolSpec, ToolOutputHint, ToolOutputStrategy, ToolSourceError};
pub use loom_llm::message::ToolCallContent;

// Re-export tool_source types
pub use tool_source::{
    BashToolsSource, DryRunToolSource, FileToolSource, FilteredToolSource,
    MemoryToolsSource, MockToolSource, ReadOnlyDirToolSource, ShortTermMemoryToolSource,
    StoreToolSource, TelegramToolsSource, ToolCallContext, ToolSource, WebToolsSource,
    YamlSpecError, YamlSpecToolSource, load_tool_specs,
    TOOL_BASH, TOOL_GET_RECENT_MESSAGES, TOOL_LIST_MEMORIES, TOOL_READ_ONLY_LIST_DIR,
    TOOL_READ_ONLY_READ_FILE, TOOL_RECALL, TOOL_REMEMBER, TOOL_SEARCH_MEMORIES, TOOL_WEB_FETCHER,
};

// Re-export MCP types
pub use tool_source::{McpSession, McpSessionError, McpToolSource};

// Re-export tools registry and adapter
pub use tools::{AggregateToolSource, Tool, ArcTool};

// Re-export commonly used tools at the root for convenience
pub use tools::bash::{BashTool, CommandExecutor, LocalCommandExecutor};
pub use tools::mcp_adapter::{register_mcp_tools, register_mcp_tools_with_specs, McpToolAdapter};

// Re-export shared utilities
pub use tools::shared::canceller::{ChildProcessCanceller, setup_cancellation};
pub use tools::shared::shell_output::{
    ShellOutput, format_shell_output, format_timed_out_output, format_terminal_timed_out_output,
    format_size, shell_output_dir, create_output_file, generate_run_id, make_relative
};

#[cfg(test)]
pub use test_util::env_test_lock;
