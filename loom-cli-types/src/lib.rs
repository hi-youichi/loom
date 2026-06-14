//! CLI run types for Loom agent patterns.
//!
//! Contains the core types used across CLI, serve, and agent crates for
//! configuring and running agents: `RunOptions`, `AnyStreamEvent`, `RunCmd`,
//! `RunCompletion`, `RunError`, `AgentRunResult`, `ResolvedAgent`, `ResolvedModelConfig`.
//!
//! These types were extracted from `loom::cli_run` to allow downstream crates
//! (cli, serve, loom-acp, loom-agent) to depend on them directly without
//! pulling in the full `loom` facade.

pub mod goal_runner;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

// Re-export active operation types from loom-types for convenience
pub use loom_types::active_operation::{
    ActiveOperation, ActiveOperationCanceller, ActiveOperationKind, RunCancellation,
};
// Re-export stub agent state types from loom-types
pub use loom_types::cli_run::{StubDupState, StubGotState, StubTotState};
// Re-export ReActState
pub use loom_types::state::ReActState;

/// Default working folder when not set (current directory).
pub const DEFAULT_WORKING_FOLDER: &str = ".";

/// Metadata about the agent profile that was resolved for a run.
#[derive(Debug, Clone)]
pub struct ResolvedAgent {
    pub name: String,
    pub description: Option<String>,
    pub source: loom_react_config::profile::ProfileSource,
}

/// Resolved model + provider configuration from a model string like "openai/gpt-4o".
#[derive(Debug, Clone, Default)]
pub struct ResolvedModelConfig {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub provider_type: Option<String>,
}

/// Options for running an agent.
pub struct RunOptions {
    pub message: loom_llm::message::UserContent,
    pub working_folder: Option<PathBuf>,
    pub session_id: Option<String>,
    pub cancellation: Option<RunCancellation>,
    pub thread_id: Option<String>,
    pub agent: Option<String>,
    pub verbose: bool,
    pub got_adaptive: bool,
    pub display_max_len: usize,
    pub output_json: bool,
    pub model: Option<String>,
    pub mcp_config_path: Option<PathBuf>,
    pub output_timestamp: bool,
    pub dry_run: bool,
    pub debug_llm: bool,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub provider_type: Option<String>,
    pub any_stream_event_sender: Option<Arc<dyn Fn(AnyStreamEvent) + Send + Sync>>,
    pub bash_executor: Option<Arc<dyn tool_basic::bash::CommandExecutor>>,
    pub extra_tools: Option<Arc<Vec<Arc<dyn tool_core::Tool>>>>,
    pub acp_session_id: Option<String>,
    pub force_compact: bool,
    pub chat_id: Option<i64>,
    pub worktree: bool,
}

impl std::fmt::Debug for RunOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunOptions")
            .field("message", &"<UserContent>")
            .field("working_folder", &self.working_folder)
            .field("model", &self.model)
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("thread_id", &self.thread_id)
            .field("dry_run", &self.dry_run)
            .finish_non_exhaustive()
    }
}

impl Clone for RunOptions {
    fn clone(&self) -> Self {
        Self {
            message: self.message.clone(),
            working_folder: self.working_folder.clone(),
            session_id: self.session_id.clone(),
            cancellation: self.cancellation.clone(),
            thread_id: self.thread_id.clone(),
            agent: self.agent.clone(),
            verbose: self.verbose,
            got_adaptive: self.got_adaptive,
            display_max_len: self.display_max_len,
            output_json: self.output_json,
            model: self.model.clone(),
            mcp_config_path: self.mcp_config_path.clone(),
            output_timestamp: self.output_timestamp,
            dry_run: self.dry_run,
            debug_llm: self.debug_llm,
            provider: self.provider.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            provider_type: self.provider_type.clone(),
            any_stream_event_sender: self.any_stream_event_sender.clone(),
            bash_executor: self.bash_executor.clone(),
            extra_tools: self.extra_tools.clone(),
            acp_session_id: self.acp_session_id.clone(),
            force_compact: self.force_compact,
            chat_id: self.chat_id,
            worktree: self.worktree,
        }
    }
}

/// Which agent pattern to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunCmd {
    React,
    Dup,
    Tot,
    Got,
}

/// Result of a completed agent run.
#[derive(Debug, Clone)]
pub enum RunCompletion {
    Finished(AgentRunResult),
    Cancelled,
}

/// Error from an agent run.
#[derive(Debug, Clone)]
pub enum RunError {
    LlmError(String),
    ToolError(String),
    Other(String),
}

/// Union type for stream events from different agent patterns.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AnyStreamEvent {
    React(loom_stream::StreamEvent<ReActState>),
    Dup(loom_stream::StreamEvent<StubDupState>),
    Tot(loom_stream::StreamEvent<StubTotState>),
    Got(loom_stream::StreamEvent<StubGotState>),
}

/// Result of a successfully completed agent run.
#[derive(Debug, Clone)]
pub struct AgentRunResult {
    pub reply: String,
    pub reasoning_content: Option<String>,
}
