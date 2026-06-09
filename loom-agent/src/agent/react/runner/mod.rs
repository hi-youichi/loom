//! ReAct graph runner: encapsulates graph build, initial state, invoke and stream.
//!
//! Identical submodules (error, initial_state) are re-exported from loom_agent_patterns.
//! Custom files (options, runner) remain local.

pub use loom_agent_patterns::agent::react::runner::error;
pub use loom_agent_patterns::agent::react::runner::initial_state;

mod options;
#[allow(clippy::module_inception)]
mod runner;

pub use error::RunError;
pub use initial_state::build_react_initial_state;
pub use options::AgentOptions;
pub use runner::{run_agent, run_react_graph_stream, ReactRunner};