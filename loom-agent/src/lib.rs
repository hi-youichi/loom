//! Agent facade crate for Loom.
//!
//! This crate re-exports everything from `agent` (core) and `agent-extensions`
//! to maintain backward compatibility with existing consumers while the
//! codebase is being restructured.
//!
//! New code should depend on `agent` and/or `agent-extensions` directly.

// Re-export extension types as `agent` module for backward compatibility
// Consumers use `loom_agent::agent::DupState`, `loom_agent::agent::TotState`, etc.
pub mod agent {
    // Re-export react types from agent-core (use ::agent for the external crate)
    pub use ::agent::{
        Agent, AgentConfig, AgentError, AgentEvent, AgentResult,
        build_react_initial_state, ReactBuildConfig, BuildRunnerError, ReactRunContext,
        build_react_run_context, ReactRunner,
    };

    // Re-export extension types
    pub use agent_extensions::{
        // DUP
        DupRunner, DupState, DupRunError,
        build_dup_runner, build_dup_initial_state,
        UnderstandOutput, DUP_UNDERSTAND_PROMPT,
        // GoT
        GotRunner, GotState, GotRunError,
        build_got_runner, build_got_initial_state,
        TaskGraph, TaskNode, TaskNodeState, TaskStatus,
        // ToT
        TotRunner, TotState, TotRunError,
        build_tot_runner, build_tot_initial_state,
        TotCandidate, TotExtension,
    };
}

// Re-export at crate root (flat namespace) — use ::agent to reference the external crate
pub use ::agent::{
    // Agent types
    Agent, AgentConfig, AgentError, AgentEvent, AgentResult,
    // React
    build_react_initial_state, ReactBuildConfig, BuildRunnerError, ReactRunContext,
    build_react_run_context, ReactRunner,
    // Runner common
    load_from_checkpoint_or_build, StreamRunOutcome, StreamRunError, run_stream_with_config,
    // Run types
    RunCompletion, RunOptions, AgentRunResult, AgentRunError,
    // Cancellation
    RunCancellation,
};

pub use agent_extensions::{
    // DUP
    DupRunner, DupState, DupRunError,
    build_dup_runner, build_dup_initial_state,
    UnderstandOutput, DUP_UNDERSTAND_PROMPT,
    // GoT
    GotRunner, GotState, GotRunError,
    build_got_runner, build_got_initial_state,
    TaskGraph, TaskNode, TaskNodeState, TaskStatus,
    // ToT
    TotRunner, TotState, TotRunError,
    build_tot_runner, build_tot_initial_state,
    TotCandidate, TotExtension,
};

// Re-export runner_common module itself for direct access
pub mod runner_common {
    pub use ::agent::runner_common::*;
}

// Re-export cli_run_agent (full dispatch with all agent patterns)
pub mod cli_run_agent;

// Re-export cli_run_agent types at crate root for backward compatibility
pub use cli_run_agent::{
    AnyRunner, AnyStreamEvent, RunCmd, RunError,
    run_agent, run_agent_with_options, run_agent_with_llm_override,
    build_runner, resolve_tier_and_build_config, resolve_tier_and_build_config_with_resolver,
    to_loom_any_stream_event,
};
