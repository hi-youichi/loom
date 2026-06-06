pub mod message;
// pub mod runner; // moved to loom-agent; requires run_agent_with_options from loom_agent
pub mod state;
// pub mod tool; // moved to loom-agent; requires run_agent_with_options from loom_agent

pub use message::{build_continuation_prompt, escape_xml_text};
// pub use runner::{GoalRunner, resume, resume_with_event_sender, write_mcp_config}; // moved to loom-agent
pub use state::{
    GoalError, GoalMeta, GoalOutcome, HistoryEntry, ToolError, TurnResult,
    DEFAULT_MAX_ITERATIONS, MAX_CONSECUTIVE_FAILURES, MAX_HISTORY_ENTRIES,
};
// pub use tool::{CodingTool, LoomTool, ShellTool, generate_mcp_config, shell_tool_args}; // moved to loom-agent

#[cfg(test)]
mod tests;
