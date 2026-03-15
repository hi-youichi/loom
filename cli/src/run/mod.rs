//! Run orchestration: delegates to loom::run_agent_with_options with stderr display callback.

mod agent;
mod display;

pub use agent::{print_reply_timestamp, run_agent_wrapper, RunAgentResult};
pub use loom::{build_helve_config, RunCmd, RunError, RunOptions};
