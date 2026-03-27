//! Loom CLI library: CLI parsing and run orchestration for the Loom agents.
//!
//! Used by the `loom` binary. Builds a [`ReactRunner`](loom::ReactRunner) from
//! config (env, working folder, etc.) and runs or streams the graph.

pub mod envelope;
pub mod log_format;
pub mod model_cmd;
pub mod run;
pub mod tool_cmd;

pub use loom::Envelope;
pub use model_cmd::{list_all_models, list_provider_models};
pub use run::{
    cli_list_models, cli_list_tools, cli_show_tool, print_reply_timestamp,
    run_agent_wrapper as run_agent, run_cli_turn, RunAgentOutput, RunAgentResult, RunCmd, RunError,
    RunOptions, RunOutput, RunStopReason, StreamOut,
};
pub use tool_cmd::{
    format_tool_show_output, format_tools_list, list_tools, show_tool, ToolShowFormat,
};
