//! Unified slash command system.
//!
//! Provides [`parse`] to detect slash commands in user input, and [`Command`]/[`CommandResult`]
//! types for cross-platform command handling.

pub mod builtins;
pub mod command;
pub mod parser;

pub use builtins::{execute, execute_async, CompactState, ResetState, SummarizeState};
pub use command::{Command, CommandResult};
pub use parser::parse;
