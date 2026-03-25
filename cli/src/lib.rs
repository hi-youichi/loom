//! Helve CLI library: CLI parsing and run orchestration for the Helve ReAct agent.
//!
//! Used by the `loom` binary. Builds a [`ReactRunner`](loom::ReactRunner) from
//! config (env, working folder, etc.) and runs or streams the graph.

pub mod backend;
pub mod envelope;
pub mod log_format;
pub mod model_cmd;
pub mod run;
pub mod tool_cmd;
pub mod tui;

pub use backend::{LocalBackend, RunBackend, RunOutput, StreamOut};
pub use loom::Envelope;
pub use model_cmd::{list_all_models, list_provider_models};
pub use run::{
    run_agent_wrapper as run_agent, RunAgentOutput, RunAgentResult, RunCmd, RunError, RunOptions,
};
pub use tool_cmd::{
    format_tool_show_output, format_tools_list, list_tools, show_tool, ToolShowFormat,
};
