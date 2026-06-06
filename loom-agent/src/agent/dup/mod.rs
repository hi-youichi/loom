//! DUP (Deeply Understanding Problems) graph and runner.
//!
//! Adds an Understand node before the plan/act/observe loop.

mod adapter_nodes;
mod prompt;
mod runner;
mod state;
mod understand_node;

pub use prompt::DUP_UNDERSTAND_PROMPT;
pub use runner::{build_dup_initial_state, DupRunError, DupRunner};
pub use state::{DupState, UnderstandOutput};
pub use understand_node::UnderstandNode;
