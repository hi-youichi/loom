//! Agent core implementations for Loom.
//!
//! This crate contains the core agent logic including:
//! - ReAct agent with Think, Act, Observe nodes
//! - Common runner infrastructure
//! - Agent tools
//! - Basic run types

pub mod agent;
pub mod runner_common;
pub mod run_types;
pub mod tools;

// Re-export agent types at crate root
pub use agent::{
    build_react_initial_state, ReactBuildConfig, BuildRunnerError, ReactRunContext, build_react_run_context,
    Agent, AgentConfig, AgentError, AgentEvent, AgentResult,
    ReactRunner,
};

// Re-export runner_common
pub use runner_common::{
    load_from_checkpoint_or_build, StreamRunOutcome, StreamRunError, run_stream_with_config,
};

// Re-export from run_types (core types only)
pub use run_types::{
    RunCompletion, RunOptions, AgentRunResult, AgentRunError,
};

// Re-export from loom for RunCancellation
pub use loom_types::active_operation::RunCancellation;
