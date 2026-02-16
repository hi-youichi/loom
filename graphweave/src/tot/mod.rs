//! ToT (Tree of Thoughts) graph and runner.
//!
//! Adds multi-candidate expansion and evaluation before Act.

mod adapter_nodes;
mod backtrack_node;
mod evaluate_node;
mod expand_node;
mod prompt;
mod runner;
mod state;

pub use backtrack_node::BacktrackNode;
pub use evaluate_node::ThinkEvaluateNode;
pub use expand_node::ThinkExpandNode;
pub use prompt::{TOT_EXPAND_SYSTEM_ADDON, TOT_RESEARCH_QUALITY_ADDON};
pub use runner::{build_tot_initial_state, TotRunError, TotRunner};
pub use state::{TotCandidate, TotExtension, TotState};
