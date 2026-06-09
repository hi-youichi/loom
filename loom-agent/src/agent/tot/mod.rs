//! ToT (Tree of Thoughts) graph and runner.
//!
//! Adds multi-candidate expansion and evaluation before Act.
//!
//! Identical submodules are re-exported from loom_agent_patterns.
//! Custom files (adapter_nodes, backtrack_node, evaluate_node, expand_node, runner, state) remain local.

pub use loom_agent_patterns::agent::tot::prompt;

mod adapter_nodes;
mod backtrack_node;
mod evaluate_node;
mod expand_node;
mod runner;
mod state;

pub use backtrack_node::BacktrackNode;
pub use evaluate_node::ThinkEvaluateNode;
pub use expand_node::ThinkExpandNode;
pub use prompt::{TOT_EXPAND_SYSTEM_ADDON, TOT_RESEARCH_QUALITY_ADDON};
pub use runner::{build_tot_initial_state, TotRunError, TotRunner};
pub use state::{TotCandidate, TotExtension, TotState};