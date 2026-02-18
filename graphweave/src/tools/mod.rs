mod aggregate_source;
pub mod bash;
mod conversation;
pub mod file;
mod mcp_adapter;
pub mod memory;
mod registry;
mod r#trait;
pub mod todo;
pub mod twitter;
pub mod web;

pub use aggregate_source::AggregateToolSource;
pub use bash::{BashTool, TOOL_BASH};
pub use conversation::{GetRecentMessagesTool, TOOL_GET_RECENT_MESSAGES};
pub use file::{
    CreateDirTool, DeleteFileTool, GlobTool, GrepTool, LsTool, MoveFileTool, ReadFileTool,
    WriteFileTool, TOOL_CREATE_DIR, TOOL_DELETE_FILE, TOOL_GLOB, TOOL_GREP, TOOL_LS,
    TOOL_MOVE_FILE, TOOL_READ_FILE, TOOL_WRITE_FILE,
};
pub use todo::{
    TodoReadTool, TodoWriteTool, TOOL_TODO_READ, TOOL_TODO_WRITE,
};
pub use twitter::{TwitterSearchTool, TOOL_TWITTER_SEARCH};
pub use memory::{
    ListMemoriesTool, RecallTool, RememberTool, SearchMemoriesTool, TOOL_LIST_MEMORIES,
    TOOL_RECALL, TOOL_REMEMBER, TOOL_SEARCH_MEMORIES,
};
pub use r#trait::Tool;
pub use registry::{ToolRegistry, ToolRegistryLocked};
pub use web::{WebFetcherTool, TOOL_WEB_FETCHER};

pub use mcp_adapter::{register_mcp_tools, McpToolAdapter};
