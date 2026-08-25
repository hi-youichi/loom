//! anureo CLI library: CLI parsing and run orchestration for the anureo agents.
//!
//! Used by the `anureo` binary. Builds a [`ReactRunner`](anureo::ReactRunner) from
//! config (env, working folder, etc.) and runs or streams the graph.

pub mod args;
pub mod codex_event_builder;
pub mod display;
pub mod envelope;
pub mod mcp_manager;
pub mod model_cmd;
pub mod profile_convert;
pub mod review_history;
pub mod run;
pub mod server_transport;
pub mod session;
pub mod tool_cmd;

pub use model_cmd::{list_all_models, list_provider_models};
pub use run::{
    cli_list_models, cli_list_tools, cli_show_tool, print_reply_timestamp,
    run_agent_wrapper as run_agent, run_cli_turn, RunAgentOutput, RunAgentResult, RunCmd, RunError,
    RunOptions, RunOutput, RunStopReason, StreamOut,
};
pub use stream_event::Envelope;
pub use tool_cmd::{
    format_tool_show_output, format_tools_list, list_tools, show_tool, ToolShowFormat,
};
