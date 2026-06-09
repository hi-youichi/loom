//! Agent implementations for Loom.
//!
//! This crate contains the agent logic extracted from loom, including:
//! - ReAct agent with Think, Act, Observe nodes
//! - DUP (Debate Understood Protocol) agent
//! - ToT (Tree of Thoughts) agent
//! - GoT (Graph of Thoughts) agent
//!
//! Agent orchestration and run orchestration functions are also included.

pub mod agent;
pub mod cli_run_agent;
pub mod runner_common;
pub mod tools;

// Re-export agent types at crate root
pub use agent::{
    build_dup_initial_state, build_got_initial_state, build_tot_initial_state, build_react_initial_state,
    DupRunError, DupRunner, DupState, GotRunError, GotRunner, GotState,
    ReactBuildConfig, BuildRunnerError, ReactRunContext, build_react_run_context,
    TotRunError, TotRunner, TotState, UnderstandOutput,
};

// Re-export runner_common
pub use runner_common::{
    load_from_checkpoint_or_build, StreamRunOutcome, StreamRunError, run_stream_with_config,
};

// Re-export from cli_run_agent
pub use cli_run_agent::{
    AnyRunner, AnyStreamEvent, RunCmd, RunCompletion, RunError, RunOptions,
    run_agent, run_agent_with_options, run_agent_with_llm_override,
    build_runner, resolve_tier_and_build_config, resolve_tier_and_build_config_with_resolver,
    to_loom_any_stream_event,
};

// Re-export from loom for RunCancellation
pub use loom_types::active_operation::RunCancellation;