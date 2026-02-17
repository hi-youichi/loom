//! Helve CLI library: CLI parsing and run orchestration for the Helve ReAct agent.
//!
//! Used by the `graphweave` binary. Builds a [`ReactRunner`](graphweave::ReactRunner) from
//! config (env, working folder, etc.) and runs or streams the graph.

pub mod run;
pub mod tool_cmd;

pub use run::{run_agent, RunCmd, RunError, RunOptions};
pub use tool_cmd::{list_tools, show_tool, ToolShowFormat};
