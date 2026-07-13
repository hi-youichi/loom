//! ReAct graph runner: encapsulates graph build, initial state, and stream.
//!
//! Custom files (error, initial_state, options, runner) remain local.

mod error;
mod initial_state;
mod options;
mod review_coordinator;
#[allow(clippy::module_inception)]
mod runner;

pub use error::RunError;
pub use initial_state::build_react_initial_state;
pub use options::AgentOptions;
pub use review_coordinator::{CoordinatorTrigger, ReviewCoordinator};
pub use runner::{run_react_graph_stream, ReactRunner};
