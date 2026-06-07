//! Agent implementations for Loom.
//!
//! This crate contains the agent logic extracted from loom, including:
//! - ReAct agent with Think, Act, Observe nodes
//! - DUP (Debate Understood Protocol) agent
//! - ToT (Tree of Thoughts) agent
//! - GoT (Graph of Thoughts) agent
//!
//! Agent orchestration and run orchestration functions are also included.
//!
//! The agent pattern implementations are now provided by loom-agent-patterns.

pub mod cli_run_agent;
pub mod tools;

// Re-export agent patterns from loom-agent-patterns
pub use loom_agent_patterns::agent;

// Re-export from runner_common (use loom-agent-patterns version)
pub use loom_agent_patterns::runner_common::{
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

// Re-export from agent::react::build (via loom-agent-patterns)
pub use loom_agent_patterns::{build_react_run_context, BuildRunnerError};
// goal_runner types remain in the loom crate; loom-agent does not re-export them.