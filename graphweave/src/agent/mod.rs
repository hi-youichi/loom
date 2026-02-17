//! Agent module: ReAct and other agent implementations.
//!
//! ReAct nodes (Think, Act, Observe), runner, and config-driven builder are unified
//! under [`react`](self::react). DUP, ToT, and GoT are under [`dup`](self::dup),
//! [`tot`](self::tot), and [`got`](self::got).
//!
//! # Adding a new agent pattern
//!
//! 1. Add a submodule (e.g. `pub mod my_agent`) with state, runner, and `build_my_agent_initial_state`.
//! 2. In [`react::build`](react/build): add `build_my_agent_runner`, reusing `build_react_run_context` and
//!    `build_checkpointer_for_state::<MyAgentState>` as needed.
//! 3. In the CLI (if used): add a variant to `RunCmd`, a branch in `run::builder::build_runner` that
//!    calls `build_my_agent_runner`, and in `run_agent` a branch that runs and returns the reply.

pub mod dup;
pub mod got;
pub mod react;
pub mod tot;

pub use dup::{build_dup_initial_state, DupRunError, DupRunner, DupState, UnderstandOutput};
pub use got::{
    build_got_initial_state, GotRunError, GotRunner, GotState, TaskGraph, TaskNode, TaskNodeState,
    TaskStatus,
};
pub use react::*;
pub use tot::{
    build_tot_initial_state, TotCandidate, TotExtension, TotRunError, TotRunner, TotState,
};
