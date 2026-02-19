//! Helve CLI library: CLI parsing and run orchestration for the Helve ReAct agent.
//!
//! Used by the `graphweave` binary. Builds a [`ReactRunner`](graphweave::ReactRunner) from
//! config (env, working folder, etc.) and runs or streams the graph.

pub mod backend;
pub mod run;
pub mod tool_cmd;

pub use backend::{
    ensure_server_or_spawn, LocalBackend, RemoteBackend, RunBackend, RunOutput, StreamOut,
};
pub use run::{run_agent_wrapper as run_agent, RunAgentResult, RunCmd, RunError, RunOptions};
pub use tool_cmd::{format_tools_list, format_tool_show_output, list_tools, show_tool, ToolShowFormat};
