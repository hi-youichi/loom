//! Unified slash command system for Loom agents.
//!
//! This crate provides:
//! - [`parse`] to detect slash commands in user input
//! - [`Command`]/[`CommandResult`] types for cross-platform command handling
//! - [`execute`]/[`execute_async`] for built-in command execution
//! - State traits: [`ResetState`], [`CompactState`], [`SummarizeState`]

pub mod builtins;
pub mod command;
pub mod command_traits;
pub mod parser;
pub mod react_impls;

// Re-exports for convenience
pub use builtins::{execute, execute_async};
pub use command::{Command, CommandResult};
pub use command_traits::{CompactState, ResetState, SummarizeState};
pub use parser::parse;
