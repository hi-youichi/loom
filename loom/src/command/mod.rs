//! Unified slash command system.
//!
//! Provides [`parse`] to detect slash commands in user input, and [`Command`]/[`CommandResult`]
//! types for cross-platform command handling.
//!
//! This module re-exports from the `loom-commands` crate.

// Re-export everything from loom-commands
pub use loom_commands::*;

// Re-export specific items for backward compatibility
pub use loom_commands::{
    builtins::{execute, execute_async, CompactState, ResetState, SummarizeState},
    command::{Command, CommandResult},
    parser::parse,
};
