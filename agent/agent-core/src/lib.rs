//! Agent core implementations for Loom.
//!
//! This crate contains:
//! - ReAct agent pattern (Think, Act, Observe nodes)
//! - DUP/ToT/GoT agent patterns (merged from agent-extensions)
//! - Agent tools (agent, bash, read, edit, etc.)
//! - Run orchestration (config building + runner execution, merged from loom crate)
//! - Profile loading, tier resolution, prompt assembly

pub mod agent;
pub mod agent_cache;
pub mod commands;
pub mod compress;
pub mod goal_runner;
pub mod profile;
pub mod run;
pub mod run_types;
pub mod runner_common;
pub mod runner_error;
pub mod state;
pub mod subagent_display;
pub mod tool_output_normalizer;
pub mod tools;

#[cfg(test)]
mod test_support;

pub use runner_error::RunnerError;

// Agent types
pub use agent::{
    build_react_initial_state, build_react_run_context, Agent, AgentConfig, AgentError, AgentEvent,
    AgentResult, BuildRunnerError, ReactBuildConfig, ReactRunContext, ReactRunner,
};

// Profile + tier + build_config
pub use agent::react::config::{EnvContext, ProjectInfo};
pub use agent::react::tier_apply::{
    extract_provider_hint, resolve_tier_and_build_config,
    resolve_tier_and_build_config_with_resolver,
};
pub use profile::{AgentProfile, ProfileSource, ResolvedAgent};
pub use tools::agent::build_config::{build_config_from_profile, load_agents_md};

// Agent pattern runners (merged from agent-extensions)
pub use agent::dup::{
    build_dup_initial_state, build_dup_runner, DupRunError, DupRunner, DupState, UnderstandOutput,
    DUP_UNDERSTAND_PROMPT,
};
pub use agent::got::{
    build_got_initial_state, build_got_runner, GotRunError, GotRunner, GotState, TaskGraph,
    TaskNode, TaskNodeState, TaskStatus,
};
pub use agent::tot::{
    build_tot_initial_state, build_tot_runner, TotCandidate, TotExtension, TotRunError, TotRunner,
    TotState,
};
pub use agent::{dup, got, tot};

// Run orchestration (merged from loom crate)
pub use run::{
    build_react_config, load_memory_prompt, resolve_model_config, run_agent_from_config,
    AgentRunResult, ResolvedModelConfig, RunCmd, RunCompletion, RunError, RunOptions, RunParams,
    TypedAnyStreamEvent, DEFAULT_WORKING_FOLDER,
};

// Runner common
pub use runner_common::{
    load_from_checkpoint_or_build, resume_from_checkpoint, run_stream_with_config, StreamRunError,
    StreamRunOutcome,
};

// Core run_types (agent-internal, different from CLI RunOptions)
pub use run_types::{
    AgentRunError, AgentRunResult as AgentRunResultCore, RunCompletion as AgentRunCompletion,
    RunOptions as AgentRunOptions,
};

// Cancellation
pub use tool_core::active_operation::RunCancellation;

// Compress (merged from loom-compress)
pub use compress::{build_graph, CompactionConfig, CompressionGraphNode};

// Commands (merged from loom-commands)
pub use commands::{execute, execute_async, parse, Command, CommandResult};
