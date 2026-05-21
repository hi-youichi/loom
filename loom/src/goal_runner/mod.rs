pub mod message;
pub mod runner;
pub mod state;
pub mod tool;

pub use message::{build_continuation_prompt, escape_xml_text};
pub use runner::{GoalRunner, resume, resume_with_event_sender};
pub use state::{
    GoalError, GoalMeta, GoalOutcome, HistoryEntry, ToolError, TurnResult,
    DEFAULT_MAX_ITERATIONS, MAX_CONSECUTIVE_FAILURES, MAX_HISTORY_ENTRIES,
};
pub use tool::{CodingTool, LoomTool, ShellTool, generate_mcp_config};

#[cfg(test)]
mod tests;
