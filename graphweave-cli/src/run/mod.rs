//! Run orchestration: delegates to graphweave::run_agent with stderr display callback.

mod agent;
mod display;

pub use agent::{run_agent_wrapper, RunAgentResult};
pub use graphweave::{build_helve_config, RunCmd, RunError, RunOptions};
