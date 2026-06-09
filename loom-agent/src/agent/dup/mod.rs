//! DUP (Deeply Understanding Problems) graph and runner.
//!
//! Adds an Understand node before the plan/act/observe loop.
//!
//! Identical submodules are re-exported from loom_agent_patterns.
//! Custom files (adapter_nodes, runner, state) remain local.

pub use loom_agent_patterns::agent::dup::prompt;
pub use loom_agent_patterns::agent::dup::understand_node;

mod adapter_nodes;
mod runner;
mod state;

pub use prompt::DUP_UNDERSTAND_PROMPT;
pub use runner::{build_dup_initial_state, DupRunError, DupRunner};
pub use state::{DupState, UnderstandOutput};
pub use understand_node::UnderstandNode;