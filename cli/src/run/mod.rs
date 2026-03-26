//! Run orchestration: delegates to loom::run_agent_with_options with stderr display callback.

mod agent;
mod contract;
mod display;

pub use agent::{
    print_reply_timestamp, run_agent_wrapper, RunAgentOutput, RunAgentResult, RunStopReason,
};
pub use contract::{
    cli_list_models, cli_list_tools, cli_show_tool, run_cli_turn, RunOutput, StreamOut,
};
pub use loom::{build_helve_config, RunCmd, RunError, RunOptions};
