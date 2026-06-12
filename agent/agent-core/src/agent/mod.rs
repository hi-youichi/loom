//! Agent module: ReAct and other agent implementations.
//!
//! ReAct nodes (Think, Act, Observe), runner, and config-driven builder are unified
//! under [`react`](self::react).
//!
//! # Adding a new agent pattern
//!
//! 1. Add a submodule (e.g. `pub mod my_agent`) with state, runner, and `build_my_agent_initial_state`.
//! 2. In [`react::build`](react/build): add `build_my_agent_runner`, reusing `build_react_run_context` and
//!    `build_checkpointer_for_state::<MyAgentState>` as needed.
//! 3. In the CLI (if used): add a variant to `RunCmd`, a branch in `run::builder::build_runner` that
//!    calls `build_my_agent_runner`, and in `run_agent` a branch that runs and returns the reply.

#[allow(clippy::module_inception)]
mod agent;
pub mod react;

pub use agent::{Agent, AgentConfig, AgentError, AgentEvent, AgentResult};
pub use react::*;