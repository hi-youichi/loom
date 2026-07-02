//! Agent core implementations for Loom.
//!
//! This crate contains:
//! - ReAct agent pattern (Think, Act, Observe nodes)
//! - DUP/ToT/GoT agent patterns (merged from agent-extensions)
//! - Agent tools (invoke_agent, bash, read, edit, etc.)
//! - Run orchestration (config building + runner execution, merged from loom crate)
//! - Profile loading, tier resolution, prompt assembly

pub mod agent;
pub mod commands;
pub mod compress;
pub mod goal_runner;
pub mod profile;
pub mod run;
pub mod runner_common;
pub mod runner_error;
pub mod run_types;
pub mod state;
pub mod subagent_display;
pub mod tool_output_normalizer;
pub mod tools;

pub use runner_error::RunnerError;

// Agent types
pub use agent::{
    build_react_initial_state, ReactBuildConfig, BuildRunnerError, ReactRunContext, build_react_run_context,
    Agent, AgentConfig, AgentError, AgentEvent, AgentResult,
    ReactRunner,
};

// Profile + tier + build_config
pub use profile::{AgentProfile, ProfileSource, ResolvedAgent};
pub use agent::react::config::{EnvContext, ProjectInfo};
pub use agent::react::tier_apply::{
    extract_provider_hint, resolve_tier_and_build_config,
    resolve_tier_and_build_config_with_resolver,
};
pub use tools::invoke_agent::build_config::{build_config_from_profile, load_agents_md};

// Agent pattern runners (merged from agent-extensions)
pub use agent::{dup, tot, got};
pub use agent::dup::{
    DupRunner, DupState, DupRunError,
    build_dup_runner, build_dup_initial_state,
    UnderstandOutput, DUP_UNDERSTAND_PROMPT,
};
pub use agent::got::{
    GotRunner, GotState, GotRunError,
    build_got_runner, build_got_initial_state,
    TaskGraph, TaskNode, TaskNodeState, TaskStatus,
};
pub use agent::tot::{
    TotRunner, TotState, TotRunError,
    build_tot_runner, build_tot_initial_state,
    TotCandidate, TotExtension,
};

// Run orchestration (merged from loom crate)
pub use run::{
    build_react_config, resolve_model_config, load_memory_prompt,
    AgentRunResult, DEFAULT_WORKING_FOLDER, ResolvedModelConfig,
    RunCmd, RunCompletion, RunError, RunOptions,
    RunParams, TypedAnyStreamEvent,
    run_agent_from_config, run_agent_from_config_traced,
};

// Runner common
pub use runner_common::{
    load_from_checkpoint_or_build, StreamRunOutcome, StreamRunError, run_stream_with_config,
};

// Core run_types (agent-internal, different from CLI RunOptions)
pub use run_types::{
    RunCompletion as AgentRunCompletion, RunOptions as AgentRunOptions,
    AgentRunResult as AgentRunResultCore, AgentRunError,
};

// Cancellation
pub use tool_core::active_operation::RunCancellation;

// Compress (merged from loom-compress)
pub use compress::{CompactionConfig, CompressionGraphNode, build_graph};

// Commands (merged from loom-commands)
pub use commands::{Command, CommandResult, execute, execute_async, parse};
