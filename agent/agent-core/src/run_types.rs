//! Public run types for agent execution.
//!
//! These types are shared across all agent patterns and consumers.

// Re-export cancellation types from anureo
pub use tool_core::active_operation::{
    ActiveOperation, ActiveOperationCanceller, ActiveOperationKind, RunCancellation,
};

/// Final result of a single agent run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentRunResult {
    pub reply: String,
    pub reasoning_content: Option<String>,
}

/// Error type for agent run result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunError(pub String);

impl std::fmt::Display for AgentRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Final completion state of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunCompletion {
    Finished(AgentRunResult),
    Cancelled,
    Error(AgentRunError),
}

/// Options for running the Helve agent.
#[derive(Clone)]
pub struct RunOptions {
    pub message: anureo_llm::message::UserContent,
    pub working_folder: Option<std::path::PathBuf>,
    pub session_id: Option<String>,
    pub agent: Option<String>,
    pub verbose: bool,
    pub got_adaptive: bool,
    pub display_max_len: usize,
    pub output_json: bool,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub provider_type: Option<String>,
    pub mcp_config_path: Option<std::path::PathBuf>,
    pub cancellation: Option<RunCancellation>,
    pub thread_id: Option<String>,
    pub output_timestamp: bool,
    pub dry_run: bool,
    pub debug_llm: bool,
    pub any_stream_event_sender:
        Option<std::sync::Arc<dyn Fn(crate::run::TypedAnyStreamEvent) + Send + Sync>>,
    pub bash_executor: Option<std::sync::Arc<dyn tool_basic::bash::CommandExecutor>>,
    pub extra_tools: Option<std::sync::Arc<Vec<std::sync::Arc<dyn tool_core::Tool>>>>,
    pub acp_session_id: Option<String>,
    pub force_compact: bool,
    pub chat_id: Option<i64>,
    pub worktree: bool,
    /// When true, task management tools are registered. Only in goal mode.
    pub goal_mode: bool,
    /// MCP servers from ACP session/new request, converted to anureo's internal type.
    /// Merged into build config alongside mcp.json servers.
    pub acp_mcp_servers: Option<Vec<env_config::McpServerDef>>,
    /// Reasoning effort override.
    pub effort: Option<String>,
}

impl std::fmt::Debug for RunOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunOptions")
            .field("message", &self.message)
            .field("working_folder", &self.working_folder)
            .field("session_id", &self.session_id)
            .field("agent", &self.agent)
            .field("verbose", &self.verbose)
            .finish()
    }
}
