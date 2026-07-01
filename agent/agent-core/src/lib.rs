//! Agent core implementations for Loom.
//!
//! This crate contains the core agent logic including:
//! - ReAct agent with Think, Act, Observe nodes
//! - Common runner infrastructure
//! - Agent tools
//! - Basic run types

pub mod agent;
pub mod goal_runner;
pub mod profile;
pub mod runner_common;
pub mod runner_error;
pub mod run_types;
pub mod tool_output_normalizer;
pub mod tools;

pub use runner_error::RunnerError;

// Re-export agent types at crate root
pub use agent::{
    build_react_initial_state, ReactBuildConfig, BuildRunnerError, ReactRunContext, build_react_run_context,
    Agent, AgentConfig, AgentError, AgentEvent, AgentResult,
    ReactRunner,
};

// Profile + tier + build_config (migrated from loom-react-config)
pub use profile::{AgentProfile, ProfileSource, ResolvedAgent};
pub use agent::react::config::{EnvContext, ProjectInfo};
pub use agent::react::tier_apply::{
    extract_provider_hint, resolve_tier_and_build_config,
    resolve_tier_and_build_config_with_resolver,
};
pub use tools::invoke_agent::build_config::{build_config_from_profile, load_agents_md};

// Re-export runner_common
pub use runner_common::{
    load_from_checkpoint_or_build, StreamRunOutcome, StreamRunError, run_stream_with_config,
};

// Re-export from run_types (core types only)
pub use run_types::{
    RunCompletion, RunOptions, AgentRunResult, AgentRunError,
};

// Re-export from loom for RunCancellation
pub use tool_core::active_operation::RunCancellation;
